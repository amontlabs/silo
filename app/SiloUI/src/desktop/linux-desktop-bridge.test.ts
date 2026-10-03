import { readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest"

// The page half of the host bridge (src-tauri/src/desktop_viewer_bridge.js),
// run against a fake Selkies socket the way Rust drives it through eval.
const native = resolve(dirname(fileURLToPath(import.meta.url)), "../../src-tauri")
const script = readFileSync(resolve(native, "src/desktop_viewer_bridge.js"), "utf8")
// Requests the script must produce, also fed to the Rust parser (desktop_bridge.rs).
const contract = JSON.parse(readFileSync(resolve(native, "src/desktop_bridge_contract.json"), "utf8")) as Array<{ name: string; url: string; body: string; kind: string }>

class FakeSocket {
  readyState?: number
  sent: string[] = []
  listeners: Array<(event: { data: unknown }) => void> = []
  send = (frame: string) => { this.sent.push(frame) }
  addEventListener(type: string, listener: (event: { data: unknown }) => void) {
    if (type === "message") this.listeners.push(listener)
  }
  receive(data: unknown) { for (const listener of this.listeners) listener({ data }) }
}

type Bridge = { invoke: (method: string, args: unknown[]) => boolean }
const bridge = () => (window as unknown as { __silo: Bridge }).__silo
const call = (method: string, ...args: unknown[]) => bridge().invoke(method, args)
const setTransport = (socket: FakeSocket | null) => { (window as unknown as { selkiesTransport: unknown }).selkiesTransport = socket }
const fetchMock = vi.fn(async (..._args: unknown[]) => ({}))
const posted = () => fetchMock.mock.calls.map(([url, init]) => ({ url: new URL(String(url), location.origin), init: init as RequestInit }))
const b64 = (text: string) => btoa(text)
const body = (init: RequestInit) => new TextDecoder().decode(init.body as Uint8Array)

const SHIFT_L = 65505
const CONTROL_L = 65507
const SUPER_L = 65515
const CHORD = ["kd,65507", "kd,118", "ku,118", "ku,65507"]
const MiB = 1024 * 1024
const flush = () => new Promise(resolve => setTimeout(resolve, 20))
const requestKinds = () => posted().map(request => request.url.searchParams.get("kind"))

let socket: FakeSocket
beforeAll(() => {
  vi.stubGlobal("fetch", fetchMock)
  window.eval(script)
})
beforeEach(() => {
  fetchMock.mockClear()
  socket = new FakeSocket()
  setTransport(socket)
})

describe("host bridge page helper", () => {
  it("cannot be replaced or removed by the page", () => {
    expect(() => { (window as unknown as Record<string, unknown>).__silo = {} }).toThrow()
    expect(() => { delete (window as unknown as Record<string, unknown>).__silo }).toThrow()
    expect(call("notAMethod")).toBe(false)
    expect(call("constructor")).toBe(false)
  })

  it("sends only well-formed Selkies frames through the socket", () => {
    expect(call("sendFrames", ["cw,aGk=", "kd,65507", "ku,65507", "r,1440x900,primary", "REQUEST_CLIPBOARD"])).toBe(true)
    expect(socket.sent).toEqual(["cw,aGk=", "kd,65507", "ku,65507", "r,1440x900,primary", "REQUEST_CLIPBOARD"])
    socket.sent.length = 0
    expect(call("sendFrames", ["cmd,rm -rf /"])).toBe(false)
    expect(call("sendFrames", ["kd,65507", "js,c,1"])).toBe(false)
    expect(call("sendFrames", ["START_AUDIO"])).toBe(false)
    expect(call("sendFrames", "cw,aGk=")).toBe(false)
    expect(socket.sent).toEqual([])
  })

  it("reports an unavailable transport instead of pretending to send", async () => {
    setTransport(null)
    expect(call("sendFrames", ["REQUEST_CLIPBOARD"])).toBe(false)
    const closed = new FakeSocket()
    closed.readyState = 3
    setTransport(closed)
    expect(call("sendFrames", ["REQUEST_CLIPBOARD"])).toBe(false)
    expect(closed.sent).toEqual([])
    vi.stubGlobal("AudioDecoder", undefined)
    call("capabilities", "nonce-0")
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(JSON.parse(body(posted()[0].init))).toEqual({ audioDecoder: false, opus: false, transport: false, clipboard: null, clipboardIn: null, clipboardOut: null })
  })

  it("never posts clipboard content to the host unprompted", async () => {
    socket.receive(`clipboard,${b64("secret")}`)
    socket.receive(`clipboard_binary,image/png,${b64("png")}`)
    await new Promise(resolve => setTimeout(resolve, 20))
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it("answers a clipboard request with the next announced text under one nonce", async () => {
    call("requestClipboard", "nonce-1", 2000, ["kd,65507", "kd,99", "ku,99", "ku,65507", "REQUEST_CLIPBOARD"])
    expect(socket.sent).toEqual(["kd,65507", "kd,99", "ku,99", "ku,65507", "REQUEST_CLIPBOARD"])
    socket.receive(`clipboard,${b64("hello world")}`)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    const [request] = posted()
    expect(request.url.pathname).toBe("/__silo/v1/clipboard")
    expect(request.url.searchParams.get("nonce")).toBe("nonce-1")
    expect(request.url.searchParams.get("kind")).toBe("text/plain")
    expect(request.init.method).toBe("POST")
    expect(body(request.init)).toBe("hello world")
    socket.receive(`clipboard,${b64("later")}`)
    await new Promise(resolve => setTimeout(resolve, 20))
    expect(fetchMock).toHaveBeenCalledOnce()
  })

  it("falls back to the last announced payload when nothing new arrives", async () => {
    socket.receive(`clipboard_binary,image/png,${b64("PNGDATA")}`)
    call("requestClipboard", "nonce-2", 30, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    const [request] = posted()
    expect(request.url.searchParams.get("kind")).toBe("image/png")
    expect(body(request.init)).toBe("PNGDATA")
  })

  it("reports kind none when the guest has announced nothing", async () => {
    setTransport(new FakeSocket())
    call("requestClipboard", "nonce-3", 10, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    const [request] = posted()
    expect(request.url.searchParams.get("kind")).toBe("none")
    expect(body(request.init)).toBe("")
  })

  it("reassembles chunked clipboard payloads and ignores ones with the wrong size", async () => {
    const data = "0123456789abcdefghij"
    const encoded = b64(data)
    socket.receive(`clipboard_start,image/png,${data.length}`)
    socket.receive(`clipboard_data,${encoded.slice(0, 12)}`)
    socket.receive(`clipboard_data,${encoded.slice(12)}`)
    socket.receive("clipboard_finish")
    call("requestClipboard", "nonce-4", 10, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(body(posted()[0].init)).toBe(data)

    fetchMock.mockClear()
    socket.receive("clipboard_start,text/plain,999")
    socket.receive(`clipboard_data,${b64("short")}`)
    socket.receive("clipboard_finish")
    call("requestClipboard", "nonce-5", 10, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    // The earlier valid payload is still the last announcement.
    expect(body(posted()[0].init)).toBe(data)
  })

  it("forgets announcements from a replaced socket", async () => {
    socket.receive(`clipboard,${b64("old")}`)
    setTransport(new FakeSocket())
    call("requestClipboard", "nonce-6", 10, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(posted()[0].url.searchParams.get("kind")).toBe("none")
  })

  it("posts audio capabilities for a nonce", async () => {
    vi.stubGlobal("AudioDecoder", { isConfigSupported: vi.fn(async () => ({ supported: true })) })
    call("capabilities", "nonce-7")
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    const [request] = posted()
    expect(request.url.pathname).toBe("/__silo/v1/capabilities")
    expect(JSON.parse(body(request.init))).toEqual({ audioDecoder: true, opus: true, transport: true, clipboard: null, clipboardIn: null, clipboardOut: null })
  })

  it("posts the Selkies same-origin verbs for mute, volume and resolution", async () => {
    const messages: unknown[] = []
    const listener = (event: MessageEvent) => messages.push(event.data)
    window.addEventListener("message", listener)
    call("setMute", true)
    call("setVolume", 4)
    call("resetResolutionToWindow")
    call("setAudioActive", false)
    await new Promise(resolve => setTimeout(resolve, 30))
    window.removeEventListener("message", listener)
    expect(messages).toEqual([
      { type: "setMute", value: true },
      { type: "setMute", value: true },
      { type: "setVolume", value: 1 },
      { type: "resetResolutionToWindow" },
      { type: "pipelineControl", pipeline: "audio", enabled: false },
    ])
  })

  it("re-applies the wanted mute and volume when the audio pipeline reports in", async () => {
    call("setMute", false)
    const messages: unknown[] = []
    const listener = (event: MessageEvent) => messages.push(event.data)
    window.addEventListener("message", listener)
    window.postMessage({ type: "pipelineStatusUpdate", audio: true }, location.origin)
    await new Promise(resolve => setTimeout(resolve, 30))
    window.removeEventListener("message", listener)
    expect(messages).toContainEqual({ type: "setMute", value: false })
    expect(messages).toContainEqual({ type: "setVolume", value: 1 })
  })

  it("produces exactly the requests the Rust contract fixture accepts", async () => {
    const sent: Record<string, { url: string; body: string }> = {}
    const run = async (name: string, start: () => void) => {
      fetchMock.mockClear()
      start()
      await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
      const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
      sent[name] = { url: String(url), body: body(init) }
    }
    socket.receive(`clipboard,${b64("hello world")}`)
    await run("clipboard text", () => call("requestClipboard", "abc123", 10, []))
    socket.receive(`clipboard_binary,image/png,${b64("PNGDATA")}`)
    await run("clipboard image", () => call("requestClipboard", "abc123", 10, []))
    setTransport(new FakeSocket())
    await run("clipboard empty", () => call("requestClipboard", "abc123", 10, []))
    vi.stubGlobal("AudioDecoder", undefined)
    await run("capabilities", () => call("capabilities", "abc123"))
    socket.receive(`clipboard_start,text/plain,${31 * 1024 * 1024}`)
    await run("clipboard too large", () => call("requestClipboard", "abc123", 10, []))
    setTransport(null)
    await run("clipboard disconnected", () => call("requestClipboard", "abc123", 10, []))
    const flavoured = new FakeSocket()
    setTransport(flavoured)
    const envelope = JSON.stringify({ "text/html": "<b>hi</b>", "text/plain": "hi" })
    flavoured.receive(`clipboard_binary,application/x-selkies-clipboard-flavours,${b64(envelope)}`)
    await run("clipboard flavours", () => call("requestClipboard", "abc123", 10, []))
    await run("sent ok", () => call("sendFrames", ["kd,65"], "abc123"))
    flavoured.readyState = 3
    await run("sent closed", () => call("sendFrames", ["kd,65"], "abc123"))
    expect(Object.keys(sent).sort()).toEqual(contract.map(entry => entry.name).sort())
    for (const entry of contract) expect(sent[entry.name], entry.name).toEqual({ url: entry.url, body: entry.body })
  })

  it("never posts a kind the host would refuse", async () => {
    socket.receive(`clipboard_binary,bad kind?x=1&y,${b64("x")}`)
    call("requestClipboard", "abc123", 10, [])
    await new Promise(resolve => setTimeout(resolve, 30))
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it("acknowledges a send under its nonce with ok, closed or refused", async () => {
    expect(call("sendFrames", ["kd,65"], "ack-1")).toBe(true)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    const [ok] = posted()
    expect(ok.url.pathname).toBe("/__silo/v1/sent")
    expect(ok.url.searchParams.get("nonce")).toBe("ack-1")
    expect(ok.url.searchParams.get("kind")).toBe("ok")
    expect(ok.init.body).toHaveLength(0)

    expect(call("sendFrames", ["cmd,rm -rf /"], "ack-2")).toBe(false)
    socket.readyState = 3
    expect(call("sendFrames", ["kd,65"], "ack-3")).toBe(false)
    expect(call("sendShortcut", CHORD, "ack-4")).toBe(false)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(4))
    expect(requestKinds()).toEqual(["ok", "refused", "closed", "closed"])
    expect(socket.sent).toEqual(["kd,65"])
    setTransport(null)
    expect(call("sendFrames", ["kd,65"], "ack-5")).toBe(false)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(5))
    expect(requestKinds()[4]).toBe("closed")
  })

  it("releases the modifiers the page holds before a shortcut chord and leaves them released", () => {
    // What Selkies sends while the user holds Ctrl+Shift+C or Command+V.
    for (const frame of [`kd,${CONTROL_L}`, `kd,${SHIFT_L}`, `kd,${SUPER_L}`, "kd,65", "ku,65", `kh,${CONTROL_L},${SHIFT_L}`]) socket.send(frame)
    socket.sent.length = 0
    expect(call("sendShortcut", CHORD, "chord-1")).toBe(true)
    expect(socket.sent).toEqual([`ku,${SHIFT_L}`, `ku,${CONTROL_L}`, `ku,${SUPER_L}`, ...CHORD])

    socket.sent.length = 0
    expect(call("sendShortcut", CHORD, "chord-2")).toBe(true)
    expect(socket.sent).toEqual(CHORD)

    // A key the page presses again after the chord is held again.
    socket.send(`kd,${SHIFT_L}`)
    socket.sent.length = 0
    call("sendShortcut", CHORD, "chord-3")
    expect(socket.sent).toEqual([`ku,${SHIFT_L}`, ...CHORD])
  })

  it("forgets modifiers the page released, and all of them on a keyboard reset or a new socket", () => {
    socket.send(`kd,${SHIFT_L}`)
    socket.send(`ku,${SHIFT_L}`)
    socket.send(`kd,${SUPER_L}`)
    socket.send("kr")
    socket.sent.length = 0
    call("sendShortcut", CHORD, "chord-4")
    expect(socket.sent).toEqual(CHORD)

    socket.send(`kd,${SHIFT_L}`)
    const replacement = new FakeSocket()
    setTransport(replacement)
    call("sendShortcut", CHORD, "chord-5")
    expect(replacement.sent).toEqual(CHORD)
  })

  it("does not treat other keys or malformed frames as modifiers", () => {
    for (const frame of ["kd,65", "kd,65506x", "kd,-1", "kd,", "kd,99999999999999999999", "m,1,2,0,0", `ku,${CONTROL_L}`]) socket.send(frame)
    socket.sent.length = 0
    call("sendShortcut", CHORD, "chord-6")
    expect(socket.sent).toEqual(CHORD)
  })

  it("releases held modifiers around the Ctrl+C of a clipboard request too", async () => {
    socket.receive(`clipboard,${b64("cached")}`)
    socket.send(`kd,${SHIFT_L}`)
    socket.sent.length = 0
    call("requestClipboard", "copy-1", 20, [...CHORD, "REQUEST_CLIPBOARD"], true)
    expect(socket.sent).toEqual([`ku,${SHIFT_L}`, ...CHORD, "REQUEST_CLIPBOARD"])
    socket.sent.length = 0
    call("requestClipboard", "copy-2", 20, [...CHORD, "REQUEST_CLIPBOARD"], false)
    expect(socket.sent).toEqual([...CHORD, "REQUEST_CLIPBOARD"])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
  })

  it("keeps waiting when the first reply repeats the cached clipboard, and answers once the content changes", async () => {
    socket.receive(`clipboard,${b64("old")}`)
    call("requestClipboard", "stale-1", 2000, ["REQUEST_CLIPBOARD"])
    socket.receive(`clipboard,${b64("old")}`)
    socket.receive(`clipboard_binary,text/plain,${b64("old")}`)
    await flush()
    expect(fetchMock).not.toHaveBeenCalled()
    socket.receive(`clipboard,${b64("new")}`)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(body(posted()[0].init)).toBe("new")
  })

  it("asks for the current selection before a copy shortcut when nothing is cached, so the old selection is not taken for the copy", async () => {
    call("requestClipboard", "fresh-1", 2000, [...CHORD, "REQUEST_CLIPBOARD"], true)
    expect(socket.sent).toEqual(["REQUEST_CLIPBOARD"])
    socket.receive(`clipboard,${b64("old")}`)
    expect(socket.sent).toEqual(["REQUEST_CLIPBOARD", ...CHORD, "REQUEST_CLIPBOARD"])
    await flush()
    expect(fetchMock).not.toHaveBeenCalled()
    socket.receive(`clipboard,${b64("old")}`)
    await flush()
    expect(fetchMock).not.toHaveBeenCalled()
    socket.receive(`clipboard,${b64("copied")}`)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(body(posted()[0].init)).toBe("copied")
  })

  it("sends the copy shortcut after a short wait when the guest has no selection to report", async () => {
    call("requestClipboard", "fresh-2", 2000, [...CHORD, "REQUEST_CLIPBOARD"], true)
    expect(socket.sent).toEqual(["REQUEST_CLIPBOARD"])
    await vi.waitFor(() => expect(socket.sent).toEqual(["REQUEST_CLIPBOARD", ...CHORD, "REQUEST_CLIPBOARD"]))
    socket.receive(`clipboard,${b64("first copy")}`)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(body(posted()[0].init)).toBe("first copy")
  })

  it("does not send the copy shortcut once the request already ended", async () => {
    call("requestClipboard", "fresh-3", 10, [...CHORD, "REQUEST_CLIPBOARD"], true)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(requestKinds()).toEqual(["none"])
    await new Promise(resolve => setTimeout(resolve, 600))
    expect(socket.sent).toEqual(["REQUEST_CLIPBOARD"])
  })

  it("answers malformed clipboard data with an explicit failure and leaves nothing pending", async () => {
    socket.receive("clipboard,!")
    call("requestClipboard", "bad-1", 20, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(requestKinds()).toEqual(["unreadable"])
    expect(posted()[0].init.body).toHaveLength(0)
    call("requestClipboard", "bad-2", 5000, [])
    socket.receive("clipboard,@@")
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(requestKinds()).toEqual(["unreadable", "unreadable"])
    socket.receive(`clipboard,${b64("later")}`)
    await flush()
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it("falls back to the cached clipboard when only repeats arrive before the deadline", async () => {
    socket.receive(`clipboard,${b64("old")}`)
    call("requestClipboard", "stale-2", 60, ["REQUEST_CLIPBOARD"])
    socket.receive(`clipboard,${b64("old")}`)
    await flush()
    expect(fetchMock).not.toHaveBeenCalled()
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(body(posted()[0].init)).toBe("old")
  })

  it("answers at once when the first announcement differs from the cached one", async () => {
    socket.receive(`clipboard,${b64("old")}`)
    call("requestClipboard", "stale-3", 5000, [])
    socket.receive(`clipboard_binary,image/png,${b64("PNG")}`)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(posted()[0].url.searchParams.get("kind")).toBe("image/png")
  })

  it("reports a selection above the cap as too large instead of an older copy", async () => {
    socket.receive(`clipboard,${b64("older")}`)
    socket.receive(`clipboard_start,text/plain,${30 * MiB}`)
    call("requestClipboard", "big-1", 20, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(requestKinds()).toEqual(["too-large"])
    expect(posted()[0].init.body).toHaveLength(0)

    // An oversized announcement that arrives during the request answers at once.
    fetchMock.mockClear()
    socket.receive(`clipboard,${b64("older")}`)
    call("requestClipboard", "big-2", 5000, [])
    socket.receive(`clipboard_start,image/png,${30 * MiB}`)
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(requestKinds()).toEqual(["too-large"])

    // A later normal copy replaces the marker.
    fetchMock.mockClear()
    socket.receive(`clipboard,${b64("fresh")}`)
    call("requestClipboard", "big-3", 20, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(body(posted()[0].init)).toBe("fresh")
  })

  it("reports an oversized single-frame announcement too", async () => {
    socket.receive(`clipboard,${b64("older")}`)
    socket.receive(`clipboard,${"QUJD".repeat(9 * MiB)}`)
    call("requestClipboard", "big-4", 20, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(requestKinds()).toEqual(["too-large"])
  })

  it("never answers a clipboard request from the cache while disconnected", async () => {
    socket.receive(`clipboard,${b64("cached")}`)
    setTransport(null)
    call("requestClipboard", "gone-1", 20, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(requestKinds()).toEqual(["disconnected"])

    fetchMock.mockClear()
    const closing = new FakeSocket()
    setTransport(closing)
    closing.receive(`clipboard,${b64("cached")}`)
    call("requestClipboard", "gone-2", 40, ["REQUEST_CLIPBOARD"])
    closing.readyState = 3
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(requestKinds()).toEqual(["disconnected"])
    await flush()
    expect(fetchMock).toHaveBeenCalledOnce()
  })

  it("passes a Selkies flavours envelope through under its own kind", async () => {
    const envelope = JSON.stringify({ "text/html": "<b>hi</b>", "text/plain": "hi" })
    socket.receive(`clipboard_binary,application/x-selkies-clipboard-flavours,${b64(envelope)}`)
    call("requestClipboard", "flavours-1", 20, [])
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(requestKinds()).toEqual(["application/x-selkies-clipboard-flavours"])
    expect(body(posted()[0].init)).toBe(envelope)
  })

  it("reports the clipboard settings Selkies mirrored onto the page", async () => {
    const page = window as unknown as Record<string, unknown>
    try {
      page.clipboard_enabled = true
      page.clipboard_in_enabled = true
      page.clipboard_out_enabled = false
      call("capabilities", "caps-1")
      await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
      expect(JSON.parse(body(posted()[0].init))).toMatchObject({ transport: true, clipboard: true, clipboardIn: true, clipboardOut: false })
      fetchMock.mockClear()
      page.clipboard_enabled = "yes"
      call("capabilities", "caps-2")
      await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
      expect(JSON.parse(body(posted()[0].init))).toMatchObject({ clipboard: null })
    } finally {
      delete page.clipboard_enabled
      delete page.clipboard_in_enabled
      delete page.clipboard_out_enabled
    }
  })

  it("keeps working after the page tampers with the built-ins it relies on", () => {
    const test = RegExp.prototype.test
    const every = Array.prototype.every
    const apply = Reflect.apply
    try {
      RegExp.prototype.test = () => true
      Array.prototype.every = (() => true) as unknown as typeof Array.prototype.every
      Reflect.apply = () => true
      expect(call("sendFrames", ["cmd,rm -rf /"])).toBe(false)
      expect(call("sendFrames", ["kd,65"])).toBe(true)
      expect(socket.sent).toEqual(["kd,65"])
    } finally {
      RegExp.prototype.test = test
      Array.prototype.every = every
      Reflect.apply = apply
    }
  })
})
