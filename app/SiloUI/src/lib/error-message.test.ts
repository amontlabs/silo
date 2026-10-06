import { describe, expect, it } from "vitest"

import { errorMessage } from "@/lib/error-message"

describe("errorMessage", () => {
  it("prefers a native bridge error's message", () => {
    expect(errorMessage({ code: "busy", message: "Computer is busy." })).toBe("Computer is busy.")
  })

  it("reads Error messages and stringifies other values", () => {
    expect(errorMessage(new Error("boom"))).toBe("boom")
    expect(errorMessage("plain")).toBe("plain")
    expect(errorMessage(null)).toBe("null")
  })

  it("uses the fallback for blank or unreadable values", () => {
    const fallback = "Could not do it."
    expect(errorMessage(new Error("  "), { fallback })).toBe(fallback)
    expect(errorMessage("", { fallback })).toBe(fallback)
    expect(errorMessage({ unexpected: true }, { fallback })).toBe(fallback)
    expect(errorMessage(undefined, { fallback })).toBe(fallback)
  })

  it("keeps readable text when a fallback is given", () => {
    expect(errorMessage(new Error(" boom "), { fallback: "x" })).toBe("boom")
    expect(errorMessage("text", { fallback: "x" })).toBe("text")
    expect(errorMessage({ code: "x", message: "bridge" }, { fallback: "y" })).toBe("bridge")
  })
})
