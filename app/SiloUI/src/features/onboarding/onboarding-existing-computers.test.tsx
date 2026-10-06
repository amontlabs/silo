import { act, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { OnboardingApp } from "@/features/onboarding/onboarding-app"
import type { OnboardingActions, OnboardingSource } from "@/features/onboarding/model/onboarding-source"
import { createMemorySettingsStore, SettingsProvider, type SettingsStore } from "@/features/preferences/settings-store"
import { onboardingScenarios } from "@/fixtures/scenarios"
import { ApplicationCatalogProvider } from "@/features/preferences/application-catalog"
import { fixtureApplicationCatalog } from "@/fixtures/application-catalog"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"

const [real, other] = onboardingScenarios.complete.computerConfigurations
// The default seeded before the real computers loaded: same name as a real one, another id.
const placeholder = { ...real, id: "00000000-0000-4000-8000-0000000000aa" }

function actions() {
  return { connectGitHub: vi.fn(), saveComputerConfiguration: vi.fn(), retryComputerSetup: vi.fn(), finishSetup: vi.fn(), submitStep: vi.fn() } satisfies OnboardingActions
}

function onboarding(store: SettingsStore, handlers: OnboardingActions, source: Partial<OnboardingSource>) {
  return <SettingsProvider store={store}><ApplicationCatalogProvider initialCatalog={fixtureApplicationCatalog}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(store)}><OnboardingApp
    source={{ ...onboardingScenarios.running, ...source }}
    actions={handlers}
    githubConnectionState="disconnected"
    completed={false}
  /></SystemIntegrationProvider></ApplicationCatalogProvider></SettingsProvider>
}

describe("Finish blocked by a computer after setup", () => {
  const review = { currentStep: "review" as const, computers: [real], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} }
  const settledQueue = (["computerRun", "computerVerify"] as const).map((id) => ({ id, status: "succeeded" as const }))

  it("names a failed computer and offers to start it", async () => {
    const user = userEvent.setup()
    const handlers = { ...actions(), startComputer: vi.fn() }
    const message = `${real.name} is not running: Start failed. Start it to finish setup.`
    render(onboarding(createMemorySettingsStore({}, review), handlers, { ...onboardingScenarios.complete, readyToFinish: false, setupQueue: settledQueue, finishBlocker: { computer: real.name, action: "start", message } }))
    expect(screen.getByRole("button", { name: "Finish" })).toBeDisabled()
    expect(screen.getByRole("contentinfo", { name: "Onboarding actions" })).toHaveTextContent(`Needs attention · ${message}`)
    expect(screen.queryByText("Not started · Continue to create your computers. The first time can take a few minutes.")).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: `Start ${real.name}` }))
    expect(handlers.startComputer).toHaveBeenCalledWith(real.name)
  })

  it("offers to check an unconfirmed computer again", async () => {
    const user = userEvent.setup()
    const handlers = { ...actions(), refreshSetupState: vi.fn() }
    render(onboarding(createMemorySettingsStore({}, review), handlers, { ...onboardingScenarios.complete, readyToFinish: false, setupQueue: settledQueue, finishBlocker: { computer: real.name, action: "refresh", message: `${real.name}'s status could not be confirmed. Check again to finish setup.` } }))
    expect(screen.getByRole("contentinfo", { name: "Onboarding actions" })).toHaveTextContent("Waiting · ")
    await user.click(screen.getByRole("button", { name: "Check again" }))
    expect(handlers.refreshSetupState).toHaveBeenCalledOnce()
  })
})

