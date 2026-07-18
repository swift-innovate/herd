// ==========================================================================
// Sessions view — agent session list (GET /agent/sessions, polled 10s) +
// detail modal. Sending a message prefers the live WebSocket
// (/agent/sessions/:id/ws) so tool-call/thinking events stream in as they
// happen; falls back to the blocking REST POST when no WS connection is up
// (e.g. no admin key set — the WS route requires ?api_key= since a browser
// WebSocket upgrade can't carry a custom header).
// ==========================================================================

(() => {
  const { api, escapeHtml, toast, fmtRelativeTime } = HerdApp;

  let pollTimer = null;
  let built = false;

  // ---- status → glyph (same vocabulary as Fleet's health glyphs — this is
  // a workflow status, not GPU health, but reusing the token set keeps a
  // single semantic language instead of inventing a second one). ----
  const STATUS = {
    active: { cls: 'health-healthy', glyph: '●' },
    processing: { cls: 'health-degraded', glyph: '◆' },
    completed: { cls: '', glyph: '○' },
    error: { cls: 'health-offline', glyph: '✗' },
  };

  function shortId(id) { return (id || '').slice(0, 8); }

  // ---- list ----------------------------------------------------------------

  function buildDom() {
    const root = document.getElementById('sessions-root');
    root.innerHTML = `
      <div class="view-header">
        <span class="view-title">Sessions</span>
        <span class="view-summary" id="sessions-summary">loading…</span>
      </div>
      <div id="sessions-banner"></div>
      <div class="card" id="sessions-card">
        <div id="sessions-table-head" class="table-head" style="grid-template-columns: 90px 1fr 90px 90px 90px;">
          <span>ID</span><span>Model</span><span>Status</span><span>Messages</span><span>Updated</span>
        </div>
        <div id="sessions-rows"></div>
      </div>`;
    built = true;
  }

  function renderBanner(message, kind = 'warn') {
    const el = document.getElementById('sessions-banner');
    if (!el) return;
    el.innerHTML = message ? `<div class="banner banner-${kind}">${escapeHtml(message)}</div>` : '';
  }

  function renderNotEnabled() {
    const card = document.getElementById('sessions-card');
    card.innerHTML = `
      <div class="empty-state" style="min-height:200px;">
        <div class="empty-state-title">Agent sessions aren't enabled on this gateway</div>
        <div class="empty-state-sub">Set <code>agent.enabled: true</code> in <code>herd.yaml</code> to turn on the agent API and this view.</div>
      </div>`;
  }

  function renderEmpty() {
    const card = document.getElementById('sessions-card');
    card.innerHTML = `
      <div class="empty-state" style="min-height:220px;">
        <div class="empty-state-title">No active sessions</div>
        <div class="empty-state-sub" style="text-align:left;">
          Agent sessions are created via the API:
          <div class="code-block" style="margin-top:10px;">
            <code>curl -X POST ${escapeHtml(window.location.origin)}/agent/sessions \\
  -H "Content-Type: application/json" -H "X-API-Key: &lt;key&gt;" \\
  -d '{"model": "your-model"}'</code>
          </div>
        </div>
      </div>`;
  }

  function rowHtml(s) {
    const st = STATUS[s.status] || { cls: '', glyph: '○' };
    return `
      <div class="table-row" style="grid-template-columns: 90px 1fr 90px 90px 90px;" data-session-id="${escapeHtml(s.id)}">
        <span class="mono" style="font-size:11px;">${escapeHtml(shortId(s.id))}…</span>
        <span style="font-size:12px;">${escapeHtml(s.model)}</span>
        <span class="${st.cls}" style="font-size:12px;">${st.glyph} ${escapeHtml(s.status)}</span>
        <span class="mono" style="font-size:12px;color:var(--text-2);">${s.message_count}</span>
        <span class="mono" style="font-size:11px;color:var(--text-2);">${fmtRelativeTime(s.updated_at)}</span>
      </div>`;
  }

  function renderTable(sessions) {
    const card = document.getElementById('sessions-card');
    if (!card.querySelector('#sessions-rows')) {
      card.innerHTML = `
        <div id="sessions-table-head" class="table-head" style="grid-template-columns: 90px 1fr 90px 90px 90px;">
          <span>ID</span><span>Model</span><span>Status</span><span>Messages</span><span>Updated</span>
        </div>
        <div id="sessions-rows"></div>`;
    }
    const rowsEl = document.getElementById('sessions-rows');
    rowsEl.innerHTML = sessions.map(rowHtml).join('');
    rowsEl.querySelectorAll('.table-row').forEach((el) => {
      el.addEventListener('click', () => openDetail(el.dataset.sessionId));
    });
  }

  async function refreshList() {
    try {
      const sessions = await api('/agent/sessions');
      renderBanner(null);
      const activeCount = sessions.filter((s) => s.status === 'active' || s.status === 'processing').length;
      document.getElementById('sessions-summary').textContent = `${activeCount} active / ${sessions.length} total`;
      document.getElementById('nav-badge-sessions').textContent = activeCount || '';
      if (sessions.length === 0) renderEmpty(); else renderTable(sessions);
    } catch (e) {
      if (e.status === 404 || e.status === 405) {
        document.getElementById('sessions-summary').textContent = 'not enabled';
        renderNotEnabled();
      } else if (e.status === 401) {
        renderBanner('Admin API key required for agent sessions.', 'warn');
      } else {
        renderBanner(`Gateway unreachable — retrying every 10s (${e.message || 'network error'}).`, 'error');
      }
    }
  }

  // ---- detail modal ----------------------------------------------------------

  let liveSocket = null;
  let liveEvents = [];

  function closeLiveSocket() {
    if (liveSocket) { try { liveSocket.close(); } catch (_) {} liveSocket = null; }
    liveEvents = [];
  }

  function roleLabel(role) {
    return { system: 'system', user: 'you', assistant: 'assistant', tool: 'tool' }[role] || role;
  }

  function messageHtml(m) {
    const toolCallsHtml = (m.tool_calls || []).map((tc) => `
      <div class="mono" style="font-size:11px;color:var(--text-2);margin-top:4px;">→ ${escapeHtml(tc.name)}(${escapeHtml(JSON.stringify(tc.arguments))})</div>`).join('');
    return `
      <div class="tile" style="padding:10px 12px;">
        <div class="tile-label">${escapeHtml(roleLabel(m.role))}</div>
        <div style="font-size:12.5px;margin-top:4px;white-space:pre-wrap;">${escapeHtml(m.content)}</div>
        ${toolCallsHtml}
      </div>`;
  }

  function liveEventHtml(ev) {
    switch (ev.type) {
      case 'thinking':
        return `<div class="mono" style="font-size:11px;color:var(--text-3);">… thinking (round ${ev.round})</div>`;
      case 'tool_call':
        return `<div class="mono" style="font-size:11px;color:var(--text-2);">→ ${escapeHtml(ev.tool)}(${escapeHtml(JSON.stringify(ev.arguments))})</div>`;
      case 'tool_result':
        return `<div class="mono" style="font-size:11px;color:${ev.success ? 'var(--text-2)' : 'var(--health-offline)'};">${ev.success ? '✓' : '✗'} ${escapeHtml(ev.tool)}: ${escapeHtml(String(ev.content).slice(0, 200))}</div>`;
      case 'permission_denied':
        return `<div class="banner banner-error" style="margin:4px 0;">⊘ permission denied: ${escapeHtml(ev.tool)} — ${escapeHtml(ev.reason)}</div>`;
      case 'error':
        return `<div class="banner banner-error" style="margin:4px 0;">${escapeHtml(ev.error)}</div>`;
      case 'message':
        return ''; // final content lands in the message list on refetch, not the live strip
      default:
        return '';
    }
  }

  function renderLive() {
    const el = document.getElementById('session-live');
    if (!el) return;
    el.innerHTML = liveEvents.map(liveEventHtml).join('');
    el.scrollTop = el.scrollHeight;
  }

  async function openDetail(id) {
    closeLiveSocket();
    HerdApp.openModal(`
      <div class="modal-title-row">
        <span class="modal-title">Session ${escapeHtml(shortId(id))}…</span>
        <span class="modal-close" id="session-modal-close">✕</span>
      </div>
      <div class="mono" id="session-meta-line" style="font-size:11px;color:var(--text-2);margin-top:2px;">loading…</div>
      <div id="session-messages" style="max-height:320px;overflow-y:auto;margin-top:14px;display:flex;flex-direction:column;gap:8px;"></div>
      <div id="session-live" style="max-height:120px;overflow-y:auto;margin-top:8px;"></div>
      <div class="session-send-row" style="margin-top:14px;display:flex;gap:8px;">
        <input id="session-send-input" placeholder="Send a message…" style="flex:1;">
        <button class="btn btn-primary" id="session-send-btn">Send</button>
      </div>
      <div style="margin-top:14px;display:flex;justify-content:flex-end;">
        <button class="btn btn-danger btn-sm" id="session-delete-btn">Delete session</button>
      </div>`);

    document.getElementById('session-modal-close').addEventListener('click', () => { closeLiveSocket(); HerdApp.closeModal(); });
    document.getElementById('session-delete-btn').addEventListener('click', () => deleteSession(id));
    document.getElementById('session-send-btn').addEventListener('click', () => sendMessage(id));
    document.getElementById('session-send-input').addEventListener('keydown', (e) => {
      if (e.key === 'Enter') sendMessage(id);
    });

    await loadSession(id);
    connectLive(id);
  }

  async function loadSession(id) {
    try {
      const s = await api(`/agent/sessions/${encodeURIComponent(id)}`);
      const metaEl = document.getElementById('session-meta-line');
      if (metaEl) metaEl.textContent = `${s.model} · ${s.status} · updated ${fmtRelativeTime(s.updated_at)}`;
      const msgEl = document.getElementById('session-messages');
      if (msgEl) {
        msgEl.innerHTML = s.messages.map(messageHtml).join('') || '<div style="color:var(--text-3);font-size:12px;">No messages yet.</div>';
        msgEl.scrollTop = msgEl.scrollHeight;
      }
    } catch (e) {
      toast(`Failed to load session: ${e.message || 'unknown error'}`, 'error');
    }
  }

  function connectLive(id) {
    const key = HerdApp.getApiKey();
    if (!key) return; // WS route requires ?api_key= — no key means read-only, REST fallback on send
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${proto}//${window.location.host}/agent/sessions/${encodeURIComponent(id)}/ws?api_key=${encodeURIComponent(key)}`;
    try {
      liveSocket = new WebSocket(url);
    } catch (_) {
      liveSocket = null;
      return;
    }
    liveSocket.onmessage = (ev) => {
      let data;
      try { data = JSON.parse(ev.data); } catch (_) { return; }
      liveEvents.push(data);
      renderLive();
      if (data.type === 'message' || data.type === 'error') {
        // Turn finished — reload the authoritative message list and clear the live strip.
        liveEvents = [];
        setTimeout(() => { renderLive(); loadSession(id); refreshList(); }, 150);
      }
    };
    liveSocket.onerror = () => { liveSocket = null; };
    liveSocket.onclose = () => { liveSocket = null; };
  }

  async function sendMessage(id) {
    const input = document.getElementById('session-send-input');
    const content = input.value.trim();
    if (!content) return;
    if (!HerdApp.getApiKey()) { toast('Admin API key required to send messages.', 'error'); return; }
    input.value = '';

    if (liveSocket && liveSocket.readyState === WebSocket.OPEN) {
      liveSocket.send(JSON.stringify({ content }));
      return;
    }

    // Fallback: blocking REST call — no incremental events, just the final result.
    try {
      await api(`/agent/sessions/${encodeURIComponent(id)}/messages`, { method: 'POST', body: { content } });
      await loadSession(id);
      refreshList();
    } catch (e) {
      toast(`Send failed: ${e.message || 'unknown error'}`, 'error');
    }
  }

  async function deleteSession(id) {
    try {
      await api(`/agent/sessions/${encodeURIComponent(id)}`, { method: 'DELETE' });
      closeLiveSocket();
      HerdApp.closeModal();
      refreshList();
      toast('Session deleted');
    } catch (e) {
      toast(`Failed to delete session: ${e.message || 'unknown error'}`, 'error');
    }
  }

  // ---- lifecycle -------------------------------------------------------------

  function mount() {
    if (!built) buildDom();
  }

  function start() {
    refreshList();
    pollTimer = setInterval(refreshList, 10000);
  }

  function stop() {
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
    closeLiveSocket();
  }

  HerdApp.registerView('sessions', { mount, start, stop, pollSeconds: 10 });
})();
