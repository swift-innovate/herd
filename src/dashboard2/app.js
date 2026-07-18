// ==========================================================================
// Herd dashboard v2 — core shell: API key, fetch wrapper, view router,
// polling scheduler, toasts, modals, formatting helpers.
// Screen modules (fleet.js, node_detail.js, ...) register themselves via
// HerdApp.registerView(name, {mount, start, stop}) and are driven from here.
// ==========================================================================

const HerdApp = (() => {
  const API_KEY_STORAGE = 'herd-api-key';
  const LAST_VIEW_STORAGE = 'herd-dashboard2-last-view';

  const views = {}; // name -> { mount, start, stop }
  let activeView = null;
  let apiKeyPresent = false;

  // ---- API key ----

  function getApiKey() {
    return localStorage.getItem(API_KEY_STORAGE) || '';
  }

  function setApiKey(key) {
    if (key) {
      localStorage.setItem(API_KEY_STORAGE, key);
    } else {
      localStorage.removeItem(API_KEY_STORAGE);
    }
    updateApiKeyBadge();
  }

  function updateApiKeyBadge() {
    const key = getApiKey();
    apiKeyPresent = !!key;
    const el = document.getElementById('api-key-mask');
    if (el) el.textContent = key ? '●●●● set' : 'not set';
  }

  function promptForApiKey() {
    const current = getApiKey();
    const next = window.prompt('Admin API key (leave blank for read-only):', current);
    if (next !== null) setApiKey(next.trim());
  }

  // ---- fetch wrapper ----

  /** Wraps fetch with the X-API-Key header and uniform error/401 handling.
   *  Returns parsed JSON on success; throws {status, message} on failure. */
  async function api(path, options = {}) {
    const opts = Object.assign({}, options);
    opts.headers = Object.assign({}, options.headers);
    const key = getApiKey();
    if (key) opts.headers['X-API-Key'] = key;
    if (opts.body && typeof opts.body !== 'string') {
      opts.body = JSON.stringify(opts.body);
      opts.headers['Content-Type'] = 'application/json';
    }

    let res;
    try {
      res = await fetch(path, opts);
    } catch (e) {
      markDisconnected();
      throw { status: 0, message: 'gateway unreachable' };
    }

    markConnected();

    if (res.status === 401) {
      throw { status: 401, message: 'API key required or invalid' };
    }
    if (!res.ok) {
      let message = `request failed (${res.status})`;
      try {
        const body = await res.json();
        if (body && body.error) message = body.error;
      } catch (_) {}
      throw { status: res.status, message };
    }
    if (res.status === 204) return null;
    const ct = res.headers.get('content-type') || '';
    if (ct.includes('application/json')) return res.json();
    return res.text();
  }

  // ---- connection indicator ----

  let connected = null; // tri-state: null=unknown

  function markConnected() {
    if (connected === true) return;
    connected = true;
    const dot = document.getElementById('conn-dot');
    const label = document.getElementById('conn-label');
    if (dot) { dot.className = 'dot-ok'; }
    if (label) label.textContent = 'connected';
  }

  function markDisconnected() {
    if (connected === false) return;
    connected = false;
    const dot = document.getElementById('conn-dot');
    const label = document.getElementById('conn-label');
    if (dot) { dot.className = 'dot-bad'; }
    if (label) label.textContent = 'gateway unreachable';
  }

  // ---- refresh countdown (driven by the active view's poll interval) ----

  let countdownTimer = null;

  function startCountdown(seconds) {
    stopCountdown();
    let remaining = seconds;
    const el = document.getElementById('refresh-countdown');
    if (el) el.textContent = String(remaining);
    countdownTimer = setInterval(() => {
      remaining = remaining > 0 ? remaining - 1 : seconds;
      if (el) el.textContent = String(remaining);
    }, 1000);
  }

  function stopCountdown() {
    if (countdownTimer) { clearInterval(countdownTimer); countdownTimer = null; }
  }

  // ---- toasts ----

  function toast(message, kind = 'success', timeoutMs = 4000) {
    const stack = document.getElementById('toast-stack');
    if (!stack) return;
    const el = document.createElement('div');
    el.className = `toast ${kind}`;
    el.textContent = message;
    stack.appendChild(el);
    setTimeout(() => el.remove(), timeoutMs);
  }

  // ---- modals ----

  function openModal(innerHtml) {
    const root = document.getElementById('modal-root');
    if (!root) return;
    root.innerHTML = `<div class="modal-panel">${innerHtml}</div>`;
    root.classList.add('open');
    root.onclick = (e) => { if (e.target === root) closeModal(); };
  }

  function closeModal() {
    const root = document.getElementById('modal-root');
    if (!root) return;
    root.classList.remove('open');
    root.innerHTML = '';
  }

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeModal();
  });

  // ---- formatting helpers ----

  function fmtGb(mb) {
    if (mb === null || mb === undefined) return '—';
    return (mb / 1024).toFixed(1);
  }

  function fmtBytesToGb(bytes) {
    if (bytes === null || bytes === undefined) return '—';
    return (bytes / (1024 ** 3)).toFixed(1);
  }

  function fmtPct(n) {
    if (n === null || n === undefined) return '—';
    return `${Math.round(n)}%`;
  }

  function fmtRelativeTime(isoOrEpochSeconds) {
    if (!isoOrEpochSeconds) return 'never';
    const then = typeof isoOrEpochSeconds === 'number'
      ? isoOrEpochSeconds * 1000
      : new Date(isoOrEpochSeconds).getTime();
    const diffSec = Math.max(0, Math.round((Date.now() - then) / 1000));
    if (diffSec < 60) return `${diffSec}s ago`;
    if (diffSec < 3600) return `${Math.round(diffSec / 60)}m ago`;
    if (diffSec < 86400) return `${Math.round(diffSec / 3600)}h ago`;
    return `${Math.round(diffSec / 86400)}d ago`;
  }

  /** Join key for the Fleet/Node-detail merge: node.backend_url vs backend-pool url. */
  function normalizeUrl(url) {
    if (!url) return '';
    return url.trim().replace(/\/+$/, '').replace(/^https?:\/\//, '').toLowerCase();
  }

  function escapeHtml(s) {
    if (s === null || s === undefined) return '';
    return String(s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  /** Safe for HTML attribute values (double- or single-quoted): also escapes '.
   *  Use for untrusted values (backend names, override scope/key) placed into
   *  a data-* attribute — never string-interpolate them into inline JS. */
  function attrEscape(s) {
    return escapeHtml(s).replace(/'/g, '&#39;');
  }

  // ---- view router ----

  /** Screen modules call this once at script-load time. `start`/`stop` drive
   *  polling (start returns nothing, is called on nav-in; stop clears any
   *  intervals the view owns, is called on nav-out). `pollSeconds` feeds the
   *  header countdown while this view is active. */
  function registerView(name, { mount, start, stop, pollSeconds }) {
    views[name] = { mount, start, stop, pollSeconds: pollSeconds || null };
  }

  /** A view section can exist (with static stub content) before its JS
   *  module registers — e.g. Models/Tasks are Phase-2 stubs authored
   *  straight into shell_body.html. Only require the DOM section to exist;
   *  `views[name]` is optional and just skips the mount/start/stop calls. */
  function switchView(name) {
    const section = document.getElementById(`view-${name}`);
    if (!section) return;

    if (activeView && views[activeView] && views[activeView].stop) {
      views[activeView].stop();
    }
    document.querySelectorAll('.view').forEach((v) => v.classList.remove('active'));
    document.querySelectorAll('.nav-item').forEach((n) => n.classList.remove('active'));
    const navItem = document.querySelector(`.nav-item[data-view="${name}"]`);
    section.classList.add('active');
    if (navItem) navItem.classList.add('active');

    activeView = name;
    localStorage.setItem(LAST_VIEW_STORAGE, name);

    const view = views[name];
    if (view && view.mount) view.mount();
    if (view && view.start) view.start();
    if (view && view.pollSeconds) startCountdown(view.pollSeconds); else stopCountdown();
  }

  function init() {
    updateApiKeyBadge();

    document.querySelectorAll('.nav-item[data-view]').forEach((el) => {
      el.addEventListener('click', () => switchView(el.dataset.view));
    });

    const keyRow = document.getElementById('api-key-row');
    if (keyRow) keyRow.addEventListener('click', promptForApiKey);

    if (typeof applyHerdMark === 'function') applyHerdMark();

    // Only restore a last-view that's an actual sidebar destination — views
    // like 'node' have no nav item and no meaning without an open(id) call,
    // so they must never be the view a fresh page load lands on.
    const lastView = localStorage.getItem(LAST_VIEW_STORAGE);
    const lastViewIsNavTarget = lastView && document.querySelector(`.nav-item[data-view="${lastView}"]`);
    switchView(lastViewIsNavTarget ? lastView : 'fleet');

    // Gateway version/strategy badge — best-effort, non-blocking.
    api('/status').then((s) => {
      const gwStrategy = document.getElementById('gw-strategy');
      if (gwStrategy && s.routing_strategy) gwStrategy.textContent = s.routing_strategy.toLowerCase();
    }).catch(() => {});
  }

  document.addEventListener('DOMContentLoaded', init);

  return {
    api, toast, openModal, closeModal, registerView, switchView,
    getApiKey, setApiKey, apiKeyPresent: () => apiKeyPresent,
    fmtGb, fmtBytesToGb, fmtPct, fmtRelativeTime, escapeHtml, attrEscape, normalizeUrl,
  };
})();
