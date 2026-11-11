import { afterEach, describe, expect, it, vi } from "vitest"

import contracts from "@/test/contracts/network-policy.json"
import { assertNativeBridgeMocksHandled, nativeBridgeMock, type NativeCommandHandlers } from "@/test/native-bridge-mock"

import { computerToolsSchema, networkPolicyStateSchema } from "./production-schemas"
import { createProductionSource } from "./production-source"
import type { ProductionBridge } from "./production-types"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"

afterEach(assertNativeBridgeMocksHandled)

function bridge(handlers: NativeCommandHandlers) {
  const invoke = nativeBridgeMock({
    read_application_state: () => structuredClone(applicationSourceForScenario("running")),
    read_backup_state: () => ({ snapshotId: "1", availability: "available", archives: [], operation: null }),
    read_setup_activity: () => [],
    read_network_state: () => ({ computers: [] }),
    remote_network_state: () => ({ computers: [] }),
    device_list: () => [],
    connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "developer@laptop" }),
    read_operation_queue: () => ({ running: [], waiting: [] }),
    ...handlers,
  })
  return { invoke, listen: vi.fn(async () => () => {}) } as unknown as ProductionBridge & { invoke: typeof invoke }
}

describe("network policy contract", () => {
  it("reads each mode, the presets, what Silo added, an update waiting and a notice", () => {
    const open = networkPolicyStateSchema.parse(contracts.open)
    expect(open).toMatchObject({ policy: { mode: "open" }, timing: "applied", allowedHosts: [], presetUpdates: [], notices: [] })
    expect(open.presets.map(({ name }) => name)).toEqual(["npm", "pypi", "anthropic"])
    const allowlist = networkPolicyStateSchema.parse(contracts.allowlist)
    expect(allowlist.policy).toEqual({ mode: "allowlist", hosts: ["example.com", "*.internal.example.org"], presets: ["npm", "pypi"] })
    expect(allowlist.timing).toBe("nextRestart")
    expect(allowlist.allowedHosts.map(({ from }) => from)).toContain("secret:stripe-key")
    expect(allowlist.presetUpdates).toEqual([{ name: "npm", title: "npm packages", addedHosts: ["registry.npmmirror.com", "npm.pkg.example"] }])
    expect(allowlist.notices).toHaveLength(1)
    expect(networkPolicyStateSchema.parse(contracts.closed)).toMatchObject({ policy: { mode: "closed" }, timing: "nextStart" })
  })

  it("reads a timing it does not know as the cautious one and an unknown mode as unreadable", () => {
    expect(networkPolicyStateSchema.parse({ ...contracts.open, timing: "later" }).timing).toBe("nextRestart")
    expect(() => networkPolicyStateSchema.parse({ ...contracts.open, policy: { mode: "everything" } })).toThrow()
  })

  it("reads whether the tools are on, the restart they wait for and the pieces that failed, and never the port or the token", () => {
    expect(computerToolsSchema.parse(contracts.toolsOff)).toMatchObject({ enabled: false, restartNeeded: false, components: [] })
    const on = computerToolsSchema.parse(contracts.toolsOn)
    expect(on).toMatchObject({ enabled: true, network: "nextStart", restartNeeded: false })
    expect(Object.keys(on)).not.toContain("port")
    expect(JSON.stringify(on)).not.toMatch(/41234|token|endpoint/i)
    expect(computerToolsSchema.parse(contracts.toolsRestart)).toMatchObject({ network: "nextRestart", restartNeeded: true })
    const components = computerToolsSchema.parse(contracts.toolsComponents).components
    expect(components.map(({ id, state }) => [id, state])).toEqual([["silo", "ok"], ["silo-mcp", "failed"], ["future-piece", "failed"]])
  })
})

describe("network policy commands", () => {
  it("reads and saves a policy by computer name, sending the one the person saw", async () => {
    const native = bridge({ read_network_policy: () => contracts.allowlist, save_network_policy: () => contracts.closed })
    const store = createProductionSource(native)
    const actions = store.applicationActions
    expect((await actions.readNetworkPolicy!("main")).policy.mode).toBe("allowlist")
    const seen = { mode: "allowlist" as const, hosts: ["example.com"], presets: ["npm"] }
    const wanted = { mode: "closed" as const, hosts: [], presets: [] }
    expect((await actions.saveNetworkPolicy!("main", wanted, seen)).policy.mode).toBe("closed")
    await actions.saveNetworkPolicy!("main", wanted)
    expect(native.invoke.mock.calls.filter(([name]) => String(name).endsWith("_network_policy"))).toEqual([
      ["read_network_policy", { computer: "main" }],
      ["save_network_policy", { computer: "main", policy: wanted, expected: seen }],
      ["save_network_policy", { computer: "main", policy: wanted }],
    ])
  })

  it("passes the engine's refusal of a stale save on in its own words", async () => {
    const native = bridge({ save_network_policy: () => { throw contracts.staleSave.message } })
    const store = createProductionSource(native)
    const actions = store.applicationActions
    await expect(actions.saveNetworkPolicy!("main", { mode: "open", hosts: [], presets: [] }, { mode: "closed", hosts: [], presets: [] })).rejects.toMatch(/Read it again and retry/)
  })

  it("turns Silo's tools on and off through their own commands", async () => {
    const native = bridge({ read_computer_tools: () => contracts.toolsOff, enable_computer_tools: () => contracts.toolsOn, revoke_computer_tools: () => contracts.toolsOff })
    const store = createProductionSource(native)
    const actions = store.applicationActions
    expect((await actions.readComputerTools!("main")).enabled).toBe(false)
    expect((await actions.setComputerTools!("main", true)).enabled).toBe(true)
    expect((await actions.setComputerTools!("main", false)).enabled).toBe(false)
    expect(native.invoke.mock.calls.filter(([name]) => String(name).includes("computer_tools"))).toEqual([
      ["read_computer_tools", { computer: "main" }],
      ["enable_computer_tools", { computer: "main" }],
      ["revoke_computer_tools", { computer: "main" }],
    ])
  })
})
