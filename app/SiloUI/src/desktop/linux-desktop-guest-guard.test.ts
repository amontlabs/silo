import { readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { beforeAll, describe, expect, it, vi } from "vitest"

// The script Silo injects into every frame of a guest desktop webview (G-20).
// It locks what it guards, so it runs once, as it does in a real frame.
const native = resolve(dirname(fileURLToPath(import.meta.url)), "../../src-tauri")
const guard = readFileSync(resolve(native, "src/desktop_viewer_bridge.js"), "utf8")

class FakeClipboard {
  writeText = vi.fn(async () => {})
  readText = vi.fn(async () => "host secret")
  write = vi.fn(async () => {})
  read = vi.fn(async () => [])
}
const clipboard = new FakeClipboard()
const original = { ...clipboard }
const execCommand = vi.fn(() => true)
class FakeMediaDevices {
  getUserMedia = vi.fn(async () => ({}))
  getDisplayMedia = vi.fn(async () => ({}))
}
const mediaDevices = new FakeMediaDevices()
const originalCapture = { ...mediaDevices }

beforeAll(() => {
  vi.stubGlobal("MediaDevices", FakeMediaDevices)
  Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: mediaDevices })
  vi.stubGlobal("Clipboard", FakeClipboard)
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: clipboard })
  Object.defineProperty(Document.prototype, "execCommand", { configurable: true, writable: true, value: execCommand })
  window.eval(guard)
})

describe("guest desktop clipboard guard", () => {
  it("refuses clipboard reads and writes before guest scripts run", async () => {
    await expect(navigator.clipboard.writeText("payload")).rejects.toMatchObject({ name: "NotAllowedError" })
    await expect(navigator.clipboard.readText()).rejects.toMatchObject({ name: "NotAllowedError" })
    await expect(navigator.clipboard.write([])).rejects.toMatchObject({ name: "NotAllowedError" })
    await expect(navigator.clipboard.read()).rejects.toMatchObject({ name: "NotAllowedError" })
    expect(original.writeText).not.toHaveBeenCalled()
    expect(original.readText).not.toHaveBeenCalled()
    // A guest script cannot put the originals back.
    expect(() => { Object.defineProperty(navigator.clipboard, "writeText", { value: original.writeText }) }).toThrow()
    expect(() => { Object.defineProperty(FakeClipboard.prototype, "writeText", { value: original.writeText }) }).toThrow()
  })

  it("stops page-driven copy while leaving other editing commands alone", () => {
    expect(document.execCommand("copy")).toBe(false)
    expect(document.execCommand("CUT")).toBe(false)
    expect(document.execCommand("paste")).toBe(false)
    expect(execCommand).not.toHaveBeenCalled()
    expect(document.execCommand("bold")).toBe(true)
    expect(execCommand).toHaveBeenCalledOnce()
    expect(() => { Object.defineProperty(Document.prototype, "execCommand", { value: execCommand }) }).toThrow()
  })

  it("converts the command once, so a stateful object cannot become copy after validation", () => {
    execCommand.mockClear()
    let conversions = 0
    const sneaky = { toString: () => (conversions++ === 0 ? "bold" : "copy") }
    document.execCommand(sneaky as unknown as string)
    expect(conversions).toBe(1)
    expect(execCommand).toHaveBeenCalledOnce()
    expect(execCommand).toHaveBeenCalledWith("bold", undefined, undefined)
    execCommand.mockClear()
    const sequence = ["x", "COPY"]
    const alternating = { toString: () => sequence.shift() ?? "paste" }
    expect(document.execCommand(alternating as unknown as string)).toBe(true)
    expect(execCommand).toHaveBeenCalledWith("x", undefined, undefined)
    execCommand.mockClear()
    expect(document.execCommand({ toString: () => "Copy" } as unknown as string)).toBe(false)
    expect(execCommand).not.toHaveBeenCalled()
  })

  it("keeps denying copy after the page tampers with the built-ins it relies on", () => {
    execCommand.mockClear()
    const lower = String.prototype.toLowerCase
    const apply = Reflect.apply
    const call = Function.prototype.call
    const test = RegExp.prototype.test
    const results: boolean[] = []
    try {
      String.prototype.toLowerCase = function () { return "bold" }
      Reflect.apply = () => true
      Function.prototype.call = () => true
      RegExp.prototype.test = () => false
      results.push(document.execCommand("copy"), document.execCommand("paste"))
    } finally {
      String.prototype.toLowerCase = lower
      Reflect.apply = apply
      Function.prototype.call = call
      RegExp.prototype.test = test
    }
    expect(results).toEqual([false, false])
    expect(execCommand).not.toHaveBeenCalled()
  })

  it("keeps guest handlers from replacing copied data but lets pasting through", () => {
    const handler = vi.fn()
    document.addEventListener("copy", handler)
    document.addEventListener("cut", handler)
    document.dispatchEvent(new Event("copy", { bubbles: true }))
    document.dispatchEvent(new Event("cut", { bubbles: true }))
    expect(handler).not.toHaveBeenCalled()
    const paste = vi.fn()
    document.addEventListener("paste", paste)
    document.dispatchEvent(new Event("paste", { bubbles: true }))
    expect(paste).toHaveBeenCalledOnce()
  })

  it("always refuses microphone and screen capture, and a page cannot unlock them", async () => {
    await expect(navigator.mediaDevices.getUserMedia({ audio: true })).rejects.toMatchObject({ name: "NotAllowedError" })
    await expect(navigator.mediaDevices.getDisplayMedia({ video: true })).rejects.toMatchObject({ name: "NotAllowedError" })
    await expect((FakeMediaDevices.prototype as unknown as { getUserMedia: (...args: unknown[]) => Promise<unknown> }).getUserMedia.call(mediaDevices, { audio: true })).rejects.toMatchObject({ name: "NotAllowedError" })
    expect(originalCapture.getUserMedia).not.toHaveBeenCalled()
    expect(() => { Object.defineProperty(navigator.mediaDevices, "getUserMedia", { value: originalCapture.getUserMedia }) }).toThrow()
    expect(() => { Object.defineProperty(FakeMediaDevices.prototype, "getUserMedia", { value: originalCapture.getUserMedia }) }).toThrow()
    const failure = vi.fn()
    ;(navigator as unknown as { webkitGetUserMedia: (...args: unknown[]) => void }).webkitGetUserMedia({ audio: true }, vi.fn(), failure)
    await vi.waitFor(() => expect(failure).toHaveBeenCalledOnce())
  })

  it("seeds the Selkies setting that keeps the page from writing the clipboard on its own", () => {
    const prefix = `${location.origin}${location.pathname}`.replace(/[^a-zA-Z0-9._-]/g, "_")
    expect(window.localStorage.getItem(`${prefix}_clipboard_seamless`)).toBe("false")
  })
})
