// ==========================================================================
// Fleet screen (frame 5a) — one row per physical node, merging the agent
// node registry (`/api/nodes`) with the routing pool (`/status` +
// `/admin/config/backends`). Same physical GPU can appear as a static/
// enrolled pool backend AND an agent-registered node until backend-side
// dedup lands (v1.4, per README) — this view is where that merge happens.
// ==========================================================================

(() => {
  const { api, escapeHtml, fmtGb, fmtPct, fmtRelativeTime, toast, normalizeUrl } = HerdApp;

  let pollTimer = null;

  // ---- health-state derivation ------------------------------------------
  // Node registry exposes: "healthy" | "degraded" | "unreachable".
  // Backend pool exposes only a bool `healthy`. Neither layer currently
  // surfaces a distinct circuit-breaker-open state as its own field, so
  // "circuit open" isn't reachable from today's API — glyph reserved for
  // when that's wired (Phase 2 candidate), not synthesized here.
  function healthFromNode(node) {
    switch (node.status) {
      case 'healthy': return 'healthy';
      case 'degraded': return 'degraded';
      case 'unreachable': return 'offline';
      default: return 'degraded';
    }
  }

  function healthFromBackend(backend) {
    if (!backend.healthy) return 'offline';
    if (!backend.gpu) return 'degraded'; // reachable, but no telemetry reported
    return 'healthy';
  }

  const HEALTH_GLYPH = {
    healthy: { glyph: '●', cls: 'health-healthy' },
    degraded: { glyph: '◆', cls: 'health-degraded' },
    offline: { glyph: '○', cls: 'health-offline' },
    circuit: { glyph: '⊘', cls: 'health-circuit' },
  };

  // ---- fetch + merge -----------------------------------------------------

  async function fetchAll() {
    const [nodesRes, statusRes, backendsRes] = await Promise.all([
      api('/api/nodes').catch(() => ({ nodes: [] })),
      api('/status').catch(() => null),
      api('/admin/config/backends').catch(() => null), // admin-gated; absent in read-only mode
    ]);

    const nodes = (nodesRes && nodesRes.nodes) || [];
    const healthyBackends = (statusRes && statusRes.healthy_backends) || [];
    const unhealthyBackends = (statusRes && statusRes.unhealthy_backends) || [];
    const allBackends = [...healthyBackends, ...unhealthyBackends];
    const effectiveByName = new Map();
    if (Array.isArray(backendsRes)) {
      backendsRes.forEach((b) => effectiveByName.set(b.name, b));
    } else if (backendsRes && Array.isArray(backendsRes.backends)) {
      backendsRes.backends.forEach((b) => effectiveByName.set(b.name, b));
    }

    // Join key: Node.backend_url vs backend-pool url (normalized). A node
    // record wins the merged row when both exist for the same physical
    // host — it carries agent_version/source/gpu telemetry the static pool
    // entry doesn't have. A pool-only backend (no agent) still gets a row.
    const byUrl = new Map();
    nodes.forEach((n) => {
      byUrl.set(normalizeUrl(n.backend_url), { node: n, backend: null });
    });
    allBackends.forEach((b) => {
      const key = normalizeUrl(b.url);
      const existing = byUrl.get(key);
      if (existing) {
        existing.backend = b;
      } else {
        byUrl.set(key, { node: null, backend: b });
      }
    });

    const rows = [];
    for (const { node, backend } of byUrl.values()) {
      const effective = backend ? effectiveByName.get(backend.name) : null;
      rows.push(buildRow(node, backend, effective));
    }
    rows.sort((a, b) => a.name.localeCompare(b.name));
    return rows;
  }

  function buildRow(node, backend, effective) {
    const name = (node && node.hostname) || (backend && backend.name) || 'unknown';
    const backendType = (node && node.backend) || (backend && backend.backend) || (effective && effective.backend) || '—';

    let health, healthNote = null;
    if (node) {
      health = healthFromNode(node);
    } else if (backend) {
      health = healthFromBackend(backend);
    } else {
      health = 'offline';
    }
    if (health === 'offline' && node && node.last_seen) {
      healthNote = `offline · last seen ${fmtRelativeTime(node.last_seen)}`;
    }

    // Telemetry: prefer node record GPU fields, fall back to backend-pool gpu block.
    const gpu = backend && backend.gpu ? backend.gpu : null;
    const hasTelemetry = !!gpu;
    const vramUsedMb = gpu ? gpu.memory_used : null;
    const vramTotalMb = gpu ? gpu.memory_total : (backend ? backend.vram_total_mb : null);
    const vramPct = gpu && gpu.memory_total ? Math.min(100, (gpu.memory_used / gpu.memory_total) * 100) : null;
    const util = gpu ? gpu.utilization : null;
    const temp = gpu ? gpu.temperature : null;

    const models = (backend && backend.models) || (node && node.models_loaded) || [];
    const hotModels = new Set((effective && effective.hot_models) || (backend && backend.hot_models) || []);
    const modelsLabel = models.length
      ? models.slice(0, 1).map((m) => (hotModels.has(m) ? `${m} ★` : m)).join(', ') +
        (models.length > 1 ? ` +${models.length - 1}` : '')
      : (node ? `${node.installed_model_count || 0} installed` : '—');

    const agentVersion = node && node.agent_version;
    const source = node ? node.source : 'static';

    return {
      id: (node && node.id) || (backend && backend.name),
      name, backendType, health, healthNote, hasTelemetry,
      vramUsedMb, vramTotalMb, vramPct, util, temp, modelsLabel, agentVersion, source,
      node, backend,
    };
  }

  // ---- render -------------------------------------------------------------

  function renderSummary(rows, statusRes) {
    const online = rows.filter((r) => r.health === 'healthy' || r.health === 'degraded').length;
    const total = rows.length;
    document.getElementById('nav-badge-fleet').textContent = `${online}/${total}`;

    const el = document.getElementById('fleet-summary');
    if (!el) return;
    if (total === 0) { el.textContent = ''; return; }
    const vramUsed = rows.reduce((sum, r) => sum + (r.vramUsedMb || 0), 0);
    const vramTotal = rows.reduce((sum, r) => sum + (r.vramTotalMb || 0), 0);
    el.textContent = `${fmtGb(vramUsed)}/${fmtGb(vramTotal)} GB VRAM · ${rows.filter(r=>r.health==='healthy').length} healthy backends`;
  }

  function rowHtml(r) {
    const h = HEALTH_GLYPH[r.health];
    const opacity = r.health === 'offline' ? 0.55 : 1;
    const telemetryCell = r.hasTelemetry
      ? `<span style="display:flex;align-items:center;gap:8px;">
           <span class="bar-track" style="flex:1;"><span class="bar-fill" style="width:${r.vramPct.toFixed(0)}%"></span></span>
           <span class="mono" style="font-size:11px;color:var(--text-2);">${fmtGb(r.vramUsedMb)}/${fmtGb(r.vramTotalMb)}</span>
         </span>`
      : `<span class="mono" style="font-size:11px;color:var(--text-3);">${escapeHtml(r.healthNote || '— no telemetry')}</span>`;

    return `
      <div class="table-row" style="grid-template-columns: 150px 100px 200px 64px 52px 1fr 90px 20px; opacity:${opacity};" data-node-id="${escapeHtml(r.id)}" data-kind="${r.node ? 'node' : 'backend'}">
        <span style="font-weight:600;"><span class="${h.cls}">${h.glyph}</span> ${escapeHtml(r.name)}</span>
        <span class="mono" style="font-size:11px;color:var(--text-2);">${escapeHtml(r.backendType)}</span>
        ${telemetryCell}
        <span class="mono" style="font-size:12px;color:var(--text-2);">${r.util !== null ? fmtPct(r.util) : '—'}</span>
        <span class="mono" style="font-size:12px;color:var(--text-2);">${r.temp !== null ? `${r.temp}°` : '—'}</span>
        <span class="mono" style="font-size:11px;color:var(--text-2);">${escapeHtml(r.modelsLabel)}</span>
        <span class="mono" style="font-size:11px;color:${r.agentVersion ? 'var(--text-2)' : 'var(--text-3)'};">${escapeHtml(r.agentVersion || (r.source === 'static' ? 'static' : '—'))}</span>
        <span style="color:var(--text-3);">›</span>
      </div>`;
  }

  function renderEmptyFirstRun() {
    const card = document.getElementById('fleet-card');
    card.innerHTML = `
      <div class="empty-state" style="min-height:480px; justify-content:center;">
        <img data-herd-mark alt="" width="96" height="96" style="border-radius:22px;">
        <div class="empty-state-title">No nodes in the herd yet</div>
        <div style="width:640px; margin-top:10px;">
          <div class="tab-strip">
            <span class="active">bash</span><span>PowerShell</span>
          </div>
          <div class="code-block">
            <code>$ curl -fsSL {{origin}}/api/nodes/script?os=linux | sh</code>
            <button class="btn btn-primary" id="fleet-copy-join">Copy</button>
          </div>
          <div class="empty-state-sub" style="margin-top:10px;">Run this on any machine with a GPU. It'll show up here on first heartbeat.</div>
        </div>
      </div>`;
    if (typeof applyHerdMark === 'function') applyHerdMark();
    const codeEl = card.querySelector('code');
    if (codeEl) codeEl.textContent = codeEl.textContent.replace('{{origin}}', window.location.origin);
    const copyBtn = document.getElementById('fleet-copy-join');
    if (copyBtn) copyBtn.addEventListener('click', () => {
      navigator.clipboard.writeText(codeEl.textContent.replace(/^\$ /, ''));
      toast('Copied to clipboard');
    });
  }

  function renderTable(rows) {
    const card = document.getElementById('fleet-card');
    // Empty-state replaces the whole card; restore table skeleton on next non-empty render.
    if (!card.querySelector('#fleet-rows')) {
      card.innerHTML = `
        <div id="fleet-table-head" class="table-head" style="grid-template-columns: 150px 100px 200px 64px 52px 1fr 90px 20px;">
          <span>Node</span><span>Backend</span><span>VRAM ▾</span><span>Util</span><span>Temp</span><span>Models</span><span>Agent</span><span></span>
        </div>
        <div id="fleet-rows"></div>`;
    }
    const rowsEl = document.getElementById('fleet-rows');
    rowsEl.innerHTML = rows.map(rowHtml).join('');
    rowsEl.querySelectorAll('.table-row').forEach((el) => {
      el.addEventListener('click', () => {
        window.HerdNodeDetail && window.HerdNodeDetail.open(el.dataset.nodeId, el.dataset.kind);
      });
    });
  }

  function renderBanner(message, kind = 'warn') {
    const el = document.getElementById('fleet-banner');
    if (!message) { el.innerHTML = ''; return; }
    el.innerHTML = `<div class="banner banner-${kind}">${escapeHtml(message)}</div>`;
  }

  async function refresh() {
    try {
      const rows = await fetchAll();
      renderBanner(null);
      if (rows.length === 0) {
        renderEmptyFirstRun();
      } else {
        renderTable(rows);
      }
      renderSummary(rows);
    } catch (e) {
      if (e.status === 401) {
        renderBanner('Admin API key required for full fleet data — showing read-only view.', 'warn');
      } else {
        renderBanner(`Gateway unreachable — retrying every 5s (${e.message || 'network error'}).`, 'error');
      }
    }
  }

  function mount() {
    // no one-time DOM wiring beyond what shell_body.html already has
  }

  function start() {
    refresh();
    pollTimer = setInterval(refresh, 5000);
  }

  function stop() {
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
  }

  // Registered synchronously (not on DOMContentLoaded) — this script runs
  // after shell_body.html has already been parsed, so the DOM it touches
  // exists, and app.js's own DOMContentLoaded handler (which picks the
  // initial view) must not run before this registration does.
  HerdApp.registerView('fleet', { mount, start, stop, pollSeconds: 5 });

  const addNodeBtn = document.getElementById('btn-add-node');
  if (addNodeBtn) addNodeBtn.addEventListener('click', () => {
    HerdApp.openModal(`
      <div class="modal-title-row"><span class="modal-title">Add a node</span><span class="modal-close" id="modal-close-x">✕</span></div>
      <div class="empty-state-sub" style="margin-top:10px; text-align:left;">
        One-time join tokens land in Phase 2 (needs the heartbeat task envelope). For now, run
        the existing enrollment script:
      </div>
      <div class="code-block" style="margin-top:14px;">
        <code>curl -fsSL ${escapeHtml(window.location.origin)}/api/nodes/script?os=linux | sh</code>
      </div>`);
    document.getElementById('modal-close-x').addEventListener('click', HerdApp.closeModal);
  });

  const updateFleetBtn = document.getElementById('btn-update-fleet');
  if (updateFleetBtn) updateFleetBtn.addEventListener('click', () => {
    HerdApp.toast('Fleet-wide update UI lands in Phase 2 — version authority exists, no dashboard trigger yet.', 'error');
  });
})();