describe("onboarding with computers that already exist", () => {
  it("replaces a placeholder seed with this device's computers once they load", async () => {
    const store = createMemorySettingsStore()
    const handlers = actions()
    const view = render(onboarding(store, handlers, { computerConfigurations: [placeholder], configurationsAuthoritative: false }))
    // A placeholder is not saved as the user's draft.
    expect(store.getSnapshot().onboardingDraft).toBeNull()
    await act(async () => { view.rerender(onboarding(store, handlers, { computerConfigurations: [real, other], configurationsAuthoritative: true, existingConfigurations: [real, other] })) })
    expect(store.getSnapshot().onboardingDraft?.computers).toEqual([real, other])
    // Once seeded from real state it is not replaced again.
    await act(async () => { view.rerender(onboarding(store, handlers, { computerConfigurations: [real], configurationsAuthoritative: true, existingConfigurations: [real] })) })
    expect(store.getSnapshot().onboardingDraft?.computers).toEqual([real, other])
  })

  it("asks before Continue would delete an existing computer, and keeping it restores it", async () => {
    const user = userEvent.setup()
    const store = createMemorySettingsStore({}, { currentStep: "computers", computers: [placeholder], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} })
    const handlers = actions()
    render(onboarding(store, handlers, { computerConfigurations: [real], existingConfigurations: [real] }))
    await user.click(screen.getByRole("button", { name: "Continue" }))
    const confirmation = screen.getByRole("alert", { name: "Confirm computer deletion" })
    expect(confirmation).toHaveTextContent(`Delete ${real.name}?`)
    expect(handlers.submitStep).not.toHaveBeenCalled()
    expect(screen.getByRole("tab", { name: /Computers/ })).toHaveAttribute("aria-selected", "true")
    await user.click(within(confirmation).getByRole("button", { name: "Keep computer" }))
    // The placeholder with the same name gives way to the computer that exists.
    expect(handlers.submitStep).toHaveBeenCalledOnce()
    expect(handlers.submitStep).toHaveBeenCalledWith("computers", expect.objectContaining({ computerConfiguration: { schemaVersion: 1, computers: [real] } }))
    expect(store.getSnapshot().onboardingDraft?.computers).toEqual([real])
    expect(screen.getByRole("tab", { name: /GitHub/ })).toHaveAttribute("aria-selected", "true")
    expect(screen.queryByRole("alert", { name: "Confirm computer deletion" })).not.toBeInTheDocument()
  })

  it("adopts loaded computers after cancelling an editor opened on a placeholder seed", async () => {
    const user = userEvent.setup()
    const store = createMemorySettingsStore()
    const handlers = actions()
    const view = render(onboarding(store, handlers, { computerConfigurations: [placeholder], configurationsAuthoritative: false }))
    await user.click(screen.getByRole("tab", { name: /Computers/ }))
    await user.click(screen.getByRole("button", { name: `More actions for ${placeholder.name}` }))
    await user.click(screen.getByRole("menuitem", { name: `Edit ${placeholder.name}` }))

    await act(async () => { view.rerender(onboarding(store, handlers, { computerConfigurations: [real, other], configurationsAuthoritative: true, existingConfigurations: [real, other] })) })
    expect(store.getSnapshot().onboardingDraft?.computers).toEqual([placeholder])
    await user.click(screen.getByRole("button", { name: "Cancel" }))

    expect(store.getSnapshot().onboardingDraft?.computers).toEqual([real, other])
    expect(handlers.saveComputerConfiguration).not.toHaveBeenCalled()
  })

  it("deletes an existing computer only after the user confirms it", async () => {
    const user = userEvent.setup()
    const added = { ...other, id: "7f3c2a10-4b5d-4e6f-8a9b-0c1d2e3f4a5b", name: "fresh" }
    const store = createMemorySettingsStore({}, { currentStep: "computers", computers: [added], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} })
    const handlers = actions()
    render(onboarding(store, handlers, { computerConfigurations: [real], existingConfigurations: [real] }))
    await user.click(screen.getByRole("button", { name: "Continue" }))
    await user.click(screen.getByRole("button", { name: `Delete ${real.name}` }))
    expect(handlers.submitStep).toHaveBeenCalledWith("computers", expect.objectContaining({ computerConfiguration: { schemaVersion: 1, computers: [added] } }), { confirmedDeletions: [real.id] })
    // Later submissions carry the confirmation without asking again.
    await user.click(screen.getByRole("button", { name: "Continue" }))
    expect(screen.queryByRole("alert", { name: "Confirm computer deletion" })).not.toBeInTheDocument()
  })

  it("treats the list's own Delete confirmation as explicit", async () => {
    const user = userEvent.setup()
    const store = createMemorySettingsStore({}, { currentStep: "computers", computers: [real, other], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} })
    const handlers = actions()
    render(onboarding(store, handlers, { computerConfigurations: [real, other], existingConfigurations: [real, other] }))
    await user.click(screen.getByRole("button", { name: `More actions for ${other.name}` }))
    await user.click(screen.getByRole("menuitem", { name: `Delete ${other.name}` }))
    await user.click(screen.getByRole("button", { name: "Delete permanently" }))
    expect(screen.queryByRole("alert", { name: "Confirm computer deletion" })).not.toBeInTheDocument()
    expect(handlers.saveComputerConfiguration).toHaveBeenCalledWith({ schemaVersion: 1, computers: [real] }, { confirmedDeletions: [other.id] })
  })

  it("retries with the current draft, not the failed request", async () => {
    const user = userEvent.setup()
    const store = createMemorySettingsStore({}, { currentStep: "computers", computers: [real], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: { [real.name]: { name: "Edited", email: "edited@example.test", apply: true } } })
    const handlers = actions()
    render(onboarding(store, handlers, onboardingScenarios["bootstrap-failure"]))
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(handlers.retryComputerSetup).toHaveBeenCalledWith(expect.objectContaining({
      computerConfiguration: { schemaVersion: 1, computers: [real] },
      github: expect.objectContaining({ computers: [expect.objectContaining({ computer: real.name, identity: { name: "Edited", email: "edited@example.test", apply: true } })] }),
    }))
  })
})
