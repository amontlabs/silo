// Runs in every frame of the guest desktop webview before its own scripts
// (G-20). Guest pages are untrusted content, and every click on the desktop is a
// user gesture, so they must not reach this device's clipboard or microphone.
// The top frame also gets `__silo`, the page half of the host bridge: Rust
// drives it through `Webview::eval` and it answers Rust only by posting to the
// proxy's reserved `/__silo/v1/<op>` route with a single-use nonce: clipboard
// content, capabilities, and an acknowledgement for every send.
(() => {
  // Everything below runs before the page's scripts; the intrinsics it relies
  // on are captured now so later tampering with prototypes cannot redirect it.
  const apply = Reflect.apply
  const defineProperty = Object.defineProperty
  const freeze = Object.freeze
  const hasOwn = Object.prototype.hasOwnProperty
  const toText = String
  const toLower = String.prototype.toLowerCase
  const arrayIsArray = Array.isArray
  const regexTest = RegExp.prototype.test
  const toNumber = Number
  const isSafeInteger = Number.isSafeInteger
  const objectKeys = Object.keys
  const stringSlice = String.prototype.slice
  const refuse = () => Promise.reject(new DOMException("Computer desktops cannot use this device's clipboard.", "NotAllowedError"))
  const refuseCapture = () => Promise.reject(new DOMException("Computer desktops cannot use this device's microphone or screen.", "NotAllowedError"))
  const lock = (target, name, value) => {
    try { defineProperty(target, name, { value, configurable: false, writable: false }) } catch { /* already locked */ }
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
    // The command is converted once; the native method only ever sees that
    // validated primitive, never the page's object.
    lock(Document.prototype, "execCommand", function (command, showUI, value) {
      const name = apply(toLower, toText(command), [])
      if (name === "copy" || name === "cut" || name === "paste") return false
      return apply(execCommand, this, [name, showUI, value])
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
  // is decoded or sent anywhere until Rust asks for it. An announcement above
  // the cap replaces it with an `oversized` marker, so a later copy never
  // answers with older content.
  let latest = null
  let assembly = null
  let sequence = 0
  const waiters = new Set()

  const base64Size = encoded => {
    const padding = encoded.endsWith("==") ? 2 : encoded.endsWith("=") ? 1 : 0
    return Math.floor(encoded.length * 3 / 4) - padding
  }
  const publish = payload => {
    latest = payload
    for (const waiter of [...waiters]) waiter(latest)
  }
  const recordOversized = () => publish({ mime: "", encoded: "", oversized: true, sequence: ++sequence })
  const record = (mime, encoded) => {
    if (base64Size(encoded) > MAX_CLIPBOARD_BYTES) recordOversized()
    else publish({ mime, encoded, oversized: false, sequence: ++sequence })
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
      assembly = null
      if (!mime || !Number.isSafeInteger(declared) || declared < 0) return
      if (declared > MAX_CLIPBOARD_BYTES) recordOversized()
      else assembly = { mime, declared, chunks: [], length: 0 }
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

  // X11 modifier keysyms: Shift, Control, Meta, Alt, Super and Hyper (left and
  // right), ISO_Level3_Shift and Mode_switch.
  const isModifierKeysym = keysym => (keysym >= 0xffe1 && keysym <= 0xffe4) || (keysym >= 0xffe7 && keysym <= 0xffee)
    || keysym === 0xfe03 || keysym === 0xff7e
  // The modifiers the guest holds down, learned from the frames that go through
  // the socket (`kd,<keysym>`, `ku,<keysym>`, `kr`). The `kh` heartbeat only
  // refreshes keys the server already holds, so it never presses one again
  // after a release.
  let heldModifiers = Object.create(null)
  const track = frame => {
    if (typeof frame !== "string") return
    if (frame === "kr") { heldModifiers = Object.create(null); return }
    const kind = apply(stringSlice, frame, [0, 3])
    if (kind !== "kd," && kind !== "ku,") return
    const keysym = toNumber(apply(stringSlice, frame, [3]))
    if (!isSafeInteger(keysym) || !isModifierKeysym(keysym)) return
    if (kind === "kd,") heldModifiers[keysym] = true
    else delete heldModifiers[keysym]
  }
  const releaseHeldModifiers = () => objectKeys(heldModifiers).map(keysym => `ku,${keysym}`)

  const watching = new WeakSet()
  // Each socket's own `send`. The socket is wrapped so the frames Selkies sends
  // keep `heldModifiers` current; Rust's frames go to the original.
  const senders = new WeakMap()
  const watch = socket => {
    if (!socket || typeof socket.addEventListener !== "function" || watching.has(socket)) return
    watching.add(socket)
    latest = null
    assembly = null
    heldModifiers = Object.create(null)
    const original = socket.send
    if (typeof original === "function") {
      senders.set(socket, original)
      try {
        defineProperty(socket, "send", {
          configurable: true,
          writable: true,
          value: function (data) {
            track(data)
            return apply(original, this, [data])
          },
        })
      } catch { /* a socket that cannot be wrapped leaves the held modifiers unknown */ }
    }
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

  // Rust accepts only these characters in the kind.
  const KIND = /^[A-Za-z0-9/.+-]{1,64}$/
  const post = async (op, nonce, kind, body) => {
    if (!apply(regexTest, KIND, [kind])) return
    const query = `nonce=${encodeURIComponent(nonce)}&kind=${encodeURIComponent(kind)}`
    try {
      await nativeFetch(`${ROUTE}${op}?${query}`, { method: "POST", body, cache: "no-store", credentials: "same-origin" })
    } catch { /* Rust times out on its own */ }
  }
  const NO_BODY = () => new Uint8Array(0)
  const decode = encoded => {
    const text = atob(encoded)
    const bytes = new Uint8Array(text.length)
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i)
    return bytes
  }
  // Absent in WebRTC mode; the socket silently drops sends unless it is open.
  const socketReady = () => Boolean(transport) && typeof transport.send === "function"
    && (transport.readyState === undefined || transport.readyState === 1)
  const wellFormed = frames => {
    if (!arrayIsArray(frames)) return false
    const count = frames.length
    for (let i = 0; i < count; i++) {
      if (typeof frames[i] !== "string" || !apply(regexTest, ALLOWED_FRAME, [frames[i]])) return false
    }
    return true
  }
  // `refused` for frames the helper will not send, `closed` for a transport
  // that is not open, `ok` once every frame went to the socket. With `release`
  // every modifier the guest holds is released first.
  const deliver = (frames, release) => {
    if (!wellFormed(frames)) return "refused"
    if (!socketReady()) return "closed"
    const send = senders.get(transport) || transport.send
    const all = release ? releaseHeldModifiers().concat(frames) : frames
    const count = all.length
    for (let i = 0; i < count; i++) {
      track(all[i])
      apply(send, transport, [all[i]])
    }
    return "ok"
  }
  // Rust waits for this answer under `nonce`; without one the outcome is only
  // the return value.
  const acknowledge = (nonce, outcome) => {
    if (typeof nonce === "string") void post("sent", nonce, outcome, NO_BODY())
    return outcome === "ok"
  }
  const sendFrames = (frames, nonce) => acknowledge(nonce, deliver(frames, false))
  // A shortcut chord goes out with the modifiers the guest holds released first,
  // so Ctrl+Shift+C or Command+V arrives as a bare Ctrl+C or Ctrl+V.
  const sendShortcut = (frames, nonce) => acknowledge(nonce, deliver(frames, true))
  // Answers with the next announcement that differs from the one cached when the
  // request started, or after the timeout with the last one announced (kind
  // `none` when there is none). Selkies answers REQUEST_CLIPBOARD at once, so a
  // repeat of the cached content can arrive before the application publishes the
  // new selection. A copy shortcut on a connection that has announced nothing yet
  // first asks for the current selection and waits for that answer (at most
  // BASELINE_WAIT_MS), so the old selection is not taken for the copied one. When
  // that answer is still missing, the shortcut goes out anyway and the first
  // announcement afterwards may be the late answer rather than the copy, so it is
  // held for BASELINE_GRACE_MS: a different announcement within that time wins, and
  // otherwise the held one is the answer.
  // Kinds `too-large` (an announcement above the cap), `unreadable` (data that is
  // not base64), `disconnected` and `refused` carry no content.
  const BASELINE_WAIT_MS = 500
  const BASELINE_GRACE_MS = 300
  const requestClipboard = (nonce, timeoutMs, frames, shortcut) => {
    const wait = Math.max(0, Math.min(Number(timeoutMs) || 0, 10000))
    let baseline = latest
    let started = sequence
    let settled = false
    let baselineTimer
    let graceTimer
    // Set when the shortcut went out without the guest's current selection; the
    // announcement held as possibly that selection.
    let unbaselined = false
    let held = null
    // Removes every waiter and timer of this request; false once it was settled.
    const settle = () => {
      if (settled) return false
      settled = true
      waiters.delete(waiter)
      waiters.delete(baselineWaiter)
      clearTimeout(timer)
      clearTimeout(baselineTimer)
      clearTimeout(graceTimer)
      return true
    }
    const answer = (kind, body) => {
      if (settle()) void post("clipboard", nonce, kind, body)
    }
    const finish = payload => {
      if (!settle()) return
      if (!payload) { void post("clipboard", nonce, "none", NO_BODY()); return }
      if (payload.oversized) { void post("clipboard", nonce, "too-large", NO_BODY()); return }
      let bytes
      try { bytes = decode(payload.encoded) } catch { void post("clipboard", nonce, "unreadable", NO_BODY()); return }
      void post("clipboard", nonce, payload.mime, bytes)
    }
    const differs = (payload, other) => payload.oversized || !other || other.oversized || payload.mime !== other.mime || payload.encoded !== other.encoded
    const changed = payload => payload.sequence > started && differs(payload, baseline)
    const waiter = payload => {
      if (payload.sequence <= started) return
      if (!unbaselined) { if (changed(payload)) finish(payload); return }
      if (!held) {
        held = payload
        graceTimer = setTimeout(() => finish(held), BASELINE_GRACE_MS)
      } else if (differs(payload, held)) finish(payload)
    }
    const send = () => {
      if (settled) return
      baseline = latest
      started = sequence
      waiters.add(waiter)
      const outcome = deliver(frames, shortcut === true)
      if (outcome !== "ok") answer(outcome === "closed" ? "disconnected" : "refused", NO_BODY())
    }
    let proceeded = false
    const proceed = answered => {
      if (proceeded || settled) return
      proceeded = true
      waiters.delete(baselineWaiter)
      clearTimeout(baselineTimer)
      send()
      unbaselined = !answered && !baseline
    }
    const baselineWaiter = () => proceed(true)
    const timer = setTimeout(() => { if (socketReady()) finish(latest); else answer("disconnected", NO_BODY()) }, wait)
    if (!socketReady()) { answer("disconnected", NO_BODY()); return }
    if (shortcut === true && !latest) {
      waiters.add(baselineWaiter)
      const outcome = deliver(["REQUEST_CLIPBOARD"], false)
      if (outcome !== "ok") { answer(outcome === "closed" ? "disconnected" : "refused", NO_BODY()); return }
      baselineTimer = setTimeout(() => proceed(false), Math.min(BASELINE_WAIT_MS, wait))
    } else send()
  }
  const capabilities = async nonce => {
    let opus = false
    try {
      if (typeof AudioDecoder !== "undefined") {
        const support = await AudioDecoder.isConfigSupported({ codec: "opus", sampleRate: 48000, numberOfChannels: 2 })
        opus = Boolean(support && support.supported)
      }
    } catch { /* unsupported */ }
    // Selkies mirrors its server settings onto `window` once the connection
    // reports them; `null` means they have not arrived.
    const setting = name => typeof window[name] === "boolean" ? window[name] : null
    const report = {
      audioDecoder: typeof AudioDecoder !== "undefined",
      opus,
      transport: socketReady(),
      clipboard: setting("clipboard_enabled"),
      clipboardIn: setting("clipboard_in_enabled"),
      clipboardOut: setting("clipboard_out_enabled"),
    }
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
    sendShortcut,
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
  const bridge = freeze({
    invoke(method, args) {
      const call = typeof method === "string" && apply(hasOwn, methods, [method]) ? methods[method] : null
      if (!call) return false
      return apply(call, undefined, arrayIsArray(args) ? args : []) !== false
    },
  })
  lock(window, "__silo", bridge)
})()
