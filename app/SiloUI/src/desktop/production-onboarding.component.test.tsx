import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"
import type { OnboardingAppProps } from "@/features/onboarding/onboarding-app"
import { onboardingScenarios } from "@/fixtures/scenarios"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { OnboardingCompletionRequest } from "@/features/onboarding/model/onboarding-source"
import type { SetupComputerConfigurationRequest } from "@/contracts/silo"
import type { ProductionSource } from "./production-source"
import { projectOnboarding } from "@/features/onboarding/model/onboarding-state"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import { ProductionOnboarding } from "./production-onboarding"

const captured = vi.hoisted(() => ({ props: null as OnboardingAppProps | null }))
vi.mock("@/features/onboarding/onboarding-app", () => ({ OnboardingApp: (props: OnboardingAppProps) => { captured.props = props; return <div>{props.source.error?.message ?? "No error"}</div> } }))
vi.mock("./production-source", async (original) => ({ ...await original<typeof import("./production-source")>(), useProductionSource: () => ({ setupQueue: [], setupEvents: [], setupCandidate: null, backup: { state: {} } }) }))

function deferred() {
  let resolve!: () => void
  let reject!: (error: Error) => void
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
const application = applicationSourceForScenario("running")
const requestA: SetupComputerConfigurationRequest = { schemaVersion: 1, computers: application.computers.map(({ configuration }) => configuration) }
const requestB: SetupComputerConfigurationRequest = { ...requestA, computers: [requestA.computers[0]] }
// requestB drops the other computers: the list's own Delete confirmation confirmed that.
const dropped = { confirmedDeletions: requestA.computers.slice(1).map(({ id }) => id) }
const dependencies = { checks: [], retry: vi.fn() }

describe("production onboarding submission errors", () => {
  it("passes the GitHub token secrets to onboarding independently of OAuth", () => {
    const source = { applicationActions: {} } as unknown as ProductionSource
    const secret = (id: string, name: string, connected: boolean, removing = false) => ({
      id, name, kind: "github-token" as const, computers: [], allowedDomains: [], state: "active" as const, account: "alice", removing,
      connection: { state: connected ? "connected" as const : "disconnected" as const },
    })
    const withTokens = { ...application, secrets: [
      secret("t1", "work", true), secret("t2", "home", false), secret("t3", "gone", true, true),
      { id: "g1", name: "API_KEY", computers: ["dev"], allowedDomains: ["example.com"], state: "active" as const },
    ] }
    const view = render(<ProductionOnboarding application={withTokens} dependencies={dependencies} source={source} />)
    expect(captured.props!.tokenSecrets).toEqual([
      { id: "t1", name: "work", account: "alice", connected: true },
      { id: "t2", name: "home", account: "alice", connected: false },
    ])
    view.rerender(<ProductionOnboarding application={{ ...application, secrets: [] }} dependencies={dependencies} source={source} />)
    expect(captured.props!.tokenSecrets).toEqual([])
  })

  it("passes an intentionally empty restored draft to identity verification", () => {
    const store = createMemorySettingsStore({}, { currentStep: "review", computers: [], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} })
    const verifySetupIdentities = vi.fn().mockResolvedValue(undefined)
    const source = { verifySetupIdentities, applicationActions: {} } as unknown as ProductionSource
    render(<SettingsProvider store={store}><ProductionOnboarding application={application} dependencies={dependencies} source={source} /></SettingsProvider>)
    expect(verifySetupIdentities).toHaveBeenCalledWith({ computerConfiguration: { schemaVersion: 1, computers: [] }, github: { connectionState: application.github.state, computers: [] } })
  })

  it("verifies an omitted identity for a recovered computer named constructor as unapplied", () => {
    const configuration = { ...requestB.computers[0], name: "constructor" }
    const store = createMemorySettingsStore({}, { currentStep: "review", computers: [configuration], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} })
    const verifySetupIdentities = vi.fn().mockResolvedValue(undefined)
    const source = { verifySetupIdentities, applicationActions: {} } as unknown as ProductionSource
    render(<SettingsProvider store={store}><ProductionOnboarding application={application} dependencies={dependencies} source={source} /></SettingsProvider>)

    expect(verifySetupIdentities).toHaveBeenCalledWith({
      computerConfiguration: { schemaVersion: 1, computers: [configuration] },
      github: { connectionState: application.github.state, computers: [{ computer: "constructor", repositories: [], identity: { name: "", email: "", apply: false } }] },
    })
  })

  it("ignores an older rejection while the newer submission succeeds", async () => {
    const first = deferred()
    const second = deferred()
    const configureConfigurations = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const source = { configureConfigurations, applicationActions: {} } as unknown as ProductionSource
    render(<ProductionOnboarding application={application} dependencies={dependencies} source={source} />)
    act(() => captured.props!.actions.saveComputerConfiguration(requestA))
    act(() => captured.props!.actions.saveComputerConfiguration(requestB, dropped))
    await act(async () => { first.reject(new Error("Old failure")); await first.promise.catch(() => {}) })
    expect(screen.getByText("No error")).toBeVisible()
    await act(async () => { second.resolve(); await second.promise })
    expect(screen.getByText("No error")).toBeVisible()
  })

  it("keeps the failed computer identity when the local promise also rejects", async () => {
    const failed = deferred()
    const source = { configureConfigurations: vi.fn(() => failed.promise), applicationActions: {} } as unknown as ProductionSource
    const error = { code: "native_bridge_failed", message: "Creation failed", recovery: "Retry", computer: requestB.computers[0].name, retryable: true }
    const current = { ...application, computerConfigurationOperation: { id: "failed", status: "failed", candidate: requestB, progressEvents: [], result: null, error } } as typeof application
    render(<ProductionOnboarding application={current} dependencies={dependencies} source={source} />)
    act(() => captured.props!.actions.saveComputerConfiguration(requestB, dropped))
    await act(async () => { failed.reject(new Error("Creation failed")); await failed.promise.catch(() => {}) })
    expect(captured.props!.source.error?.computer).toBe(requestB.computers[0].name)
    const progress = projectOnboarding({ ...captured.props!.source, progressEvents: [{ ...onboardingScenarios.running.progressEvents[0], computer: requestB.computers[0].name, step: "computer-configuration", fraction: 0 }] }, "disconnected").computerProgress
    expect(progress.computers[0]).toMatchObject({ name: requestB.computers[0].name, status: "failed", detail: "Creation failed" })
    expect(progress.workingCount).toBe(0)
  })

  it("retries the newest submitted configuration before an older native candidate", async () => {
    const configureConfigurations = vi.fn().mockResolvedValue(application)
    const source = { configureConfigurations, applicationActions: {} } as unknown as ProductionSource
    const older = { ...application, computerConfigurationOperation: { id: "old", status: "applying", candidate: requestA, progressEvents: [], result: null, error: null } } as typeof application
    render(<ProductionOnboarding application={older} dependencies={dependencies} source={source} />)
    await act(async () => { captured.props!.actions.saveComputerConfiguration(requestB, dropped); await Promise.resolve() })
    await act(async () => { captured.props!.actions.retryComputerSetup(); await Promise.resolve() })
    expect(configureConfigurations).toHaveBeenLastCalledWith(requestB)
  })

  it("retries the submitted Git identity step instead of computer configuration", async () => {
    const request: OnboardingCompletionRequest = { computerConfiguration: requestA, applications: application.preferences, github: { connectionState: "disconnected", computers: [] } }
    const submitSetupStep = vi.fn().mockRejectedValueOnce(new Error("Identity verification failed")).mockResolvedValue(undefined)
    const configureConfigurations = vi.fn()
    const source = { submitSetupStep, configureConfigurations, applicationActions: {} } as unknown as ProductionSource
    render(<ProductionOnboarding application={application} dependencies={dependencies} source={source} />)
    await act(async () => { captured.props!.actions.submitStep!("github", request) })
    expect(screen.getByText("Identity verification failed")).toBeVisible()
    await act(async () => { captured.props!.actions.retryComputerSetup() })
    expect(submitSetupStep).toHaveBeenCalledTimes(2)
    expect(submitSetupStep).toHaveBeenLastCalledWith("github", request)
    expect(configureConfigurations).not.toHaveBeenCalled()
    expect(screen.getByText("No error")).toBeVisible()
  })

  it("retries Finish and shows completion only after the settings callback succeeds", async () => {
    const request: OnboardingCompletionRequest = { computerConfiguration: requestA, applications: application.preferences, github: { connectionState: "disconnected", computers: [] } }
    const finishSetup = vi.fn().mockRejectedValueOnce(new Error("Settings write failed")).mockImplementation(async (_request, save: () => Promise<void>) => save())
    const configureConfigurations = vi.fn()
    const onOpenApp = vi.fn()
    const source = { finishSetup, configureConfigurations, applicationActions: {} } as unknown as ProductionSource
    render(<ProductionOnboarding application={application} dependencies={dependencies} source={source} onOpenApp={onOpenApp} />)
    await act(async () => { captured.props!.actions.finishSetup(request) })
    expect(screen.getByText("Settings write failed")).toBeVisible()
    expect(captured.props!.completed).toBe(false)
    await act(async () => { captured.props!.actions.retryComputerSetup() })
    expect(finishSetup).toHaveBeenCalledTimes(2)
    expect(finishSetup).toHaveBeenLastCalledWith(request, expect.any(Function))
    expect(configureConfigurations).not.toHaveBeenCalled()
    expect(screen.getByText("No error")).toBeVisible()
    expect(captured.props!.completed).toBe(true)
    expect(captured.props!.onOpenApp).toBe(onOpenApp)
  })


  it("never sends a delete for an existing computer the user did not confirm deleting", async () => {
    const configureConfigurations = vi.fn().mockResolvedValue(application)
    const submitSetupStep = vi.fn().mockResolvedValue(undefined)
    const finishSetup = vi.fn().mockResolvedValue(undefined)
    // The committed list is read from the source when submitting, not from a stale prop.
    const source = { configureConfigurations, submitSetupStep, finishSetup, getSnapshot: () => ({ source: application }), applicationActions: {} } as unknown as ProductionSource
    render(<ProductionOnboarding application={null} dependencies={dependencies} source={source} />)
    const request: OnboardingCompletionRequest = { computerConfiguration: requestB, applications: application.preferences, github: { connectionState: "disconnected", computers: [] } }
    await act(async () => { captured.props!.actions.saveComputerConfiguration(requestB) })
    expect(screen.getByText(/Setup did not delete .*No computer changed\./)).toBeVisible()
    await act(async () => { captured.props!.actions.submitStep!("computers", request) })
    await act(async () => { captured.props!.actions.finishSetup(request) })
    await act(async () => { captured.props!.actions.retryComputerSetup(request) })
    expect(configureConfigurations).not.toHaveBeenCalled()
    expect(submitSetupStep).not.toHaveBeenCalled()
    expect(finishSetup).not.toHaveBeenCalled()
    await act(async () => { captured.props!.actions.submitStep!("computers", request, dropped) })
    expect(submitSetupStep).toHaveBeenCalledWith("computers", request)
  })

  it("retries a failed step with the current draft instead of the failed request", async () => {
    const failed: OnboardingCompletionRequest = { computerConfiguration: requestA, applications: application.preferences, github: { connectionState: "disconnected", computers: [{ computer: requestA.computers[0].name, repositories: [], identity: { name: "Old", email: "old@example.invalid", apply: true } }] } }
    const edited: OnboardingCompletionRequest = { ...failed, github: { ...failed.github, computers: [{ ...failed.github.computers[0], identity: { name: "New", email: "new@example.invalid", apply: true } }] } }
    const submitSetupStep = vi.fn().mockRejectedValueOnce(new Error("Identity verification failed")).mockResolvedValue(undefined)
    const source = { submitSetupStep, applicationActions: {} } as unknown as ProductionSource
    render(<ProductionOnboarding application={application} dependencies={dependencies} source={source} />)
    await act(async () => { captured.props!.actions.submitStep!("github", failed) })
    await act(async () => { captured.props!.actions.retryComputerSetup(edited) })
    expect(submitSetupStep).toHaveBeenLastCalledWith("github", edited)
    // A configuration failure retried from the current draft uses its computers.
    const configureConfigurations = vi.fn().mockRejectedValueOnce(new Error("Creation failed")).mockResolvedValue(application)
    Object.assign(source, { configureConfigurations })
    await act(async () => { captured.props!.actions.saveComputerConfiguration(requestA) })
    await act(async () => { captured.props!.actions.retryComputerSetup({ ...edited, computerConfiguration: requestB }, dropped) })
    expect(configureConfigurations).toHaveBeenLastCalledWith(requestB)
  })

  it("verifies the saved draft identity when onboarding is restored", async () => {
    const configuration = requestB.computers[0]
    const identity = { name: "Saved author", email: "saved@example.invalid", apply: true }
    const draft = { currentStep: "review" as const, computers: requestB.computers, unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: { [configuration.name]: identity } }
    const store = createMemorySettingsStore({}, draft)
    const verifySetupIdentities = vi.fn().mockResolvedValue(undefined)
    const source = { verifySetupIdentities, applicationActions: {} } as unknown as ProductionSource
    render(<SettingsProvider store={store}><ProductionOnboarding application={application} dependencies={dependencies} source={source} /></SettingsProvider>)
    expect(verifySetupIdentities).toHaveBeenCalledWith({ computerConfiguration: requestB, github: { connectionState: application.github.state, computers: [{ computer: configuration.name, repositories: [], identity }] } })
    const changed = { ...identity, email: "changed@example.invalid" }
    await act(async () => { await store.updateOnboardingDraft({ ...draft, computerIdentities: { [configuration.name]: changed } }) })
    expect(verifySetupIdentities).toHaveBeenLastCalledWith(expect.objectContaining({ github: expect.objectContaining({ computers: [{ computer: configuration.name, repositories: [], identity: changed }] }) }))
  })

})

