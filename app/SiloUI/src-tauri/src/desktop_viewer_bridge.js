// Runs in every frame of the guest desktop webview before its own scripts
// (G-20). Guest pages are untrusted content, and every click on the desktop is a
// user gesture, so they must not reach this device's clipboard or microphone.
// The top frame also gets `__silo`, the page half of the host bridge: Rust
// drives it through `Webview::eval` and it answers Rust only by posting to the
// proxy's reserved `/__silo/v1/<op>` route with a single-use nonce.
(() => {
  const refuse = () => Promise.reject(new DOMException("Computer desktops cannot use this device's clipboard.", "NotAllowedError"))
  const refuseCapture = () => Promise.reject(new DOMException("Computer desktops cannot use this device's microphone or screen.", "NotAllowedError"))
  const lock = (target, name, value) => {
    try { Object.defineProperty(target, name, { value, configurable: false, writable: false }) } catch { /* already locked */ }
  }
  if (typeof Clipboard !== "undefined") {
    for (const name of ["read", "readText", "write", "writeText"]) lock(Clipboard.prototype, name, refuse)
  }
  const clipboard = typeof navigator === "undefined" ? undefined : navigator.clipboard
  if (clipboard) {
    for (const name of ["read", "readText", "write", "writeText"]) lock(clipboard, name, refuse)
  }
  if (typeof Document !== "undefined") {
    const execCommand = Document.prototype.execCommand
    lock(Document.prototype, "execCommand", function (command, ...rest) {
      return /^(copy|cut|paste)$/i.test(String(command)) ? false : execCommand.call(this, command, ...rest)
    })
  }
  // Page handlers could replace the copied data; the user's own selection is
  // still copied by the default action. Pasting stays the user's choice.
  for (const type of ["copy", "cut"]) {
    window.addEventListener(type, event => event.stopImmediatePropagation(), true)
  }
  // Capture stays off even if the app is ever granted the OS permission and
  // whatever the Selkies server accepts.
  if (typeof MediaDevices !== "undefined") {
    for (const name of ["getUserMedia", "getDisplayMedia"]) lock(MediaDevices.prototype, name, refuseCapture)
  }
  if (typeof navigator !== "undefined") {
    if (navigator.mediaDevices) {
      for (const name of ["getUserMedia", "getDisplayMedia"]) lock(navigator.mediaDevices, name, refuseCapture)
    }
    for (const name of ["getUserMedia", "webkitGetUserMedia", "mozGetUserMedia"]) {
      lock(navigator, name, (...args) => {
        const failure = args[2]
        if (typeof failure === "function") setTimeout(() => failure(new DOMException("Computer desktops cannot use this device's microphone.", "NotAllowedError")), 0)
      })
    }
  }

  if (window.top !== window) return

  // Selkies reads its client settings from localStorage under the sanitized
  // origin and path (lib/util.js getStorageAppName). Seamless sync would let
  // the page write the device clipboard on every guest copy.
  try {
    const prefix = `${location.origin}${location.pathname}`.replace(/[^a-zA-Z0-9._-]/g, "_")
    window.localStorage.setItem(`${prefix}_clipboard_seamless`, "false")
  } catch { /* opaque origin: not a Selkies page */ }

  const nativeFetch = window.fetch.bind(window)
  const nativePost = window.postMessage.bind(window)
  const encoder = new TextEncoder()
  const ROUTE = "/__silo/v1/"
  // Mirrors the Rust per-operation cap; larger payloads are dropped here.
  const MAX_CLIPBOARD_BYTES = 24 * 1024 * 1024
  const TEXT_MIME = "text/plain"
  // Frames Rust may send (Selkies input_handler.py `_dispatch_message`): `cw`,
  // `cws`, `cwd`, `cwe` write text; `cb`, `cbs`, `cbd`, `cbe` write binary; `kd` and
  // `ku` are key events; `r,WxH` resizes; `REQUEST_CLIPBOARD` asks the server to
  // push its current selection.
  const ALLOWED_FRAME = /^(?:cw,[A-Za-z0-9+/=]*|cws,[^,]+,\d+|cwd,[^,]+,[A-Za-z0-9+/=]*|cwe,[^,]+|cb,[^,]+,[A-Za-z0-9+/=]*|cbs,[^,]+,[^,]+,\d+|cbd,[^,]+,[A-Za-z0-9+/=]*|cbe,[^,]+|kd,\d+|ku,\d+|r,\d+x\d+,primary|REQUEST_CLIPBOARD)$/

  // The last clipboard payload the guest announced, kept as base64 so nothing
  // is decoded or sent anywhere until Rust asks for it.
  let latest = null
  let assembly = null
  let sequence = 0
  const waiters = new Set()

  const base64Size = encoded => {
    const padding = encoded.endsWith("==") ? 2 : encoded.endsWith("=") ? 1 : 0
    return Math.floor(encoded.length * 3 / 4) - padding
  }
  const record = (mime, encoded) => {
    if (base64Size(encoded) > MAX_CLIPBOARD_BYTES) return
    latest = { mime, encoded, sequence: ++sequence }
    for (const waiter of [...waiters]) waiter(latest)
  }
  // Selkies websockets_mode.py `send_ws_clipboard_data`: `clipboard,<b64>` and
  // `clipboard_binary,<mime>,<b64>` below 16 KiB; otherwise `clipboard_start,<mime>,<size>`,
  // `clipboard_data,<b64>` chunks and `clipboard_finish`.
  const observe = data => {
    if (typeof data !== "string" || !data.startsWith("clipboard")) return
    if (data.startsWith("clipboard,")) {
      assembly = null
      record(TEXT_MIME, data.slice(10))
    } else if (data.startsWith("clipboard_binary,")) {
      assembly = null
      const split = data.indexOf(",", 17)
      if (split > 17) record(data.slice(17, split), data.slice(split + 1))
    } else if (data.startsWith("clipboard_start,")) {
      const [, mime, size] = data.split(",")
      const declared = Number(size)
      assembly = mime && Number.isSafeInteger(declared) && declared >= 0 && declared <= MAX_CLIPBOARD_BYTES
        ? { mime, declared, chunks: [], length: 0 } : null
    } else if (data.startsWith("clipboard_data,")) {
      if (!assembly) return
      const chunk = data.slice(15)
      assembly.length += chunk.length
      if (assembly.length > Math.ceil(assembly.declared / 3) * 4 + 4) assembly = null
      else assembly.chunks.push(chunk)
    } else if (data === "clipboard_finish") {
      const done = assembly
      assembly = null
      if (!done) return
      const encoded = done.chunks.join("")
      if (base64Size(encoded) === done.declared) record(done.mime, encoded)
    }
  }
  const watching = new WeakSet()
  const watch = socket => {
    if (!socket || typeof socket.addEventListener !== "function" || watching.has(socket)) return
    watching.add(socket)
    latest = null
    assembly = null
    socket.addEventListener("message", event => observe(event && event.data))
  }
  // Selkies assigns `window.selkiesTransport = websocket` for every connection
  // (selkies-ws-core.js); the setter sees each one without polling.
  let transport = null
  try {
    Object.defineProperty(window, "selkiesTransport", {
      configurable: true,
      enumerable: true,
      get: () => transport,
      set: value => { transport = value; watch(value) },
    })
  } catch { /* the page already owns the property */ }

  const post = async (op, nonce, kind, body) => {
    const query = `nonce=${encodeURIComponent(nonce)}&kind=${encodeURIComponent(kind)}`
    try {
      await nativeFetch(`${ROUTE}${op}?${query}`, { method: "POST", body, cache: "no-store", credentials: "same-origin" })
    } catch { /* Rust times out on its own */ }
  }
  const decode = encoded => {
    const text = atob(encoded)
    const bytes = new Uint8Array(text.length)
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i)
    return bytes
  }
  // Absent in WebRTC mode; the socket silently drops sends unless it is open.
  const socketReady = () => Boolean(transport) && typeof transport.send === "function"
    && (transport.readyState === undefined || transport.readyState === 1)
  const sendFrames = frames => {
    if (!Array.isArray(frames) || !socketReady()) return false
    if (!frames.every(frame => typeof frame === "string" && ALLOWED_FRAME.test(frame))) return false
    for (const frame of frames) transport.send(frame)
    return true
  }
  // Answers with the next announcement after the request, or after the timeout
  // with the last one announced earlier (kind `none` when there is none).
  const requestClipboard = (nonce, timeoutMs, frames) => {
    const started = sequence
    let settled = false
    const waiter = payload => { if (payload.sequence > started) finish(payload) }
    const finish = payload => {
      if (settled) return
      settled = true
      waiters.delete(waiter)
      clearTimeout(timer)
      void (payload ? post("clipboard", nonce, payload.mime, decode(payload.encoded)) : post("clipboard", nonce, "none", new Uint8Array(0)))
    }
    const timer = setTimeout(() => finish(latest), Math.max(0, Math.min(Number(timeoutMs) || 0, 10000)))
    waiters.add(waiter)
    sendFrames(frames)
  }
  const capabilities = async nonce => {
    let opus = false
    try {
      if (typeof AudioDecoder !== "undefined") {
        const support = await AudioDecoder.isConfigSupported({ codec: "opus", sampleRate: 48000, numberOfChannels: 2 })
        opus = Boolean(support && support.supported)
      }
    } catch { /* unsupported */ }
    const report = { audioDecoder: typeof AudioDecoder !== "undefined", opus, transport: socketReady() }
    await post("capabilities", nonce, "application/json", encoder.encode(JSON.stringify(report)))
  }
  // Selkies applies mute and volume to a gain node that exists only once audio
  // is flowing, so the wanted values are re-applied when the pipeline changes.
  const audio = { muted: null, volume: null }
  const applyAudio = () => {
    if (audio.muted !== null) nativePost({ type: "setMute", value: audio.muted }, location.origin)
    if (audio.volume !== null) nativePost({ type: "setVolume", value: audio.volume }, location.origin)
  }
  window.addEventListener("message", event => {
    if (event.origin === location.origin && event.data && event.data.type === "pipelineStatusUpdate") applyAudio()
  })
  const methods = {
    sendFrames,
    requestClipboard,
    capabilities,
    setMute: muted => { audio.muted = muted === true; applyAudio() },
    setVolume: volume => { audio.volume = Math.max(0, Math.min(1, Number(volume) || 0)); applyAudio() },
    // Selkies has no frame for this from the page; its own control message
    // keeps the client's pipeline state in step with the server.
    setAudioActive: active => {
      nativePost({ type: "pipelineControl", pipeline: "audio", enabled: active === true }, location.origin)
      if (active === true) for (const delay of [500, 1500, 3000]) setTimeout(applyAudio, delay)
    },
    resetResolutionToWindow: () => nativePost({ type: "resetResolutionToWindow" }, location.origin),
  }
  const bridge = Object.freeze({
    invoke(method, args) {
      const call = Object.prototype.hasOwnProperty.call(methods, method) ? methods[method] : null
      if (!call) return false
      return call(...(Array.isArray(args) ? args : [])) !== false
    },
  })
  lock(window, "__silo", bridge)
})()
