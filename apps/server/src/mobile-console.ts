/**
 * The web console — a mobile-first, Tailscale-web-style management page for
 * the harness's remote access. Served directly by the mobile listener at
 * `/console?token=…` so a phone browser needs nothing but a URL: no app, no
 * QR, no deep link. The long-lived token keeps the URL bookmarkable across
 * restarts.
 *
 * Deliberately dependency-free: no framework, no build step. It speaks the
 * same req/res WebSocket protocol as every other client, but only ever calls
 * the connection-management methods.
 *
 * The page is a compiled string because the server build is a plain `tsc -b`
 * with no asset-copy step — a static .html would never reach dist/. Keep it
 * self-contained; escape nothing, because the page JS never uses backticks or
 * `${}`.
 */
export const CONSOLE_PAGE = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="color-scheme" content="dark">
<title>Harness console</title>
<style>
  :root {
    color-scheme: dark;
    --bg: #0f0f0f;
    --surface: #1a1a1a;
    --surface-2: #222222;
    --line: #262626;
    --text: #ededed;
    --text-2: #a3a3a3;
    --text-3: #6f6f6f;
    --accent: #65b8ff;
    --danger: #fe8549;
    --ok: #3fb950;
  }
  * { box-sizing: border-box; }
  html, body { margin: 0; padding: 0; }
  body {
    background: var(--bg);
    color: var(--text);
    font: 15px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    -webkit-font-smoothing: antialiased;
    padding: env(safe-area-inset-top) 0 env(safe-area-inset-bottom);
  }
  header {
    position: sticky; top: 0; z-index: 1;
    display: flex; align-items: center; justify-content: space-between;
    gap: 12px; padding: 16px 20px;
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(12px);
    border-bottom: 1px solid var(--line);
  }
  header h1 { font-size: 17px; font-weight: 650; margin: 0; letter-spacing: -0.01em; }
  main { max-width: 640px; margin: 0 auto; padding: 16px 20px 48px; }
  .card {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 18px;
    margin-bottom: 14px;
  }
  .card h2 { font-size: 13px; font-weight: 600; color: var(--text-2); margin: 0 0 12px; text-transform: none; }
  .card h3 { font-size: 12px; font-weight: 600; color: var(--text-3); margin: 16px 0 8px; }
  .pill {
    display: inline-flex; align-items: center; gap: 7px;
    font-size: 12.5px; font-weight: 550; color: var(--text-2);
    border: 1px solid var(--line); border-radius: 999px; padding: 4px 11px;
    background: var(--surface-2);
  }
  .pill::before { content: ""; width: 8px; height: 8px; border-radius: 50%; background: var(--text-3); }
  .pill.on { color: var(--text); }
  .pill.on::before { background: var(--ok); box-shadow: 0 0 8px color-mix(in srgb, var(--ok) 60%, transparent); }
  .pill.off::before { background: var(--text-3); }
  .pill.pairing::before { background: var(--accent); }
  .kv { display: grid; grid-template-columns: 90px 1fr; gap: 6px 12px; margin: 0; }
  .kv dt { color: var(--text-3); font-size: 13px; margin: 0; }
  .kv dd { margin: 0; font-size: 13.5px; word-break: break-all; }
  ul.links, ul.devices { list-style: none; margin: 0; padding: 0; }
  ul.links li {
    display: flex; align-items: center; gap: 8px;
    border: 1px solid var(--line); border-radius: 10px;
    background: var(--surface-2); padding: 8px 10px; margin-bottom: 8px;
  }
  ul.links code {
    flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font: 12px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    color: var(--text);
  }
  ul.links .tag {
    flex: none; font-size: 10.5px; font-weight: 600; letter-spacing: 0.02em;
    color: var(--text-3); border: 1px solid var(--line); border-radius: 6px; padding: 2px 6px;
  }
  ul.devices li {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 10px 0; border-bottom: 1px solid var(--line);
  }
  ul.devices li:last-child { border-bottom: none; }
  ul.devices .device-name { font-size: 14px; font-weight: 550; }
  ul.devices .device-note { font-size: 12px; color: var(--text-3); margin-top: 2px; }
  button {
    appearance: none; border: 1px solid var(--line); background: var(--surface-2);
    color: var(--text); font: inherit; font-size: 13px; font-weight: 550;
    border-radius: 9px; padding: 8px 14px; cursor: pointer;
  }
  button:active { transform: translateY(1px); }
  button:disabled { opacity: 0.5; cursor: default; }
  button.primary { background: var(--accent); border-color: transparent; color: #0b1220; }
  button.danger { border-color: color-mix(in srgb, var(--danger) 55%, var(--line)); color: var(--danger); }
  button.small { padding: 5px 10px; font-size: 12px; }
  .row { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 12px; }
  .note { font-size: 12.5px; color: var(--text-3); margin: 0; }
  .pairing-box {
    margin-top: 12px; border: 1px dashed color-mix(in srgb, var(--accent) 45%, var(--line));
    border-radius: 10px; padding: 12px; background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .pairing-box code {
    display: block; font: 11.5px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    color: var(--text); word-break: break-all; margin: 8px 0;
  }
  .empty { color: var(--text-3); font-size: 13px; margin: 0; }
  #notice {
    margin: 0 20px 16px; border: 1px solid color-mix(in srgb, var(--danger) 50%, var(--line));
    background: color-mix(in srgb, var(--danger) 8%, transparent); color: var(--text);
    border-radius: 10px; padding: 10px 14px; font-size: 13px;
  }
  .unauthorized { padding: 40px 24px; text-align: center; color: var(--text-2); }
  .unauthorized h2 { font-size: 16px; color: var(--text); }
  .unauthorized p { font-size: 13.5px; }
  @media (prefers-reduced-motion: reduce) {
    header { backdrop-filter: none; }
  }
</style>
</head>
<body>
<header>
  <h1>Harness console</h1>
  <span id="pill" class="pill off">Connecting</span>
</header>
<main id="app" hidden>
  <div id="notice" hidden></div>

  <section class="card">
    <h2>This computer</h2>
    <dl class="kv">
      <dt>Name</dt><dd id="server-name">—</dd>
      <dt>Port</dt><dd id="server-port">—</dd>
    </dl>
    <h3>Console links</h3>
    <p class="note">The URL never changes — bookmark it.</p>
    <ul class="links" id="links"></ul>
  </section>

  <section class="card">
    <h2>Mobile access</h2>
    <p class="note" id="access-note">—</p>
    <div class="row">
      <button id="pair-btn" class="primary">Generate pairing code</button>
      <button id="stop-btn" class="danger">Stop accepting connections</button>
    </div>
    <div class="pairing-box" id="pairing-box" hidden>
      <strong>Pairing code for the native app</strong>
      <code id="pairing-link"></code>
      <button class="small" id="copy-pairing">Copy</button>
      <span class="note" id="pairing-expiry"></span>
    </div>
  </section>

  <section class="card">
    <h2>Paired devices</h2>
    <ul class="devices" id="devices"></ul>
    <p class="empty" id="devices-empty">No paired devices.</p>
  </section>
</main>

<div class="unauthorized" id="unauthorized" hidden>
  <h2>This console needs its link</h2>
  <p>Open the console URL from Harness on your computer — it carries the access token.</p>
</div>

<script>
(function () {
  'use strict'
  var token = new URLSearchParams(window.location.search).get('token')
  if (!token) {
    document.getElementById('unauthorized').hidden = false
    return
  }

  var app = document.getElementById('app')
  var pill = document.getElementById('pill')
  var notice = document.getElementById('notice')
  var state = { enabled: false, pairing: null, now: Date.now() }
  var nextId = 1
  var pending = Object.create(null)
  var socket = null
  var reconnectTimer = null

  function showNotice(message) {
    notice.textContent = message
    notice.hidden = !message
  }

  function connect() {
    var scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    var ws = new WebSocket(scheme + '//' + window.location.host + '/ws?console_token=' + encodeURIComponent(token))
    socket = ws
    ws.onopen = function () {
      pill.className = 'pill on'
      pill.textContent = 'Connected'
      refresh()
    }
    ws.onmessage = function (event) {
      var message
      try { message = JSON.parse(event.data) } catch (error) { return }
      if (message && typeof message.id === 'string' && Object.prototype.hasOwnProperty.call(pending, message.id)) {
        var entry = pending[message.id]
        delete pending[message.id]
        window.clearTimeout(entry.timer)
        if (message.error) entry.reject(new Error(message.error.message || 'request failed'))
        else entry.resolve(message.result)
      }
    }
    ws.onclose = function () {
      pill.className = 'pill off'
      pill.textContent = 'Reconnecting'
      if (reconnectTimer) window.clearTimeout(reconnectTimer)
      reconnectTimer = window.setTimeout(connect, 1500)
    }
    ws.onerror = function () { try { ws.close() } catch (error) {} }
  }

  function request(method, params) {
    return new Promise(function (resolve, reject) {
      var id = String(nextId++)
      var timer = window.setTimeout(function () {
        delete pending[id]
        reject(new Error('request timed out'))
      }, 8000)
      pending[id] = { resolve: resolve, reject: reject, timer: timer }
      if (!socket || socket.readyState !== WebSocket.OPEN) {
        window.clearTimeout(timer)
        delete pending[id]
        reject(new Error('not connected'))
        return
      }
      socket.send(JSON.stringify({ id: id, method: method, params: params || {} }))
    })
  }

  function copy(text) {
    function fallback() {
      var area = document.createElement('textarea')
      area.value = text
      area.style.position = 'fixed'
      area.style.opacity = '0'
      document.body.appendChild(area)
      area.select()
      try { document.execCommand('copy') } catch (error) {}
      document.body.removeChild(area)
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).catch(fallback)
    } else {
      fallback()
    }
  }

  function render(status) {
    state.enabled = status.enabled
    document.getElementById('server-name').textContent = status.serverName
    document.getElementById('server-port').textContent = String(status.port)
    pill.className = 'pill ' + (status.enabled ? 'on' : 'off')
    pill.textContent = status.enabled ? 'Accepting connections' : 'Off'

    var links = document.getElementById('links')
    links.textContent = ''
    status.consoleUrls.forEach(function (url) {
      var li = document.createElement('li')
      var tag = document.createElement('span')
      tag.className = 'tag'
      tag.textContent = url.indexOf('/100.') !== -1 ? 'Tailscale' : 'LAN'
      var code = document.createElement('code')
      code.textContent = url
      var copyBtn = document.createElement('button')
      copyBtn.className = 'small'
      copyBtn.textContent = 'Copy'
      copyBtn.onclick = function () { copy(url) }
      li.appendChild(tag)
      li.appendChild(code)
      li.appendChild(copyBtn)
      links.appendChild(li)
    })

    document.getElementById('access-note').textContent = status.enabled
      ? 'Paired native apps can connect right now.'
      : 'Native apps are refused until you generate a pairing code.'

    var devices = document.getElementById('devices')
    devices.textContent = ''
    document.getElementById('devices-empty').hidden = status.devices.length > 0
    status.devices.forEach(function (device) {
      var li = document.createElement('li')
      var wrap = document.createElement('div')
      var name = document.createElement('div')
      name.className = 'device-name'
      name.textContent = device.name
      var note = document.createElement('div')
      note.className = 'device-note'
      note.textContent = 'Paired ' + formatAgo(device.createdAt) + ' · seen ' + formatAgo(device.lastSeenAt)
      wrap.appendChild(name)
      wrap.appendChild(note)
      var revoke = document.createElement('button')
      revoke.className = 'danger small'
      revoke.textContent = 'Revoke'
      revoke.onclick = function () {
        revoke.disabled = true
        request('connections.revoke', { deviceId: device.id })
          .then(refresh)
          .catch(function (error) { showNotice(error.message) })
          .finally(function () { revoke.disabled = false })
      }
      li.appendChild(wrap)
      li.appendChild(revoke)
      devices.appendChild(li)
    })
  }

  function formatAgo(ms) {
    var minutes = Math.max(0, Math.floor((state.now - ms) / 60000))
    if (minutes === 0) return 'just now'
    if (minutes < 60) return minutes + 'm ago'
    var hours = Math.floor(minutes / 60)
    if (hours < 24) return hours + 'h ago'
    return Math.floor(hours / 24) + 'd ago'
  }

  function refresh() {
    request('connections.status', {}).then(render).catch(function (error) {
      showNotice(error.message)
    })
  }

  function renderPairing(offer) {
    state.pairing = { uri: offer.pairingUri, expiresAt: offer.expiresAt }
    document.getElementById('pairing-box').hidden = false
    document.getElementById('pairing-link').textContent = offer.pairingUri
    tickPairing()
  }

  function tickPairing() {
    if (!state.pairing) return
    var remaining = state.pairing.expiresAt - Date.now()
    if (remaining <= 0) {
      state.pairing = null
      document.getElementById('pairing-box').hidden = true
      return
    }
    var seconds = Math.ceil(remaining / 1000)
    document.getElementById('pairing-expiry').textContent =
      seconds >= 60 ? 'Expires in ' + Math.floor(seconds / 60) + 'm ' + (seconds % 60) + 's' : 'Expires in ' + seconds + 's'
  }

  document.getElementById('pair-btn').addEventListener('click', function () {
    var btn = document.getElementById('pair-btn')
    btn.disabled = true
    request('connections.startPairing', {})
      .then(function (offer) {
        renderPairing(offer)
        render(offer)
        showNotice('')
      })
      .catch(function (error) { showNotice(error.message) })
      .finally(function () { btn.disabled = false })
  })

  document.getElementById('stop-btn').addEventListener('click', function () {
    var btn = document.getElementById('stop-btn')
    btn.disabled = true
    request('connections.stop', {})
      .then(function () {
        state.pairing = null
        document.getElementById('pairing-box').hidden = true
        refresh()
      })
      .catch(function (error) { showNotice(error.message) })
      .finally(function () { btn.disabled = false })
  })

  document.getElementById('copy-pairing').addEventListener('click', function () {
    if (state.pairing) copy(state.pairing.uri)
  })

  app.hidden = false
  window.setInterval(function () {
    state.now = Date.now()
    tickPairing()
  }, 1000)
  window.setInterval(refresh, 3000)
  connect()
})()
</script>
</body>
</html>
`
