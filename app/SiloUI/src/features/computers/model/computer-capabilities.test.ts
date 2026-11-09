import { describe, expect, it } from "vitest"
import { computerCan } from "./computer-capabilities"

describe("computerCan", () => {
  it("keeps checkpoints, forks and the terminal for a container and hides what it lacks", () => {
    const container = { runtime: "container" } as const
    expect(computerCan(container, "terminal")).toBe(true)
    expect(computerCan(container, "editor")).toBe(true)
    expect(computerCan(container, "sshAccess")).toBe(true)
    expect(computerCan(container, "export")).toBe(false)
    expect(computerCan(container, "desktop")).toBe(false)
    expect(computerCan(container, "memorySnapshots")).toBe(false)
  })

  it("offers a virtual machine everything", () => {
    for (const capability of ["terminal", "editor", "sshAccess", "export", "desktop", "memorySnapshots"] as const) {
      expect(computerCan({ runtime: undefined }, capability)).toBe(true)
    }
  })
})
