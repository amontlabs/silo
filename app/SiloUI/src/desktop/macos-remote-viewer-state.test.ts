import { describe, expect, it } from "vitest"
import { isTransientSessionError, macosDisplayRoute, reconnectDelay, resizeTarget, statusLabel } from "./macos-remote-viewer-state"

describe("macosDisplayRoute", () => {
  it("reads the computer, device and name", () => {
    expect(macosDisplayRoute("?macosDisplay=mac-1&device=dev-2&name=Build%20Mac")).toEqual({ computer: "mac-1", device: "dev-2", name: "Build Mac" })
  })
  it("defaults the name and device", () => {
    expect(macosDisplayRoute("?macosDisplay=mac-1")).toEqual({ computer: "mac-1", device: null, name: "mac-1" })
  })
  it("is null without the display parameter", () => {
    expect(macosDisplayRoute("?desktop=x")).toBeNull()
    expect(macosDisplayRoute("")).toBeNull()
  })
})

describe("reconnectDelay", () => {
  it("doubles from one second up to fifteen", () => {
    expect([1, 2, 3, 4, 5, 6, 7, 8].map(reconnectDelay)).toEqual([1000, 2000, 4000, 8000, 15000, 15000, 15000, 15000])
  })
})

describe("isTransientSessionError", () => {
  it("retries only owner and network failures", () => {
    expect(isTransientSessionError("The device that owns this computer is offline")).toBe(true)
    expect(isTransientSessionError("The computer is not running")).toBe(false)
    expect(isTransientSessionError("Setup has not finished")).toBe(false)
    expect(isTransientSessionError("Screen Sharing is unavailable")).toBe(false)
  })
})

describe("resizeTarget", () => {
  it("scales by the pixel ratio and rounds down to even", () => {
    expect(resizeTarget(1001, 700, 2)).toEqual({ widthPx: 2002, heightPx: 1400 })
    expect(resizeTarget(1000.5, 700.2, 1.5)).toEqual({ widthPx: 1500, heightPx: 1050 })
    expect(resizeTarget(801, 601, 1)).toEqual({ widthPx: 800, heightPx: 600 })
  })
  it("ignores tiny sizes", () => {
    expect(resizeTarget(600, 800, 1)).toBeNull()
    expect(resizeTarget(900, 300, 1)).toBeNull()
  })
})

describe("statusLabel", () => {
  it("describes the reconnect attempt and delay", () => {
    expect(statusLabel({ phase: "reconnecting", attempt: 3, delayMs: 4000 })).toContain("attempt 3, in 4s")
  })
})
