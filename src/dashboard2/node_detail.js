// ==========================================================================
// Node detail (frame 5b) — Telemetry / Models & Routing / Activity tabs.
// Routing allowlist + hot-model controls only apply when this node has a
// matching entry in the routing pool (backend.url == node.backend_url) —
// an agent-only node with no pool entry shows those controls as read-only.
// ==========================================================================

window.HerdNodeDetail = (() => {
  const { api, escapeHtml, fmtPct, fmtRelativeTime, toast, normalizeUrl } = HerdApp;

  let currentNodeId = null;
  let currentKind = 'node'; // 'node' (agent-registered) | 'backend' (routing-pool-only, no agent)
  let currentTab = 'telemetry';

  async function open(nodeId, kind) {
    currentNodeId = nodeId;
    currentKind = kind || 'node';
    currentTab = 'telemetry';
    HerdApp.switchView('node');
    await render();
  }

  async function render() {
    const root = document.getElementById('node-detail-root');
    root.innerHTML = '<div class="skeleton-row"></div><div class="skeleton-row"></div><div class="skeleton-row"></div>';
    if (currentKind === 'backend') {
      await renderBackendDetail(root);
    } else {
      await renderNodeDetail(root);
    }
  }

  async function renderNodeDetail(root) {
    let node, modelsResp, effective;
    try {
      node = await api(`/api/nodes/${encodeURIComponent(currentNodeId)}`);
    } catch (e) {
      root.innerHTML = `<div class="banner banner-error">Failed to load node: ${escapeHtml(e.message || 'unknown error')}</div>`;
      return;
    }
    try {
      modelsResp = await api(`/api/nodes/${encodeURIComponent(currentNodeId)}/models`);
    } catch (e) {
      modelsResp = null;
    }
    try {
      const backends = await api('/admin/config/backends');
      const list = Array.isArray(backends) ? backends : (backends && backends.backends) || [];
      effective = list.find((b) => normalizeUrl(b.url) === normalizeUrl(node.backend_url)) || null;
    } catch (e) {
      effective = null; // admin-gated or no match — routing controls degrade to read-only
    }

    // Live GPU telemetry comes from the pool's /status entry, not the node
    // registry record — reuse the same match to pull it if present.
    let gpu = null, poolHealthy = null;
    try {
      const status = await api('/status');
      const all = [...(status.healthy_backends || []), ...(status.unhealthy_backends || [])];
      const match = all.find((b) => normalizeUrl(b.url) === normalizeUrl(node.backend_url));
      if (match) { gpu = match.gpu || null; poolHealthy = match.healthy; }
    } catch (e) { /* best-effort */ }

    root.innerHTML = renderShell(node, gpu, poolHealthy);
    wireHeader(node);
    wireTabs(node);
    renderTabBody(node, modelsResp, effective, gpu);
  }

  // ---- backend-only detail (routing-pool entry with no agent-registered
  // node — e.g. a backend configured directly in herd.yaml). There's no
  // /api/nodes/:id record to fetch here at all, so this pulls straight from
  // /status (live telemetry) and /admin/config/backends (routing config)
  // instead of erroring out with a "node not found" dead end. ----

  async function renderBackendDetail(root) {
    let backend = null, effective = null;
    try {
      const status = await api('/status');
      const all = [...(status.healthy_backends || []), ...(status.unhealthy_backends || [])];
      backend = all.find((b) => b.name === currentNodeId) || null;
    } catch (e) { /* best-effort */ }
    try {
      const backends = await api('/admin/config/backends');
      const list = Array.isArray(backends) ? backends : (backends && backends.backends) || [];
      effective = list.find((b) => b.name === currentNodeId) || null;
    } catch (e) {
      effective = null; // admin-gated — routing controls degrade to read-only
    }

    if (!backend && !effective) {
      root.innerHTML = `<div class="banner banner-error">Failed to load backend: not found in the routing pool.</div>`;
      return;
    }

    root.innerHTML = renderBackendShell(backend, effective);
    wireBackendHeader(effective);
    wireTabs();
    renderBackendTabBody(backend, effective);
  }

  function renderBackendShell(backend, effective) {
    const name = currentNodeId;
    const healthy = backend ? backend.healthy : (effective ? effective.healthy : false);
    const dot = healthy ? { cls: 'health-healthy', glyph: '●' } : { cls: 'health-offline', glyph: '○' };
    const backendType = (effective && effective.backend) || '—';
    const url = (effective && effective.url) || '—';
    const priority = effective ? effective.priority : null;
    return `
      <div class="mono" style="font-size:11px;color:var(--text-3);">
        Fleet / <span style="color:var(--text-1);">${escapeHtml(name)}</span>
        <span style="margin-left:8px;font-size:10px;">source: static</span>
      </div>
      <div style="display:flex;align-items:center;gap:10px;margin-top:8px;flex-wrap:wrap;">
        <span class="${dot.cls}" style="font-size:15px;">${dot.glyph}</span>
        <span style="font-weight:700;font-size:18px;white-space:nowrap;">${escapeHtml(name)}</span>
        <span class="mono" style="font-size:11px;color:var(--text-2);white-space:nowrap;">${escapeHtml(backendType)} · ${escapeHtml(url)}${priority !== null ? ` · priority ${priority}` : ''}</span>
        <span style="margin-left:auto;display:flex;gap:6px;" id="node-actions"></span>
      </div>
      <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-top:14px;" id="node-telemetry-tiles"></div>
      <div class="section-tabs" id="node-tabs">
        <span data-tab="telemetry">Telemetry</span>
        <span data-tab="models">Models &amp; Routing</span>
        <span data-tab="activity">Activity</span>
      </div>
      <div id="node-tab-body" style="margin-top:12px;"></div>`;
  }

  function wireBackendHeader(effective) {
    const actions = document.getElementById('node-actions');
    const enabled = effective ? effective.enabled : true;
    const buttons = [
      { label: 'Restart backend', cls: 'btn-secondary', onClick: () => toast('Restart needs the agent-owns-lifecycle prerequisite (Phase 2) — not wired yet.', 'error') },
      { label: 'Install model', cls: 'btn-secondary', onClick: () => toast('Install flow needs the fit endpoint + task envelope (Phase 2).', 'error') },
      { label: enabled ? 'Disable' : 'Enable', cls: 'btn-secondary', onClick: () => toggleBackendEnabled(enabled) },
    ];
    // No Remove button — static backends (configured directly in herd.yaml
    // or the config overlay) have no delete endpoint to call.
    actions.innerHTML = buttons.map((b, i) => `<button class="btn btn-sm ${b.cls}" data-i="${i}">${escapeHtml(b.label)}</button>`).join('');
    actions.querySelectorAll('button').forEach((btn, i) => btn.addEventListener('click', buttons[i].onClick));
  }

  async function toggleBackendEnabled(currentlyEnabled) {
    try {
      await api(`/admin/config/backends/${encodeURIComponent(currentNodeId)}`, { method: 'PUT', body: { enabled: !currentlyEnabled } });
      toast(`Backend ${currentlyEnabled ? 'disabled' : 'enabled'}.`);
      render();
    } catch (e) {
      toast(`Failed: ${e.message || 'unknown error'}`, 'error');
    }
  }

  function renderBackendTabBody(backend, effective) {
    const gpu = backend && backend.gpu ? backend.gpu : null;
    renderTelemetryTiles({ source: 'static' }, gpu);
    const body = document.getElementById('node-tab-body');
    if (currentTab === 'telemetry') {
      body.innerHTML = `<div class="empty-state-sub" style="text-align:left;">Telemetry tiles above refresh with the rest of the page; this tab is intentionally the same view as the header strip.</div>`;
    } else if (currentTab === 'models') {
      const modelsResp = backend ? { models_loaded: backend.models || [] } : null;
      body.innerHTML = renderModelsRouting(null, modelsResp, effective);
      wireModelsRouting(null, effective);
    } else {
      body.innerHTML = renderActivity();
    }
  }

  function healthDot(node, poolHealthy) {
    if (node.status === 'healthy' || poolHealthy === true) return { cls: 'health-healthy', glyph: '●' };
    if (node.status === 'degraded') return { cls: 'health-degraded', glyph: '◆' };
    return { cls: 'health-offline', glyph: '○' };
  }

  function renderShell(node, gpu, poolHealthy) {
    const dot = healthDot(node, poolHealthy);
    const uptime = node.registered_at ? fmtRelativeTime(node.registered_at) : '—';
    return `
      <div class="mono" style="font-size:11px;color:var(--text-3);">
        Fleet / <span style="color:var(--text-1);">${escapeHtml(node.hostname)}</span>
        <span style="margin-left:8px;font-size:10px;">source: ${escapeHtml(node.source)}</span>
      </div>
      <div style="display:flex;align-items:center;gap:10px;margin-top:8px;flex-wrap:wrap;">
        <span class="${dot.cls}" style="font-size:15px;">${dot.glyph}</span>
        <span style="font-weight:700;font-size:18px;white-space:nowrap;">${escapeHtml(node.hostname)}</span>
        <span class="mono" style="font-size:11px;color:var(--text-2);white-space:nowrap;">${escapeHtml(node.backend)} · ${escapeHtml(node.backend_url)} · ${escapeHtml(node.backend_version || node.agent_version || '—')} · registered ${uptime}</span>
        ${(node.tags || []).map((t) => `<span class="pill">${escapeHtml(t)}</span>`).join('')}
        <span style="margin-left:auto;display:flex;gap:6px;" id="node-actions"></span>
      </div>
      <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-top:14px;" id="node-telemetry-tiles"></div>
      <div class="section-tabs" id="node-tabs">
        <span data-tab="telemetry">Telemetry</span>
        <span data-tab="models">Models &amp; Routing</span>
        <span data-tab="activity">Activity</span>
      </div>
      <div id="node-tab-body" style="margin-top:12px;"></div>`;
  }

  function wireHeader(node) {
    const actions = document.getElementById('node-actions');
    const buttons = [
      { label: 'Restart backend', cls: 'btn-secondary', onClick: () => toast('Restart needs the agent-owns-lifecycle prerequisite (Phase 2) — not wired yet.', 'error') },
      { label: 'Install model', cls: 'btn-secondary', onClick: () => toast('Install flow needs the fit endpoint + task envelope (Phase 2).', 'error') },
      { label: node.enabled ? 'Disable' : 'Enable', cls: 'btn-secondary', onClick: () => toggleEnabled(node) },
      { label: 'Remove', cls: 'btn-danger', onClick: () => confirmRemove(node) },
    ];
    actions.innerHTML = buttons.map((b, i) => `<button class="btn btn-sm ${b.cls}" data-i="${i}">${escapeHtml(b.label)}</button>`).join('');
    actions.querySelectorAll('button').forEach((btn, i) => btn.addEventListener('click', buttons[i].onClick));

    const tiles = document.getElementById('node-telemetry-tiles');
  }

  async function toggleEnabled(node) {
    try {
      await api(`/api/nodes/${encodeURIComponent(node.id)}`, { method: 'PUT', body: { enabled: !node.enabled } });
      toast(`Node ${node.enabled ? 'disabled' : 'enabled'}.`);
      render();
    } catch (e) {
      toast(`Failed: ${e.message || 'unknown error'}`, 'error');
    }
  }

  function confirmRemove(node) {
    HerdApp.openModal(`
      <div class="modal-title-row"><span class="modal-title">Remove ${escapeHtml(node.hostname)}?</span><span class="modal-close" id="modal-close-x">✕</span></div>
      <div class="empty-state-sub" style="margin-top:10px;text-align:left;">Removes this node from the registry. It reappears on its next heartbeat if the agent is still running.</div>
      <div style="display:flex;gap:8px;margin-top:16px;justify-content:flex-end;">
        <button class="btn btn-secondary" id="cancel-remove">Cancel</button>
        <button class="btn btn-danger" id="confirm-remove">Remove</button>
      </div>`);
    document.getElementById('modal-close-x').addEventListener('click', HerdApp.closeModal);
    document.getElementById('cancel-remove').addEventListener('click', HerdApp.closeModal);
    document.getElementById('confirm-remove').addEventListener('click', async () => {
      try {
        await api(`/api/nodes/${encodeURIComponent(node.id)}`, { method: 'DELETE' });
        HerdApp.closeModal();
        toast('Node removed.');
        HerdApp.switchView('fleet');
      } catch (e) {
        toast(`Failed: ${e.message || 'unknown error'}`, 'error');
      }
    });
  }

  function tile(label, valueHtml, sub) {
    return `<div class="tile"><div class="tile-label">${escapeHtml(label)}</div><div class="tile-value">${valueHtml}</div>${sub ? `<div class="tile-sub">${escapeHtml(sub)}</div>` : ''}</div>`;
  }

  function renderTelemetryTiles(node, gpu) {
    const tiles = document.getElementById('node-telemetry-tiles');
    if (gpu) {
      const vramUsed = (gpu.memory_used / 1024).toFixed(1);
      const vramTotal = (gpu.memory_total / 1024).toFixed(1);
      const pct = Math.min(100, (gpu.memory_used / gpu.memory_total) * 100);
      tiles.innerHTML = `
        <div class="tile">
          <div class="tile-label">GPU${node.gpu_model ? ` · ${escapeHtml(node.gpu_model)}` : ''}</div>
          <div class="tile-value">${vramUsed}<span class="unit">/${vramTotal} GB</span></div>
          <div class="bar-track" style="margin-top:6px;"><div class="bar-fill" style="width:${pct.toFixed(0)}%"></div></div>
          <div class="tile-sub">${fmtPct(gpu.utilization)} util · ${gpu.temperature}°C</div>
        </div>
        ${tile('Queue', '<span class="muted">not reported</span>', 'backend does not expose slots via /status yet')}
        ${tile('TTFT p50', '<span class="muted">not reported</span>', 'backend does not expose this yet')}
        ${tile('Power', '<span class="muted">not reported</span>', 'backend does not expose this')}`;
    } else {
      tiles.innerHTML = `
        <div class="tile" style="grid-column: span 4;">
          <div class="tile-label">Telemetry</div>
          <div class="tile-value muted">not reported</div>
          <div class="tile-sub">${node.source === 'agent' ? 'no matching routing-pool entry — this node has no live GPU telemetry source' : 'static/enrolled node — no agent telemetry'}</div>
        </div>`;
    }
  }

  function renderModelsRouting(node, modelsResp, effective) {
    const installed = (modelsResp && modelsResp.models_loaded) || [];
    const registry = (modelsResp && modelsResp.model_registry) || null;
    const hotModels = new Set((effective && effective.hot_models) || []);
    const modelsEnabled = effective ? effective.models_enabled : undefined; // undefined = no match, null = "route all"

    const rows = (registry || installed.map((name) => ({ file_name: name, loaded: true }))).map((m) => {
      const name = m.file_name;
      const routed = modelsEnabled === undefined ? null : (modelsEnabled === null || modelsEnabled.includes(name));
      const hot = hotModels.has(name);
      return `
        <div class="table-row" style="grid-template-columns:30px 1fr 90px 90px 60px 110px;">
          <span>${routed === null ? '<span class="mono" style="font-size:10px;color:var(--text-3);">n/a</span>' : `<input type="checkbox" ${routed ? 'checked' : ''} data-model="${escapeHtml(name)}" class="route-toggle" ${effective ? '' : 'disabled'}>`}</span>
          <span class="mono" style="font-size:12px;">${escapeHtml(name)}</span>
          <span class="mono" style="font-size:11px;color:var(--text-2);">${m.loaded ? 'resident' : ''}</span>
          <span style="font-size:11px;color:${hot ? 'var(--amber)' : 'var(--text-3)'};cursor:${effective ? 'pointer' : 'default'};" class="hot-toggle" data-model="${escapeHtml(name)}">${hot ? '★ hot' : 'pin'}</span>
        </div>`;
    }).join('');

    return `
      ${effective ? '' : '<div class="banner banner-warn">No matching routing-pool backend for this node — routing allowlist and hot-model controls are read-only.</div>'}
      <div class="table-head" style="grid-template-columns:30px 1fr 90px 90px 60px 110px;">
        <span>Route</span><span>Model</span><span>State</span><span>Hot</span><span></span>
      </div>
      <div id="node-models-rows">${rows || '<div style="padding:20px;color:var(--text-3);font-size:12px;">No models installed on this node.</div>'}</div>
      <div style="margin-top:10px;font-size:11px;color:var(--text-3);">Hot ★ takes chrome amber — it's a user setting, not a health state.</div>`;
  }

  function renderActivity() {
    return `
      <div class="empty-state-sub" style="text-align:left;">
        Per-node activity filtering isn't exposed by <code>/analytics</code> yet — see the
        full Analytics view for fleet-wide request volume and latency. Node-scoped activity
        (requests routed here, errors, circuit-breaker events) is a parity item to fold in
        once the analytics endpoint accepts a node/backend filter.
      </div>`;
  }

  function renderTabBody(node, modelsResp, effective, gpu) {
    renderTelemetryTiles(node, gpu);
    const body = document.getElementById('node-tab-body');
    if (currentTab === 'telemetry') {
      body.innerHTML = `<div class="empty-state-sub" style="text-align:left;">Telemetry tiles above refresh with the rest of the page; this tab is intentionally the same view as the header strip.</div>`;
    } else if (currentTab === 'models') {
      body.innerHTML = renderModelsRouting(node, modelsResp, effective);
      wireModelsRouting(node, effective);
    } else {
      body.innerHTML = renderActivity();
    }
  }

  function wireModelsRouting(node, effective) {
    if (!effective) return;
    document.querySelectorAll('.route-toggle').forEach((cb) => {
      cb.addEventListener('change', async () => {
        const current = new Set(effective.models_enabled || []);
        if (cb.checked) current.add(cb.dataset.model); else current.delete(cb.dataset.model);
        try {
          await api(`/admin/config/backends/${encodeURIComponent(effective.name)}/models`, {
            method: 'PUT', body: { models_enabled: Array.from(current) },
          });
          toast('Routing allowlist updated.');
        } catch (e) {
          toast(`Failed: ${e.message || 'unknown error'}`, 'error');
          cb.checked = !cb.checked;
        }
      });
    });
    document.querySelectorAll('.hot-toggle').forEach((el) => {
      el.addEventListener('click', async () => {
        const hot = new Set(effective.hot_models || []);
        const name = el.dataset.model;
        if (hot.has(name)) hot.delete(name); else hot.add(name);
        try {
          await api(`/admin/config/backends/${encodeURIComponent(effective.name)}`, {
            method: 'PUT', body: { hot_models: Array.from(hot) },
          });
          toast('Hot models updated.');
          render();
        } catch (e) {
          toast(`Failed: ${e.message || 'unknown error'}`, 'error');
        }
      });
    });
  }

  function wireTabs() {
    document.querySelectorAll('#node-tabs span').forEach((el) => {
      el.classList.toggle('active', el.dataset.tab === currentTab);
      el.addEventListener('click', () => {
        currentTab = el.dataset.tab;
        render();
      });
    });
  }

  // Registered synchronously — see fleet.js for why this can't wait on
  // DOMContentLoaded (app.js's initial-view switch would race it).
  HerdApp.registerView('node', { mount: () => {}, start: () => {}, stop: () => {} });

  return { open };
})();
