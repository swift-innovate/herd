// ==========================================================================
// Settings view — config editor (GET/PUT /admin/config, secret redaction),
// per-backend model-routing allowlists (port of the old dashboard's #G3
// Model Routing UI), config-overrides table, and a static Agent Guide
// reference. No auto-poll: this view holds an in-progress edit buffer
// (`fullConfig`), and blindly refetching under the user while they type
// would clobber it — refresh is manual (Refresh buttons), same as the old
// dashboard's Settings tab.
// ==========================================================================

(() => {
  const { api, escapeHtml, attrEscape, toast } = HerdApp;

  let built = false;
  // The full config object from GET /admin/config, mutated in place by the
  // form and PUT back whole on Save. Sub-trees the form has no UI for
  // (routing.auto, routing.scored, routing_profiles, tls, rate_limiting,
  // budget, discovery, fleet, frontier, providers, task_classifier,
  // agent.permissions, data_dir) are never touched, so they round-trip
  // unchanged — a partial rebuild-from-form-fields (what the old dashboard
  // did) would silently reset all of those to their defaults on every save.
  let fullConfig = null;

  function el(id) { return document.getElementById(id); }
  function val(id) { return el(id).value; }
  function setVal(id, v) { el(id).value = v; }
  function setChecked(id, v) { el(id).checked = !!v; }

  // ---- DOM shell ------------------------------------------------------------

  function buildDom() {
    const root = document.getElementById('settings-root');
    root.innerHTML = `
      <div class="view-header">
        <span class="view-title">Settings</span>
        <div class="view-actions">
          <button class="btn btn-secondary btn-sm" id="settings-refresh">Refresh</button>
        </div>
      </div>
      <div id="settings-banner"></div>

      <div id="settings-locked" class="card" style="display:none;">
        <div class="empty-state">
          <div class="empty-state-title">API key required</div>
          <div class="empty-state-sub">Enter your admin API key in the sidebar to view and edit settings.</div>
        </div>
      </div>

      <div id="settings-body" style="display:none;">

        <div class="card mr-section" style="padding:16px;margin-bottom:16px;">
          <div class="view-header" style="margin-bottom:2px;">
            <span class="card-title" style="margin-bottom:0;">Model Routing</span>
            <div class="view-actions"><button class="btn btn-secondary btn-sm" id="mr-refresh">Refresh</button></div>
          </div>
          <div class="empty-state-sub" style="text-align:left;margin-bottom:10px;">
            Choose which installed models Herd routes to on each backend. Changes apply within one discovery tick — no restart.
          </div>
          <div id="mr-cards"><div class="skeleton-row"></div></div>
        </div>

        <div class="card" style="padding:16px;margin-bottom:16px;">
          <div class="view-header" style="margin-bottom:8px;">
            <span class="card-title" style="margin-bottom:0;">Config Overrides</span>
            <div class="view-actions"><button class="btn btn-secondary btn-sm" id="overrides-refresh">Refresh</button></div>
          </div>
          <div class="empty-state-sub" style="text-align:left;margin-bottom:10px;">
            GUI-managed overrides layered on top of <code>herd.yaml</code> (the overlay wins and survives restarts). Delete a row to restore the YAML value.
          </div>
          <div id="overrides-table-head" class="table-head" style="grid-template-columns:110px 160px 1fr 140px 70px;">
            <span>Scope</span><span>Key</span><span>Value</span><span>Updated</span><span></span>
          </div>
          <div id="overrides-rows"></div>
        </div>

        <div class="settings-grid">
          <div class="card" style="padding:16px;">
            <div class="card-title">Server</div>
            <label class="form-row"><span class="form-label">Host</span><input type="text" id="cfg-server-host" placeholder="0.0.0.0"></label>
            <label class="form-row"><span class="form-label">Port</span><input type="number" id="cfg-server-port" placeholder="40114"></label>
            <label class="form-row"><span class="form-label">API Key</span><input type="password" id="cfg-server-apikey" placeholder="Leave blank to keep current"></label>
            <label class="form-row"><span class="form-label">Enrollment Key (for node registration)</span><input type="password" id="cfg-server-enrollmentkey" placeholder="Leave blank to keep current"></label>
            <label class="form-row"><span class="form-label">Rate Limit (req/s, 0 = unlimited)</span><input type="number" id="cfg-server-ratelimit" min="0" placeholder="0"></label>
          </div>

          <div class="card" style="padding:16px;">
            <div class="card-title">Routing</div>
            <label class="form-row"><span class="form-label">Strategy</span>
              <select id="cfg-routing-strategy">
                <option value="model_aware">Model Aware</option>
                <option value="priority">Priority</option>
                <option value="least_busy">Least Busy</option>
                <option value="weighted_round_robin">Weighted Round Robin</option>
                <option value="scored">Scored</option>
              </select>
            </label>
            <label class="form-row"><span class="form-label">Timeout</span><input type="text" id="cfg-routing-timeout" placeholder="120s"></label>
            <label class="form-row"><span class="form-label">Retry Count</span><input type="number" id="cfg-routing-retrycount" min="0"></label>
            <label class="form-row"><span class="form-label">Default Keep Alive</span><input type="text" id="cfg-routing-keepalive" placeholder="5m"></label>
          </div>

          <div class="card" style="padding:16px;">
            <div class="card-title">Circuit Breaker</div>
            <label class="form-row"><span class="form-label">Failure Threshold</span><input type="number" id="cfg-cb-threshold" min="1"></label>
            <label class="form-row"><span class="form-label">Timeout</span><input type="text" id="cfg-cb-timeout" placeholder="120s"></label>
            <label class="form-row"><span class="form-label">Recovery Time</span><input type="text" id="cfg-cb-recovery" placeholder="30s"></label>
          </div>

          <div class="card" style="padding:16px;">
            <div class="card-title">Observability</div>
            <label class="form-check"><input type="checkbox" id="cfg-obs-metrics"> Metrics endpoint</label>
            <label class="form-check"><input type="checkbox" id="cfg-obs-adminapi"> Admin API</label>
            <label class="form-check"><input type="checkbox" id="cfg-obs-tracing"> Tracing</label>
            <label class="form-row"><span class="form-label">Log Retention (days)</span><input type="number" id="cfg-obs-retention" min="1"></label>
            <label class="form-row"><span class="form-label">Max Log Size (MB)</span><input type="number" id="cfg-obs-maxsize" min="0"></label>
            <label class="form-row"><span class="form-label">Max Log Files</span><input type="number" id="cfg-obs-maxfiles" min="1"></label>
          </div>

          <div class="card" style="padding:16px;">
            <div class="card-title">Agent</div>
            <label class="form-check"><input type="checkbox" id="cfg-agent-enabled"> Enabled</label>
            <label class="form-row"><span class="form-label">Max Sessions</span><input type="number" id="cfg-agent-maxsessions" min="1"></label>
            <label class="form-row"><span class="form-label">Max Tool Rounds</span><input type="number" id="cfg-agent-maxrounds" min="1"></label>
            <label class="form-row"><span class="form-label">Session TTL (minutes)</span><input type="number" id="cfg-agent-ttl" min="1"></label>
            <label class="form-row"><span class="form-label">Default Model</span><input type="text" id="cfg-agent-model" placeholder="e.g. llama3:8b"></label>
          </div>

          <div class="card" style="padding:16px;">
            <div class="card-title">Model Warmer</div>
            <label class="form-row"><span class="form-label">Interval (seconds, min 10)</span><input type="number" id="cfg-warmer-interval" min="10"></label>
          </div>
        </div>

        <div class="card" style="padding:16px;margin-top:16px;">
          <div class="view-header" style="margin-bottom:8px;">
            <span class="card-title" style="margin-bottom:0;">Backends</span>
            <div class="view-actions"><button class="btn btn-primary btn-sm" id="backends-add">+ Add Backend</button></div>
          </div>
          <div class="table-head" style="grid-template-columns:1fr 1.6fr 70px 1fr 1fr 60px;">
            <span>Name</span><span>URL</span><span>Priority</span><span>Hot Models</span><span>Tags</span><span></span>
          </div>
          <div id="backends-rows"></div>
        </div>

        <div style="display:flex;gap:10px;justify-content:flex-end;margin-top:16px;">
          <button class="btn btn-secondary" id="cfg-reset">Reset</button>
          <button class="btn btn-primary" id="cfg-save">Save &amp; Reload</button>
        </div>

        <div class="card" style="padding:16px;margin-top:20px;">
          <div class="view-header" style="margin-bottom:0;cursor:pointer;" id="guide-toggle">
            <span class="card-title" style="margin-bottom:0;">Agent Guide</span>
            <span class="view-summary">API reference for building against this gateway</span>
            <span style="margin-left:auto;color:var(--text-3);" id="guide-caret">&#9656;</span>
          </div>
          <div id="guide-body" style="display:none;margin-top:14px;"></div>
        </div>

      </div>`;
    wireStaticControls();
    built = true;
  }

  function renderBanner(message, kind = 'warn') {
    const bel = el('settings-banner');
    if (!bel) return;
    bel.innerHTML = message ? `<div class="banner banner-${kind}">${escapeHtml(message)}</div>` : '';
  }

  // ---- config editor ----------------------------------------------------

  async function loadConfig() {
    try {
      fullConfig = await api('/admin/config');
    } catch (e) {
      if (e.status === 401) {
        el('settings-locked').style.display = '';
        el('settings-body').style.display = 'none';
      } else {
        renderBanner(`Gateway unreachable (${e.message || 'network error'}).`, 'error');
      }
      return;
    }
    el('settings-locked').style.display = 'none';
    el('settings-body').style.display = '';
    renderBanner(
      fullConfig.server && fullConfig.server.api_key === '********'
        ? null
        : 'No API key is set — admin endpoints are unprotected. Set one below.',
    );
    populateForm();
    loadModelRouting();
    loadOverrides();
    loadGuide();
  }

  function populateForm() {
    const s = fullConfig.server, r = fullConfig.routing, cb = fullConfig.circuit_breaker;
    const ob = fullConfig.observability, ag = fullConfig.agent, mw = fullConfig.model_warmer;

    setVal('cfg-server-host', s.host || '');
    setVal('cfg-server-port', s.port ?? 40114);
    setVal('cfg-server-apikey', '');
    el('cfg-server-apikey').placeholder = s.api_key === '********' ? 'Set (leave blank to keep)' : 'Not set';
    setVal('cfg-server-enrollmentkey', '');
    el('cfg-server-enrollmentkey').placeholder = s.enrollment_key === '********' ? 'Set (leave blank to keep)' : 'Auto-generated';
    setVal('cfg-server-ratelimit', s.rate_limit ?? 0);

    setVal('cfg-routing-strategy', r.strategy || 'model_aware');
    setVal('cfg-routing-timeout', r.timeout || '120s');
    setVal('cfg-routing-retrycount', r.retry_count ?? 2);
    setVal('cfg-routing-keepalive', r.default_keep_alive || '5m');

    setVal('cfg-cb-threshold', cb.failure_threshold ?? 5);
    setVal('cfg-cb-timeout', cb.timeout || '120s');
    setVal('cfg-cb-recovery', cb.recovery_time || '30s');

    setChecked('cfg-obs-metrics', ob.metrics ?? true);
    setChecked('cfg-obs-adminapi', ob.admin_api ?? false);
    setChecked('cfg-obs-tracing', ob.tracing ?? false);
    setVal('cfg-obs-retention', ob.log_retention_days ?? 7);
    setVal('cfg-obs-maxsize', ob.log_max_size_mb ?? 100);
    setVal('cfg-obs-maxfiles', ob.log_max_files ?? 5);

    setChecked('cfg-agent-enabled', ag.enabled ?? false);
    setVal('cfg-agent-maxsessions', ag.max_sessions ?? 100);
    setVal('cfg-agent-maxrounds', ag.max_tool_rounds ?? 10);
    setVal('cfg-agent-ttl', ag.session_ttl_minutes ?? 60);
    setVal('cfg-agent-model', ag.default_model || '');

    setVal('cfg-warmer-interval', mw.interval_secs ?? 240);

    renderBackendsTable();
  }

  function renderBackendsTable() {
    const rowsEl = el('backends-rows');
    if (!fullConfig.backends.length) {
      rowsEl.innerHTML = `<div class="table-row" style="grid-template-columns:1fr;"><span class="empty-state-sub">No backends configured.</span></div>`;
      return;
    }
    rowsEl.innerHTML = fullConfig.backends.map((b, i) => `
      <div class="table-row" style="grid-template-columns:1fr 1.6fr 70px 1fr 1fr 60px;cursor:default;" data-i="${i}">
        <span><input type="text" value="${attrEscape(b.name)}" data-field="name"></span>
        <span><input type="text" value="${attrEscape(b.url)}" data-field="url"></span>
        <span><input type="number" value="${b.priority}" min="0" data-field="priority"></span>
        <span><input type="text" value="${attrEscape((b.hot_models || []).join(', '))}" data-field="hot_models"></span>
        <span><input type="text" value="${attrEscape((b.tags || []).join(', '))}" data-field="tags"></span>
        <span><button class="btn btn-danger btn-xs" data-remove>Remove</button></span>
      </div>`).join('');
    rowsEl.querySelectorAll('.table-row').forEach((row) => {
      const i = parseInt(row.dataset.i, 10);
      row.querySelectorAll('input[data-field]').forEach((inp) => {
        inp.addEventListener('change', () => {
          const field = inp.dataset.field;
          if (field === 'priority') fullConfig.backends[i].priority = parseInt(inp.value, 10) || 0;
          else if (field === 'hot_models' || field === 'tags') {
            fullConfig.backends[i][field] = inp.value.split(',').map((s) => s.trim()).filter(Boolean);
          } else {
            fullConfig.backends[i][field] = inp.value;
          }
        });
      });
      row.querySelector('[data-remove]').addEventListener('click', () => {
        fullConfig.backends.splice(i, 1);
        renderBackendsTable();
      });
    });
  }

  function applyFormToConfig() {
    const s = fullConfig.server, r = fullConfig.routing, cb = fullConfig.circuit_breaker;
    const ob = fullConfig.observability, ag = fullConfig.agent, mw = fullConfig.model_warmer;

    s.host = val('cfg-server-host');
    s.port = parseInt(val('cfg-server-port'), 10) || 40114;
    const apiKeyInput = val('cfg-server-apikey');
    if (apiKeyInput) s.api_key = apiKeyInput; // blank = leave existing value/sentinel untouched
    const enrollInput = val('cfg-server-enrollmentkey');
    if (enrollInput) s.enrollment_key = enrollInput;
    s.rate_limit = parseInt(val('cfg-server-ratelimit'), 10) || 0;

    r.strategy = val('cfg-routing-strategy');
    r.timeout = val('cfg-routing-timeout');
    r.retry_count = parseInt(val('cfg-routing-retrycount'), 10) || 0;
    r.default_keep_alive = val('cfg-routing-keepalive');

    cb.failure_threshold = parseInt(val('cfg-cb-threshold'), 10) || 5;
    cb.timeout = val('cfg-cb-timeout');
    cb.recovery_time = val('cfg-cb-recovery');

    ob.metrics = el('cfg-obs-metrics').checked;
    ob.admin_api = el('cfg-obs-adminapi').checked;
    ob.tracing = el('cfg-obs-tracing').checked;
    ob.log_retention_days = parseInt(val('cfg-obs-retention'), 10) || 7;
    ob.log_max_size_mb = parseInt(val('cfg-obs-maxsize'), 10) || 100;
    ob.log_max_files = parseInt(val('cfg-obs-maxfiles'), 10) || 5;

    ag.enabled = el('cfg-agent-enabled').checked;
    ag.max_sessions = parseInt(val('cfg-agent-maxsessions'), 10) || 100;
    ag.max_tool_rounds = parseInt(val('cfg-agent-maxrounds'), 10) || 10;
    ag.session_ttl_minutes = parseInt(val('cfg-agent-ttl'), 10) || 60;
    ag.default_model = val('cfg-agent-model') || null;

    mw.interval_secs = parseInt(val('cfg-warmer-interval'), 10) || 240;
  }

  async function saveConfig() {
    applyFormToConfig();
    try {
      const result = await api('/admin/config', { method: 'PUT', body: fullConfig });
      toast(result.message || 'Config saved and reloaded');
      const newKey = val('cfg-server-apikey');
      if (newKey) HerdApp.setApiKey(newKey);
      await loadConfig();
    } catch (e) {
      toast(e.message || 'Save failed', 'error');
    }
  }

  // ---- model routing (port of the old dashboard's #G3 UI) ---------------

  async function loadModelRouting() {
    const container = el('mr-cards');
    let data;
    try {
      data = await api('/admin/config/backends');
    } catch (e) {
      container.innerHTML = `<div class="empty-state-sub">${e.status === 401 ? 'API key required.' : 'Gateway unreachable.'}</div>`;
      return;
    }
    const backends = Array.isArray(data) ? data : (data.backends || []);
    if (!backends.length) {
      container.innerHTML = '<div class="empty-state-sub">No backends configured.</div>';
      return;
    }
    container.innerHTML = backends.map(renderMrCard).join('');
    wireMrCards();
  }

  function renderMrCard(b) {
    const available = b.models_available || [];
    const mrEnabled = b.models_enabled; // null/undefined => route all installed
    const allowAll = mrEnabled === null || mrEnabled === undefined;
    const names = Array.from(new Set([...available, ...(Array.isArray(mrEnabled) ? mrEnabled : [])])).sort();
    const rows = names.map((m) => {
      const installed = available.includes(m);
      const isChecked = allowAll ? true : mrEnabled.includes(m);
      const missing = isChecked && !installed;
      return `<label class="mr-row${missing ? ' missing' : ''}">
        <input type="checkbox" data-mrmodel value="${attrEscape(m)}" ${isChecked ? 'checked' : ''}>
        <span>${escapeHtml(m)}</span>
        ${missing ? '<span class="mr-miss-badge">missing</span>' : ''}
      </label>`;
    }).join('') || '<div class="empty-state-sub" style="padding:6px;">No models reported yet.</div>';

    const enabledSet = allowAll ? available.slice() : names.filter((m) => mrEnabled.includes(m));
    const hot = b.hot_models || [];
    const hotOpts = enabledSet.map((m) => `<option value="${attrEscape(m)}" ${hot.includes(m) ? 'selected' : ''}>${escapeHtml(m)}</option>`).join('');

    const note = allowAll ? 'Routing ALL installed models (no allowlist set).' : `${mrEnabled.length} of ${names.length} allowed.`;
    const dotCls = b.healthy ? 'health-healthy' : 'health-offline';
    const dotGlyph = b.healthy ? '●' : '○';

    return `<div class="tile mr-card" data-backend="${attrEscape(b.name)}">
      <div class="mr-head">
        <span class="${dotCls}">${dotGlyph}</span>
        <span style="font-weight:600;">${escapeHtml(b.name)}</span>
        <span class="pill">${escapeHtml(b.backend || '')}</span>
        <span class="mono" style="font-size:11px;color:var(--text-3);margin-left:auto;">${escapeHtml(b.url || '')}</span>
      </div>
      <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap;">
        <input type="text" class="mr-filter" placeholder="Filter ${names.length} models…" style="flex:1;min-width:160px;">
        <button class="btn btn-secondary btn-xs mr-all">All</button>
        <button class="btn btn-secondary btn-xs mr-none">None</button>
      </div>
      <div class="mr-list">${rows}</div>
      <div class="mr-actions">
        <span class="mr-note">${note}</span>
        <button class="btn btn-secondary btn-sm mr-clear">Route all</button>
        <button class="btn btn-primary btn-sm mr-save">Save allowlist</button>
      </div>
      <div class="mr-hot">
        <span style="font-size:11px;color:var(--text-3);">Hot models (kept warm)</span>
        <select multiple size="3" class="mr-hot-select">${hotOpts}</select>
        <div class="mr-actions"><button class="btn btn-secondary btn-sm mr-save-hot">Save hot models</button></div>
      </div>
    </div>`;
  }

  function wireMrCards() {
    el('mr-cards').querySelectorAll('.mr-card').forEach((card) => {
      const name = card.dataset.backend;
      card.querySelector('.mr-filter').addEventListener('input', (e) => {
        const q = e.target.value.toLowerCase();
        card.querySelectorAll('.mr-row').forEach((row) => {
          const m = row.querySelector('input').value.toLowerCase();
          row.style.display = m.includes(q) ? '' : 'none';
        });
      });
      card.querySelector('.mr-all').addEventListener('click', () => setVisibleChecked(card, true));
      card.querySelector('.mr-none').addEventListener('click', () => setVisibleChecked(card, false));
      card.querySelector('.mr-clear').addEventListener('click', () => mrClearAllowlist(name));
      card.querySelector('.mr-save').addEventListener('click', () => mrSaveAllowlist(name, card));
      card.querySelector('.mr-save-hot').addEventListener('click', () => mrSaveHot(name, card));
    });
  }

  function setVisibleChecked(card, checked) {
    card.querySelectorAll('.mr-row').forEach((row) => {
      if (row.style.display !== 'none') row.querySelector('input').checked = checked;
    });
  }

  async function mrSaveAllowlist(name, card) {
    const models = Array.from(card.querySelectorAll('input[data-mrmodel]:checked')).map((i) => i.value);
    try {
      await api(`/admin/config/backends/${encodeURIComponent(name)}/models`, { method: 'PUT', body: { models_enabled: models } });
      toast(`Saved allowlist for ${name} (${models.length} models)`);
      loadModelRouting();
      loadOverrides();
    } catch (e) {
      toast(e.status === 404 ? `Backend ${name} not found` : (e.message || 'Save failed'), 'error');
    }
  }

  async function mrClearAllowlist(name) {
    try {
      await api(`/admin/config/backends/${encodeURIComponent(name)}/models`, { method: 'PUT', body: { models_enabled: null } });
      toast(`${name}: routing all models (allowlist cleared)`);
      loadModelRouting();
      loadOverrides();
    } catch (e) {
      toast(e.message || 'Clear failed', 'error');
    }
  }

  async function mrSaveHot(name, card) {
    const sel = card.querySelector('.mr-hot-select');
    const hot = Array.from(sel.selectedOptions).map((o) => o.value);
    try {
      await api(`/admin/config/backends/${encodeURIComponent(name)}`, { method: 'PUT', body: { hot_models: hot } });
      toast(`Saved hot models for ${name} (${hot.length})`);
      loadOverrides();
    } catch (e) {
      toast(e.message || 'Save failed', 'error');
    }
  }

  // ---- config overrides ---------------------------------------------------

  async function loadOverrides() {
    const rowsEl = el('overrides-rows');
    let data;
    try {
      data = await api('/admin/config/overrides');
    } catch (e) {
      rowsEl.innerHTML = `<div class="table-row" style="grid-template-columns:1fr;"><span class="empty-state-sub">${e.status === 401 ? 'API key required.' : 'Gateway unreachable.'}</span></div>`;
      return;
    }
    const rows = data.overrides || [];
    if (!rows.length) {
      rowsEl.innerHTML = `<div class="table-row" style="grid-template-columns:1fr;"><span class="empty-state-sub">No overrides — running pure herd.yaml.</span></div>`;
      return;
    }
    rowsEl.innerHTML = rows.map((o) => {
      let v = o.value_json || '';
      if (v.length > 80) v = `${v.slice(0, 80)}…`;
      const when = (o.updated_at || '').replace('T', ' ').slice(0, 19);
      return `<div class="table-row" style="grid-template-columns:110px 160px 1fr 140px 70px;cursor:default;" data-scope="${attrEscape(o.scope)}" data-key="${attrEscape(o.key)}">
        <span style="font-size:12px;">${escapeHtml(o.scope)}</span>
        <span class="mono" style="font-size:11px;">${escapeHtml(o.key)}</span>
        <span class="mono" style="font-size:11px;color:var(--text-2);">${escapeHtml(v)}</span>
        <span class="mono" style="font-size:10px;color:var(--text-3);">${escapeHtml(when)}</span>
        <span><button class="btn btn-danger btn-xs override-delete">Delete</button></span>
      </div>`;
    }).join('');
    rowsEl.querySelectorAll('.override-delete').forEach((btn) => {
      btn.addEventListener('click', () => {
        const row = btn.closest('.table-row');
        deleteOverride(row.dataset.scope, row.dataset.key);
      });
    });
  }

  async function deleteOverride(scope, key) {
    try {
      await api(`/admin/config/overrides/${encodeURIComponent(scope)}/${encodeURIComponent(key)}`, { method: 'DELETE' });
      toast(`Removed override ${scope}/${key}`);
      loadModelRouting();
      loadOverrides();
    } catch (e) {
      toast(e.status === 404 ? 'Override not found' : (e.message || 'Delete failed'), 'error');
    }
  }

  // ---- agent guide (static reference content, collapsed by default) -----

  let guideBuilt = false;

  function loadGuide() {
    if (guideBuilt) return;
    el('guide-body').innerHTML = `
      <div class="guide-section">
        <h3>Quick Start</h3>
        <div class="guide-card">
          <h4>1. Discover models</h4>
          <pre><code>GET /v1/models</code></pre>
          <p>Returns all models available across healthy backends. Check before requesting an unknown model.</p>
        </div>
        <div class="guide-card">
          <h4>2. Send chat requests</h4>
          <pre><code>POST /v1/chat/completions
Content-Type: application/json

{
  "model": "qwen2.5-coder:32b",
  "messages": [{"role": "user", "content": "Hello"}],
  "stream": true
}</code></pre>
          <p>OpenAI-compatible — existing client libraries work unchanged. Always specify the model for optimal routing.</p>
        </div>
        <div class="guide-card">
          <h4>3. Target specific backends</h4>
          <pre><code>X-Herd-Tags: gpu,fast</code></pre>
          <p>Route only to backends matching all specified tags. Use for workload isolation.</p>
        </div>
        <div class="guide-card">
          <h4>4. Trace requests</h4>
          <pre><code>X-Request-Id: agent-task-42</code></pre>
          <p>Send a correlation ID to trace requests in logs. If omitted, Herd generates a UUID v4 and returns it.</p>
        </div>
      </div>

      <div class="guide-section">
        <h3>Endpoints</h3>
        <div class="table-head" style="grid-template-columns:1.6fr 70px 2fr 60px;"><span>Action</span><span>Method</span><span>Endpoint</span><span>Auth</span></div>
        ${[
          ['Chat (OpenAI)', 'POST', '/v1/chat/completions', 'No'],
          ['List models', 'GET', '/v1/models', 'No'],
          ['Health check', 'GET', '/health', 'No'],
          ['Cluster status', 'GET', '/status', 'No'],
          ['Analytics', 'GET', '/analytics?hours=24', 'No'],
          ['Prometheus', 'GET', '/metrics', 'No'],
          ['Effective backend configs (+ models)', 'GET', '/admin/config/backends', 'Yes'],
          ['Set models allowlist (null clears)', 'PUT', '/admin/config/backends/{name}/models', 'Yes'],
          ['List config overrides', 'GET', '/admin/config/overrides', 'Yes'],
          ['Delete one override', 'DELETE', '/admin/config/overrides/{scope}/{key}', 'Yes'],
          ['Agent sessions', 'GET/POST', '/agent/sessions', 'Yes'],
        ].map(([action, method, endpoint, auth]) => `<div class="table-row" style="grid-template-columns:1.6fr 70px 2fr 60px;cursor:default;">
          <span style="font-size:12px;">${escapeHtml(action)}</span>
          <span class="mono" style="font-size:11px;color:var(--text-2);">${escapeHtml(method)}</span>
          <span class="mono" style="font-size:11px;">${escapeHtml(endpoint)}</span>
          <span style="font-size:11px;color:${auth === 'Yes' ? 'var(--text-2)' : 'var(--text-3)'};">${escapeHtml(auth)}</span>
        </div>`).join('')}
      </div>

      <div class="guide-section">
        <h3>Routing Strategies</h3>
        <div class="table-head" style="grid-template-columns:140px 1fr;"><span>Strategy</span><span>Behavior</span></div>
        ${[
          ['model_aware', 'Prefers backends with your model already loaded. Avoids cold starts. (default)'],
          ['priority', 'Always routes to the highest-priority healthy backend.'],
          ['least_busy', 'Routes to the lowest GPU utilization.'],
          ['weighted_round_robin', 'Distributes across backends weighted by priority.'],
        ].map(([strat, behavior]) => `<div class="table-row" style="grid-template-columns:140px 1fr;cursor:default;">
          <span class="mono" style="font-size:11px;">${escapeHtml(strat)}</span>
          <span style="font-size:12px;color:var(--text-2);">${escapeHtml(behavior)}</span>
        </div>`).join('')}
      </div>

      <div class="guide-section">
        <h3>Full reference</h3>
        <div class="empty-state-sub" style="text-align:left;">
          See <a href="/skills.md" target="_blank">/skills.md</a> (full agent skills reference) or
          <a href="/skills" target="_blank">/skills</a> (structured JSON version), served by this gateway.
        </div>
      </div>`;
    guideBuilt = true;
  }

  // ---- static control wiring (runs once, at buildDom time) --------------

  function wireStaticControls() {
    el('settings-refresh').addEventListener('click', loadConfig);
    el('mr-refresh').addEventListener('click', loadModelRouting);
    el('overrides-refresh').addEventListener('click', loadOverrides);
    el('backends-add').addEventListener('click', () => {
      fullConfig.backends.push({ name: '', url: '', priority: 50, hot_models: [], tags: [] });
      renderBackendsTable();
      const rows = el('backends-rows').querySelectorAll('.table-row');
      const last = rows[rows.length - 1];
      if (last) last.querySelector('input')?.focus();
    });
    el('cfg-reset').addEventListener('click', loadConfig);
    el('cfg-save').addEventListener('click', saveConfig);
    el('guide-toggle').addEventListener('click', () => {
      const body = el('guide-body');
      const open = body.style.display !== 'none';
      body.style.display = open ? 'none' : '';
      el('guide-caret').innerHTML = open ? '&#9656;' : '&#9662;';
    });
  }

  // ---- lifecycle ------------------------------------------------------------

  function mount() {
    if (!built) buildDom();
  }

  function start() {
    loadConfig();
  }

  function stop() {}

  HerdApp.registerView('settings', { mount, start, stop });
})();
