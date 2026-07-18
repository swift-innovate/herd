// ==========================================================================
// Analytics view — fleet-wide request volume, latency, token/cost metrics.
// Backed entirely by GET /analytics?hours=N (AnalyticsStats, src/analytics.rs).
// Note: the real field names are latency_p50/p95/p99 (ms) — not p50_ms as the
// old dashboard.html reads (a stale/broken reference there, not replicated
// here). There is no error_count field in AnalyticsStats, so no success-rate
// stat is shown (the old dashboard's version of that was reading a field
// that doesn't exist either).
// ==========================================================================

(() => {
  const { api, escapeHtml, fmtRelativeTime } = HerdApp;

  let pollTimer = null;
  let built = false;
  let hours = 24;
  const charts = {}; // name -> Chart instance

  // Data-viz palette — deliberately distinct from --health-* (green/amber/red/
  // purple are health-only) and used only for non-health chart series. The
  // timeline chart is chrome amber (activity, not a status column).
  let COLOR = {};

  function resolveColors() {
    const cs = getComputedStyle(document.documentElement);
    const v = (name) => cs.getPropertyValue(name).trim();
    COLOR = {
      amber: v('--amber'),
      neutral: v('--neutral-fill'),
      text2: v('--text-2'),
      grid: v('--border-2'),
      text3: v('--text-3'),
      palette: [v('--amber'), v('--neutral-fill'), v('--text-2'), '#e3c179', '#8a7a63', '#c8935f'],
    };
  }

  function fmtMs(ms) {
    if (ms === null || ms === undefined) return '—';
    return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(2)}s`;
  }

  // ---- one-time DOM + Chart.js setup --------------------------------------

  function buildDom() {
    const root = document.getElementById('analytics-root');
    root.innerHTML = `
      <div class="view-header">
        <span class="view-title">Analytics</span>
        <span class="view-summary" id="analytics-summary">loading…</span>
        <div class="view-actions">
          <select id="analytics-hours">
            <option value="1">1h</option>
            <option value="6">6h</option>
            <option value="24" selected>24h</option>
            <option value="72">3d</option>
            <option value="168">7d</option>
          </select>
        </div>
      </div>
      <div id="analytics-banner"></div>

      <div class="chart-grid">
        <div class="card chart-card">
          <div class="chart-card-title">Request volume</div>
          <div class="chart-container"><canvas id="chart-timeline"></canvas></div>
        </div>
        <div class="card chart-card">
          <div class="chart-card-title">By model</div>
          <div class="chart-container"><canvas id="chart-models"></canvas></div>
        </div>
        <div class="card chart-card">
          <div class="chart-card-title">By backend</div>
          <div class="chart-container"><canvas id="chart-backends"></canvas></div>
        </div>
      </div>

      <div class="tile-row cols-3">
        <div class="tile"><div class="tile-label">P50 latency</div><div class="tile-value" id="lat-p50">—</div></div>
        <div class="tile"><div class="tile-label">P95 latency</div><div class="tile-value" id="lat-p95">—</div></div>
        <div class="tile"><div class="tile-label">P99 latency</div><div class="tile-value" id="lat-p99">—</div></div>
      </div>

      <div class="tile-row cols-2">
        <div class="tile">
          <div class="tile-label">Estimated API cost avoided</div>
          <div class="tile-value" id="cost-avoided">—</div>
          <div class="tile-sub">vs. equivalent cloud API pricing</div>
        </div>
        <div class="tile">
          <div class="tile-label">Avg tokens/second</div>
          <div class="tile-value" id="tok-per-sec">—</div>
          <div class="tile-sub" id="token-total-sub"></div>
        </div>
      </div>

      <div class="card chart-card" style="margin-top:14px;">
        <div class="chart-card-title">Token usage by model</div>
        <div class="chart-container tall"><canvas id="chart-tokens"></canvas></div>
      </div>

      <div class="card" style="margin-top:14px;">
        <div class="table-head" style="grid-template-columns: 1fr 80px 80px 80px;">
          <span>Model</span><span>P50</span><span>P95</span><span>P99</span>
        </div>
        <div id="model-latency-rows"><div style="padding:20px;color:var(--text-3);font-size:12px;">No data yet</div></div>
      </div>

      <div class="card" style="margin-top:14px;">
        <div class="table-head" style="grid-template-columns: 1fr 80px 80px 80px;">
          <span>Backend</span><span>P50</span><span>P95</span><span>P99</span>
        </div>
        <div id="backend-latency-rows"><div style="padding:20px;color:var(--text-3);font-size:12px;">No data yet</div></div>
      </div>`;

    document.getElementById('analytics-hours').addEventListener('change', (e) => {
      hours = Number(e.target.value) || 24;
      refresh();
    });

    resolveColors();
    buildCharts();
    built = true;
  }

  function buildCharts() {
    const commonScales = {
      x: { grid: { color: COLOR.grid }, ticks: { color: COLOR.text3, font: { size: 10 } } },
      y: { grid: { color: COLOR.grid }, ticks: { color: COLOR.text3, font: { size: 10 } }, beginAtZero: true },
    };

    charts.timeline = new Chart(document.getElementById('chart-timeline'), {
      type: 'line',
      data: { labels: [], datasets: [{ data: [], borderColor: COLOR.amber, backgroundColor: 'transparent', tension: 0.3, pointRadius: 0 }] },
      options: { responsive: true, maintainAspectRatio: false, plugins: { legend: { display: false } }, scales: commonScales },
    });

    charts.models = new Chart(document.getElementById('chart-models'), {
      type: 'bar',
      data: { labels: [], datasets: [{ data: [], backgroundColor: COLOR.palette }] },
      options: { responsive: true, maintainAspectRatio: false, plugins: { legend: { display: false } }, scales: commonScales },
    });

    charts.backends = new Chart(document.getElementById('chart-backends'), {
      type: 'bar',
      data: { labels: [], datasets: [{ data: [], backgroundColor: COLOR.palette }] },
      options: { responsive: true, maintainAspectRatio: false, plugins: { legend: { display: false } }, scales: commonScales },
    });

    charts.tokens = new Chart(document.getElementById('chart-tokens'), {
      type: 'bar',
      data: {
        labels: [],
        datasets: [
          { label: 'in', data: [], backgroundColor: COLOR.neutral },
          { label: 'out', data: [], backgroundColor: COLOR.amber },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        plugins: { legend: { labels: { color: COLOR.text2, font: { size: 10 } } } },
        scales: { x: { ...commonScales.x, stacked: true }, y: { ...commonScales.y, stacked: true } },
      },
    });
  }

  // ---- render ---------------------------------------------------------------

  function renderBanner(message, kind = 'warn') {
    const el = document.getElementById('analytics-banner');
    if (!el) return;
    el.innerHTML = message ? `<div class="banner banner-${kind}">${escapeHtml(message)}</div>` : '';
  }

  function latencyRowsHtml(map) {
    const entries = Object.entries(map || {});
    if (!entries.length) return '<div style="padding:20px;color:var(--text-3);font-size:12px;">No data yet</div>';
    return entries
      .sort((a, b) => b[1].p50 - a[1].p50)
      .map(([name, lat]) => `
        <div class="table-row" style="grid-template-columns: 1fr 80px 80px 80px;">
          <span class="mono" style="font-size:12px;">${escapeHtml(name)}</span>
          <span class="mono" style="font-size:11px;color:var(--text-2);">${fmtMs(lat.p50)}</span>
          <span class="mono" style="font-size:11px;color:var(--text-2);">${fmtMs(lat.p95)}</span>
          <span class="mono" style="font-size:11px;color:var(--text-2);">${fmtMs(lat.p99)}</span>
        </div>`)
      .join('');
  }

  function render(data) {
    document.getElementById('analytics-summary').textContent =
      `${(data.total_requests || 0).toLocaleString()} requests · last ${hours}h`;

    document.getElementById('lat-p50').textContent = fmtMs(data.latency_p50);
    document.getElementById('lat-p95').textContent = fmtMs(data.latency_p95);
    document.getElementById('lat-p99').textContent = fmtMs(data.latency_p99);

    document.getElementById('cost-avoided').textContent = `$${(data.estimated_api_cost_usd || 0).toFixed(2)}`;
    document.getElementById('tok-per-sec').textContent = `${(data.tokens_per_second_avg || 0).toFixed(1)} t/s`;
    const totalIn = data.total_tokens_in || 0;
    const totalOut = data.total_tokens_out || 0;
    document.getElementById('token-total-sub').textContent =
      (totalIn || totalOut) ? `${totalIn.toLocaleString()} in / ${totalOut.toLocaleString()} out` : '';

    // Timeline
    const timeline = data.timeline || [];
    charts.timeline.data.labels = timeline.map(([ts]) => fmtRelativeTime(ts));
    charts.timeline.data.datasets[0].data = timeline.map(([, count]) => count);
    charts.timeline.update('none');

    // By model (top 6)
    const models = Object.entries(data.model_counts || {}).sort((a, b) => b[1] - a[1]).slice(0, 6);
    charts.models.data.labels = models.map(([m]) => m.split(':')[0]);
    charts.models.data.datasets[0].data = models.map(([, c]) => c);
    charts.models.update('none');

    // By backend
    const backends = Object.entries(data.backend_counts || {}).sort((a, b) => b[1] - a[1]);
    charts.backends.data.labels = backends.map(([b]) => b);
    charts.backends.data.datasets[0].data = backends.map(([, c]) => c);
    charts.backends.update('none');

    // Token usage by model (stacked in/out) — model_token_counts is a Rust
    // tuple (in, out), serialized as a 2-element JSON array.
    const tokenModels = Object.entries(data.model_token_counts || {}).slice(0, 8);
    charts.tokens.data.labels = tokenModels.map(([m]) => m.split(':')[0]);
    charts.tokens.data.datasets[0].data = tokenModels.map(([, v]) => v[0] || 0);
    charts.tokens.data.datasets[1].data = tokenModels.map(([, v]) => v[1] || 0);
    charts.tokens.update('none');

    document.getElementById('model-latency-rows').innerHTML = latencyRowsHtml(data.model_latency);
    document.getElementById('backend-latency-rows').innerHTML = latencyRowsHtml(data.backend_latency);
  }

  async function refresh() {
    try {
      const data = await api(`/analytics?hours=${hours}`);
      if (data && data.error) throw { status: 0, message: data.error };
      renderBanner(null);
      render(data);
    } catch (e) {
      if (e.status === 401) {
        renderBanner('Admin API key required for analytics data.', 'warn');
      } else {
        renderBanner(`Gateway unreachable — retrying every 30s (${e.message || 'network error'}).`, 'error');
      }
    }
  }

  function mount() {
    if (!built) buildDom();
  }

  function start() {
    refresh();
    pollTimer = setInterval(refresh, 30000);
  }

  function stop() {
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
  }

  HerdApp.registerView('analytics', { mount, start, stop, pollSeconds: 30 });
})();
