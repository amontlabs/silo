import { afterEach, describe, expect, it, vi } from "vitest"

import contracts from "@/test/contracts/agents.json"
import { assertNativeBridgeMocksHandled, nativeBridgeMock } from "@/test/native-bridge-mock"

import { ensureSshName, installAgent, listAgents } from "./production-agents"
import type { ProductionBridge } from "./production-types"

afterEach(assertNativeBridgeMocksHandled)

type Listener = (event?: { payload: unknown }) => void

function bridge(handlers: Parameters<typeof nativeBridgeMock>[0]) {
  const listeners = new Map<string, Listener>()
  const stop = vi.fn()
  const invoke = nativeBridgeMock(handlers)
  const listen = vi.fn(async (event: string, handler: Listener) => { listeners.set(event, handler); return stop })
  return { native: { invoke, listen } as unknown as ProductionBridge, invoke, listen, stop, emit: (event: string, payload: unknown) => listeners.get(event)?.({ payload }) }
}

describe("production agents", () => {
  it("lists the agents of a computer", async () => {
    const { native, invoke } = bridge({ list_agents: () => contracts.list })
    const agents = await listAgents(native, "main")
    expect(agents.map(({ id, state, version }) => [id, state, version])).toEqual([["claude-code", "ready", "2.1.287"], ["codex", "missing", undefined]])
    expect(invoke).toHaveBeenCalledWith("list_agents", { computer: "main" })
  })

  it("installs an agent, passes on only its own progress, and stops listening afterwards", async () => {
    const seen: unknown[] = []
    const { native, invoke, listen, stop, emit } = bridge({
      install_agent: () => {
        emit("silo://agent-install-progress", contracts.progress)
        emit("silo://agent-install-progress", { ...contracts.progress, agent: "claude-code" })
        emit("silo://agent-install-progress", { ...contracts.progress, computerId: "other-1" })
        emit("silo://agent-install-progress", { nonsense: true })
        return contracts.installed
      },
    })
    const agents = await installAgent(native, "main", "main-1", "codex", progress => seen.push(progress))
    expect(agents.every(({ state }) => state === "ready")).toBe(true)
    expect(invoke).toHaveBeenCalledWith("install_agent", { computer: "main", agent: "codex" })
    expect(seen).toEqual([{ computerId: "main-1", agent: "codex", step: "downloading", detail: "Downloading Codex (42%)" }])
    expect(listen).toHaveBeenCalledTimes(1)
    expect(stop).toHaveBeenCalledTimes(1)
  })

  it("still stops listening when the install fails, and passes the error on", async () => {
    const { native, stop } = bridge({ install_agent: () => { throw new Error("The download could not be verified.") } })
    await expect(installAgent(native, "main", "main-1", "codex", () => {})).rejects.toThrow("The download could not be verified.")
    expect(stop).toHaveBeenCalledTimes(1)
  })

  it("listens for nothing when nobody wants the progress", async () => {
    const { native, listen } = bridge({ install_agent: () => contracts.installed })
    await installAgent(native, "main", "main-1", "claude-code")
    expect(listen).not.toHaveBeenCalled()
  })

  it("reads the SSH name of a computer", async () => {
    const { native, invoke } = bridge({ ensure_ssh_name: () => contracts.sshName })
    expect(await ensureSshName(native, "main")).toMatchObject({ alias: contracts.sshName.alias, includeInstalled: true })
    expect(invoke).toHaveBeenCalledWith("ensure_ssh_name", { computer: "main" })
  })
})
