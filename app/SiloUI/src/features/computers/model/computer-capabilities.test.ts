import { describe, expect, it } from "vitest"
import { computerCan, kindOf, workspaceRoot, type ComputerCapability, type ComputerKind } from "./computer-capabilities"

const capabilities: ComputerCapability[] = ["terminal", "editor", "sshAccess", "desktop", "memorySnapshots", "restart", "storage", "files", "ports", "secrets", "github", "checkpoints", "fork"]

function offered(kind: ComputerKind) {
  return capabilities.filter(capability => computerCan(kind, capability))
}

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
    for (const capability of capabilities) {
      expect(computerCan({ runtime: undefined }, capability)).toBe(true)
    }
  })

  it("offers a macOS computer of this device what the engine serves it", () => {
    expect(offered({ os: "macos" })).toEqual(["terminal", "editor", "desktop", "memorySnapshots", "files", "ports", "secrets", "github", "checkpoints"])
  })

  it("offers a macOS computer of another device only what its owner serves", () => {
    expect(offered({ os: "macos", remote: true })).toEqual(["desktop"])
  })

  it("leaves a Linux computer on another device as it is", () => {
    expect(offered({ remote: true })).toEqual(capabilities)
  })

  it("reads the kind of an application computer", () => {
    expect(kindOf({ configuration: { runtime: "container" }, device: {} })).toEqual({ runtime: "container", os: undefined, remote: true })
    expect(kindOf({ configuration: {}, os: "macos" })).toEqual({ runtime: undefined, os: "macos", remote: false })
  })

  it("keeps a macOS computer's files in the working account's workspace", () => {
    expect(workspaceRoot({ os: "macos" })).toBe("/Users/silo/workspace")
    expect(workspaceRoot({})).toBe("/workspace")
  })
})
