import { afterEach, describe, expect, it, vi } from "vitest"

import contracts from "@/test/contracts/agents.json"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { OnboardingCompletionRequest } from "@/features/onboarding/model/onboarding-source"
import { productionOnboardingSource } from "./production-onboarding"
import { createProductionSource, type ProductionBridge } from "./production-source"

afterEach(() => { vi.useRealTimers() })

const application = applicationSourceForScenario("running")
const computer = application.computers[0].configuration
const request: OnboardingCompletionRequest = {
  agent: "codex",
  computerConfiguration: { schemaVersion: 1, computers: [computer] },
  applications: application.preferences,
  github: { connectionState: "disconnected", computers: [{ computer: computer.name, repositories: [], identity: { name: "Test", email: "test@example.invalid", apply: true } }] },
}
/** The committed state once the request's computer exists. */
const applied = { ...application, computers: [application.computers[0]] }
const missing = { agents: [{ id: "claude-code", name: "Claude Code", state: "missing", version: null, expectedVersion: "2.1.287" }, { id: "codex", name: "Codex", state: "missing", version: null, expectedVersion: "0.167.0" }] }

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

async function setup(handlers: Record<string, (args?: Record<string, unknown>) => unknown> = {}) {
  const events = new Map<string, (event?: { payload: unknown }) => void>()
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (Object.hasOwn(handlers, command)) return handlers[command](args)
    if (command === "read_application_state") return applied
    if (command === "read_backup_state") return { snapshotId: "test", availability: "available", archives: [], operation: null }
    if (command === "read_setup_activity") return []
    if (command === "change_computer_configuration" || command === "retry_computer_configuration") return applied
    if (command === "configure_computer_identities") return undefined
    if (command === "list_agents") return missing
    if (command === "install_agent") return contracts.installed
    throw new Error(`Unexpected command ${command}`)
  })
  const bridge = { invoke, listen: async (name: string, handler: (event?: { payload: unknown }) => void) => { events.set(name, handler); return () => events.delete(name) } } as ProductionBridge
  const store = createProductionSource(bridge)
  await store.initialize()
  const calls = (command: string) => invoke.mock.calls.filter(([name]) => name === command).map(([, args]) => args)
  return { store, invoke, calls, emit: (payload: unknown) => events.get("silo://agent-install-progress")?.({ payload }) }
}

const snapshotOf = (store: Awaited<ReturnType<typeof setup>>["store"]) => store.getSnapshot()
const agentItem = (store: Awaited<ReturnType<typeof setup>>["store"]) => snapshotOf(store).setupQueue.find(({ id }) => id === "agentInstall")