describe("remote device onboarding", () => {
  it("connects and completes onboarding without submitting local computer configuration", async () => {
    const connectDevice = vi.fn().mockResolvedValue(undefined)
    const configureConfigurations = vi.fn()
    const onOpenApp = vi.fn()
    const store = createMemorySettingsStore()
    const source = { configureConfigurations, applicationActions: { connectDevice } } as unknown as ProductionSource
    render(<SettingsProvider store={store}><ProductionOnboarding application={null} dependencies={{ checks: [{ id: "runtime-microsandbox", title: "Runtime", status: "unavailable", detail: "Not installed", remediation: null }], retry: vi.fn() }} source={source} onOpenApp={onOpenApp} /></SettingsProvider>)
    act(() => captured.props!.onConnectDevice!())
    fireEvent.change(screen.getByRole("textbox", { name: "Device address" }), { target: { value: "owner@office" } })
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(onOpenApp).toHaveBeenCalledOnce())
    expect(connectDevice).toHaveBeenCalledWith("owner@office")
    expect(configureConfigurations).not.toHaveBeenCalled()
    expect(store.getSnapshot().settings.onboardingComplete).toBe(true)
  })

  it("keeps setup incomplete when the remote connection fails", async () => {
    const connectDevice = vi.fn().mockRejectedValue(new Error("Offline · last known status"))
    const onOpenApp = vi.fn()
    const store = createMemorySettingsStore()
    const source = { applicationActions: { connectDevice } } as unknown as ProductionSource
    render(<SettingsProvider store={store}><ProductionOnboarding application={null} dependencies={dependencies} source={source} onOpenApp={onOpenApp} /></SettingsProvider>)
    act(() => captured.props!.onConnectDevice!())
    fireEvent.change(screen.getByRole("textbox", { name: "Device address" }), { target: { value: "owner@office" } })
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    expect(await screen.findByRole("alert")).toHaveTextContent("Offline · last known status")
    expect(store.getSnapshot().settings.onboardingComplete).toBe(false)
    expect(onOpenApp).not.toHaveBeenCalled()
  })
})
