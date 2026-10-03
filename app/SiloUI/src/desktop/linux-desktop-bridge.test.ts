import { readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest"

// The page half of the host bridge (src-tauri/src/desktop_viewer_bridge.js),
// run against a fake Selkies socket the way Rust drives it through eval.
const native = resolve(dirname(fileURLToPath(import.meta.url)), "../../src-tauri")
const script = readFileSync(resolve(native, "src/desktop_viewer_bridge.js"), "utf8")

class FakeSocket {
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
    const closed = new FakeSocket() as FakeSocket & { readyState: number }
    closed.readyState = 3
    setTransport(closed)
    expect(call("sendFrames", ["REQUEST_CLIPBOARD"])).toBe(false)
    expect(closed.sent).toEqual([])
    vi.stubGlobal("AudioDecoder", undefined)
    call("capabilities", "nonce-0")
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    expect(JSON.parse(body(posted()[0].init))).toEqual({ audioDecoder: false, opus: false, transport: false })
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
    expect(JSON.parse(body(request.init))).toEqual({ audioDecoder: true, opus: true, transport: true })
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
})