describe("installing the chosen agent during setup", () => {
  it("installs it in the first computer when the agent step continues, and shows the install's progress", async () => {
    const install = deferred<unknown>()
    const { store, calls, emit } = await setup({ install_agent: () => install.promise })
    await store.submitSetupStep("agent", request)
    await vi.waitFor(() => expect(calls("install_agent")).toEqual([{ computer: computer.name, agent: "codex" }]))
    expect(calls("list_agents")).toEqual([{ computer: computer.name }])
    expect(agentItem(store)?.status).toBe("running")
    expect(snapshotOf(store).setupAgent).toEqual({ computer: computer.name, agent: "codex" })
    emit({ computerId: computer.id, agent: "codex", step: "downloading", detail: "Downloading Codex (42%)" })
    expect(snapshotOf(store).setupAgent).toEqual({ computer: computer.name, agent: "codex", step: "downloading", detail: "Downloading Codex (42%)" })
    install.resolve(contracts.installed)
    await vi.waitFor(() => expect(agentItem(store)?.status).toBe("succeeded"))
    store.dispose()
  })

  it("shows the install on the setup surface by itself, so the items Finish waits for are unchanged", async () => {
    const install = deferred<unknown>()
    const { store, calls } = await setup({ install_agent: () => install.promise })
    await store.submitSetupStep("agent", request)
    await vi.waitFor(() => expect(calls("install_agent")).toHaveLength(1))
    const source = productionOnboardingSource(applied, { checks: [], retry: vi.fn() }, application.preferences, snapshotOf(store))
    expect(source.agentSetup).toEqual({ computer: computer.name, agent: "codex", status: "running" })
    expect(source.setupQueue?.map(({ id }) => id)).not.toContain("agentInstall")
    install.resolve(contracts.installed)
    await vi.waitFor(() => expect(agentItem(store)?.status).toBe("succeeded"))
    store.dispose()
  })

  it("does not install an agent the computer already has", async () => {
    const { store, calls } = await setup({ list_agents: () => contracts.installed })
    await store.submitSetupStep("agent", request)
    await vi.waitFor(() => expect(agentItem(store)?.status).toBe("succeeded"))
    expect(calls("install_agent")).toEqual([])
    store.dispose()
  })

  it("installs nothing, and queues nothing, when the person sets up their own agent or has not chosen", async () => {
    const { store, invoke } = await setup()
    await store.submitSetupStep("agent", { ...request, agent: "none" })
    await store.submitSetupStep("agent", { ...request, agent: undefined })
    expect(invoke.mock.calls.filter(([name]) => name === "list_agents" || name === "install_agent")).toEqual([])
    expect(snapshotOf(store).setupQueue.some(({ id }) => id === "agentInstall")).toBe(false)
    expect(snapshotOf(store).setupAgent).toBeUndefined()
    store.dispose()
  })

  it("queues the install once, however often the steps after it are submitted", async () => {
    const { store, calls } = await setup()
    await store.submitSetupStep("agent", request)
    await store.submitSetupStep("github", request)
    await store.submitSetupStep("github", request)
    await vi.waitFor(() => expect(agentItem(store)?.status).toBe("succeeded"))
    expect(calls("install_agent")).toHaveLength(1)
    store.dispose()
  })

  it("installs the agent on Finish when the agent step was skipped", async () => {
    const { store, calls } = await setup()
    const markComplete = vi.fn().mockResolvedValue(undefined)
    await store.finishSetup(request, markComplete)
    expect(calls("install_agent")).toEqual([{ computer: computer.name, agent: "codex" }])
    expect(markComplete).toHaveBeenCalledOnce()
    store.dispose()
  })

  it("does not fail setup when the install fails: Finish completes and the failure stays on the agent", async () => {
    const { store } = await setup({ install_agent: () => { throw new Error("The download could not be verified.") } })
    const markComplete = vi.fn().mockResolvedValue(undefined)
    await store.finishSetup(request, markComplete)
    expect(markComplete).toHaveBeenCalledOnce()
    expect(agentItem(store)).toMatchObject({ status: "failed", failure: "The download could not be verified." })
    const source = productionOnboardingSource(applied, { checks: [], retry: vi.fn() }, application.preferences, snapshotOf(store))
    expect(source.agentSetup).toMatchObject({ status: "failed", failure: "The download could not be verified." })
    expect(source.setupQueue?.every(({ status }) => status === "succeeded")).toBe(true)
    store.dispose()
  })

  it("installs again on retry and ends ready", async () => {
    let attempts = 0
    const { store, calls } = await setup({ install_agent: () => { if (++attempts === 1) throw new Error("No network."); return contracts.installed } })
    await store.submitSetupStep("agent", request)
    await vi.waitFor(() => expect(agentItem(store)?.status).toBe("failed"))
    store.retryAgentInstall()
    await vi.waitFor(() => expect(agentItem(store)?.status).toBe("succeeded"))
    expect(calls("install_agent")).toHaveLength(2)
    expect(agentItem(store)?.failure).toBeUndefined()
    store.dispose()
  })

  it("starts over when a different agent is chosen", async () => {
    const { store, calls } = await setup()
    await store.submitSetupStep("agent", request)
    await vi.waitFor(() => expect(agentItem(store)?.status).toBe("succeeded"))
    await store.submitSetupStep("agent", { ...request, agent: "claude-code" })
    await vi.waitFor(() => expect(calls("install_agent")).toHaveLength(2))
    expect(calls("install_agent").at(-1)).toEqual({ computer: computer.name, agent: "claude-code" })
    store.dispose()
  })
})
