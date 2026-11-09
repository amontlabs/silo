import { describe, expect, it } from "vitest"
import { computerCan } from "./computer-capabilities"

describe("computerCan", () => {
  it("keeps everything for a container but the memory in its checkpoints", () => {
    const container = { runtime: "container" } as const
    expect(computerCan(container, "terminal")).toBe(true)
    expect(computerCan(container, "editor")).toBe(true)
    expect(computerCan(container, "sshAccess")).toBe(true)
    expect(computerCan(container, "desktop")).toBe(true)
    expect(computerCan(container, "memorySnapshots")).toBe(false)
  })

  it("offers a virtual machine everything", () => {
    for (const capability of ["terminal", "editor", "sshAccess", "desktop", "memorySnapshots"] as const) {
      expect(computerCan({ runtime: undefined }, capability)).toBe(true)
    }
  })
})
