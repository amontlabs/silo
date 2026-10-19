import { describe, expect, it, vi } from "vitest"

import { onboardingScenarios } from "@/fixtures/scenarios"
import { projectOnboarding } from "@/features/onboarding/model/onboarding-state"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ProductionSnapshot } from "./production-source"
import { productionOnboardingSource } from "./production-onboarding"

const application = applicationSourceForScenario("running")
describe("production onboarding", () => {
  it("enables Finish after an empty computer configuration is saved and verified", () => {
    const emptyApplication = { ...application, computers: [] }
    const setup = { setupCandidate: { schemaVersion: 1, computers: [] }, setupQueue: [
      { id: "computerRun", status: "succeeded" }, { id: "computerVerify", status: "succeeded" },
    ], setupEvents: [] } as unknown as ProductionSnapshot
    const dependencies = { checks: onboardingScenarios.complete.preflightChecks, retry: vi.fn() }
    const source = productionOnboardingSource(emptyApplication, dependencies, application.preferences, setup)
    expect(source.computerConfigurations).toEqual([])
    expect(projectOnboarding(source, "disconnected").finishEnabled).toBe(true)
    const pending = { ...setup, setupQueue: setup.setupQueue.map((item) => ({ ...item, status: "running" as const })) }
    expect(productionOnboardingSource(emptyApplication, dependencies, application.preferences, pending).readyToFinish).toBe(false)
  })

  it("projects only live dependency and configuration state", () => {
    const checks = [{ id: "system-os", title: "Supported OS", status: "pass" as const, detail: "macOS", remediation: null }]
    const source = productionOnboardingSource(application, { checks, retry: vi.fn() }, application.preferences)
    expect(source.preflightChecks).toBe(checks)
    expect(source.computerConfigurations).toEqual(application.computers.map(({ configuration }) => configuration))
    expect(source.bootstrapResult?.message).toBe("Computer configuration verified.")
    expect(source.bootstrapState.completedPhases).not.toContain("identity")
    expect(source.bootstrapState.completedPhases).not.toContain("github")
    expect(source.readyToFinish).toBe(true)
  })

  it("never adopts remote computers into local setup or counts them as configured", () => {
    const remote = { ...application.computers[0], device: { id: "office", computerId: "remote-computer", name: "Office", address: "owner@office", connected: true }, configuration: { ...application.computers[0].configuration, id: "silo-remote:office:remote-computer", name: "remote-build" } }
    const source = productionOnboardingSource({ ...application, computers: [remote] }, { checks: [], retry: vi.fn() }, application.preferences)
    expect(source.computerConfigurations).not.toEqual(expect.arrayContaining([expect.objectContaining({ name: "remote-build" })]))
    expect(source.readyToFinish).toBe(false)
    const mixed = productionOnboardingSource({ ...application, computers: [...application.computers, remote] }, { checks: [], retry: vi.fn() }, application.preferences)
    expect(mixed.computerConfigurations).toEqual(application.computers.map(({ configuration }) => configuration))
  })

  it("offers only dev on a fresh install without claiming it has been created", () => {
    const source = productionOnboardingSource({ ...application, computers: [] }, { checks: [], retry: vi.fn() }, application.preferences)
    expect(source.computerConfigurations).toEqual([expect.objectContaining({ name: "dev", cpus: 8, memoryGiB: 32, workspaceStorageGiB: 120, runtimeStorageGiB: 100 })])
    expect(source.readyToFinish).toBe(false)
    expect(source.bootstrapResult).toBeNull()
    expect(source.progressEvents).toEqual([])
  })

  it("enables Finish after a real computer is configured without inventing progress events", () => {
    const native = { ...application, computers: [application.computers[0]] }
    const dependencies = { checks: onboardingScenarios.complete.preflightChecks, retry: vi.fn() }
    const current = productionOnboardingSource(native, dependencies, application.preferences)
    const view = projectOnboarding(current, "disconnected")
    expect(view.finishEnabled).toBe(true)
    expect(view.computerProgress.computers[0].status).toBe("ready")
    expect(view.computerProgress.completedOperations).toBe(2)
    expect(view.computerProgress.totalOperations).toBe(2)
    expect(current.progressEvents).toEqual([])
    expect(projectOnboarding(productionOnboardingSource(null, dependencies, application.preferences), "disconnected").finishEnabled).toBe(false)
  })

  it("keeps dependency onboarding available when runtime state cannot load", () => {
    const checks = [{ id: "runtime-microsandbox", title: "MicroSandbox", status: "failed" as const, detail: "Bundled runtime missing", remediation: null }]
    const source = productionOnboardingSource(null, { checks, retry: vi.fn() }, application.preferences)
    expect(source.preflightChecks).toBe(checks)
    expect(source.computerConfigurations.map(({ name }) => name)).toEqual(["dev"])
    expect(source.bootstrapResult).toBeNull()
  })

  it("names why Finish is unavailable when a created computer failed, is unconfirmed or is starting", () => {
    const dependencies = { checks: onboardingScenarios.complete.preflightChecks, retry: vi.fn() }
    const [dev, other] = application.computers
    const withDev = (changes: Partial<typeof dev>) => ({ ...application, computers: [{ ...dev, ...changes }, other] })
    const failed = productionOnboardingSource(withDev({ state: "failed", stateDetail: "Failed", lifecycleFailure: "Start failed: not enough memory" }), dependencies, application.preferences)
    expect(failed.readyToFinish).toBe(false)
    expect(failed.finishBlocker).toEqual({ computer: dev.configuration.name, action: "start", message: `${dev.configuration.name} is not running: Start failed: not enough memory. Start it to finish setup.` })
    const stale = productionOnboardingSource(withDev({ freshness: "stale" }), dependencies, application.preferences)
    expect(stale.finishBlocker).toMatchObject({ computer: dev.configuration.name, action: "refresh" })
    const starting = productionOnboardingSource(withDev({ state: "starting" }), dependencies, application.preferences)
    expect(starting.finishBlocker).toMatchObject({ computer: dev.configuration.name, action: null, message: `Waiting for ${dev.configuration.name} to start…` })
    expect(projectOnboarding(failed, "disconnected").finishBlocker).toEqual(failed.finishBlocker)
    // Nothing blocks a configured device, and a running setup explains itself.
    expect(productionOnboardingSource(application, dependencies, application.preferences).finishBlocker).toBeNull()
    const applying = { ...withDev({ state: "starting" }), computerConfigurationOperation: { id: "a", status: "applying", candidate: { schemaVersion: 1, computers: [] }, progressEvents: [], result: null, error: null } } as typeof application
    expect(productionOnboardingSource(applying, dependencies, application.preferences).finishBlocker).toBeNull()
  })

  it("seeds from real local state, and marks the saved list or defaults as a placeholder before it", () => {
    const dependencies = { checks: [], retry: vi.fn() }
    const configurations = application.computers.map(({ configuration }) => configuration)
    const snapshot = (extra: Partial<ProductionSnapshot>) => ({ setupQueue: [], setupEvents: [], error: null, ...extra }) as unknown as ProductionSnapshot
    const loaded = productionOnboardingSource(application, dependencies, application.preferences, snapshot({}))
    expect(loaded).toMatchObject({ configurationsAuthoritative: true, existingConfigurations: configurations, computerConfigurations: configurations })

    const beforeLoad = productionOnboardingSource(null, dependencies, application.preferences, snapshot({ savedConfigurations: [configurations[1]] }))
    expect(beforeLoad).toMatchObject({ configurationsAuthoritative: false, existingConfigurations: undefined, computerConfigurations: [configurations[1]] })

    // While this device's computers update, the shell has no local rows: not an empty device.
    const shell = { ...application, computers: [] }
    const updating = productionOnboardingSource(shell, dependencies, application.preferences, snapshot({ localUpdating: true }))
    expect(updating.configurationsAuthoritative).toBe(false)
    // A real local computer is not reported as absent while the device's state is unknown.
    const updatingWithDev = productionOnboardingSource(application, dependencies, application.preferences, snapshot({ localUpdating: true }))
    expect(updatingWithDev.existingConfigurations).toBeUndefined()
    expect(updatingWithDev.existingConfigurationIds).toBeUndefined()
    const unreadable = productionOnboardingSource(shell, dependencies, application.preferences, snapshot({ error: "Silo could not read application state" }))
    expect(unreadable.configurationsAuthoritative).toBe(false)
    expect(unreadable.computerConfigurations.map(({ name }) => name)).toEqual(["dev"])

    // A loaded device with no computers offers the default, as setup of this device.
    expect(productionOnboardingSource(shell, dependencies, application.preferences, snapshot({})).configurationsAuthoritative).toBe(true)
  })

  it("reports the ids of existing computers whose saved configuration cannot be read", () => {
    const broken = structuredClone(application)
    broken.computers[0] = { ...broken.computers[0], configuration: { ...broken.computers[0].configuration, cpus: "many" as unknown as number } }
    const source = productionOnboardingSource(broken, { checks: [], retry: vi.fn() }, application.preferences)
    expect(source.existingConfigurations?.map(({ id }) => id)).not.toContain(broken.computers[0].configuration.id)
    expect(source.existingConfigurationIds).toContain(broken.computers[0].configuration.id)
  })
})
