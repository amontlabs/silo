import { act, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { OnboardingApp } from "@/features/onboarding/onboarding-app"
import type { GitHubConnectionState, OnboardingActions } from "@/features/onboarding/model/onboarding-source"
import { createMemorySettingsStore, createSettingsStore, SettingsProvider, type SettingsSnapshot, type SettingsStore } from "@/features/preferences/settings-store"
import { onboardingScenarios, repositoryFixtures } from "@/fixtures/scenarios"
import { ApplicationCatalogProvider } from "@/features/preferences/application-catalog"
import { fixtureApplicationCatalog } from "@/fixtures/application-catalog"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"

function actions(): OnboardingActions {
  return { connectGitHub: vi.fn(), saveComputerConfiguration: vi.fn(), retryComputerSetup: vi.fn(), finishSetup: vi.fn() }
}

function onboarding(store: SettingsStore, handlers: OnboardingActions, { completed = false, githubConnectionState = "connected", scenario = "running", deviceIdentity }: {
  completed?: boolean
  githubConnectionState?: GitHubConnectionState
  scenario?: "running" | "complete"
  deviceIdentity?: { name: string; email: string } | null
} = {}) {
  return <SettingsProvider store={store}><ApplicationCatalogProvider initialCatalog={fixtureApplicationCatalog}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(store)}><OnboardingApp
    source={{ ...onboardingScenarios[scenario], ...(deviceIdentity !== undefined && { currentDeviceGitIdentity: deviceIdentity }) }}
    actions={handlers}
    githubConnectionState={githubConnectionState}
    completed={completed}
    repositoryOptions={repositoryFixtures}
  /></SystemIntegrationProvider></ApplicationCatalogProvider></SettingsProvider>
}

async function restartStore(previous: SettingsStore) {
  await previous.flush()
  const saved = JSON.parse(JSON.stringify(previous.getSnapshot()))
  const next = createMemorySettingsStore(saved.settings)
  await next.updateOnboardingDraft(saved.onboardingDraft)
  return next
}

describe("onboarding restart recovery", () => {
  it("reports failed draft delivery and retries the latest edits before clearing the warning", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {})
    let saved: SettingsSnapshot = { revision: 0, settings: {}, saveError: null, onboardingDraft: {
      currentStep: "github", computers: onboardingScenarios.complete.computerConfigurations,
      unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {},
    } }
    let failing = true
    const store = createSettingsStore({
      read: async () => saved,
      subscribe: async () => () => {},
      updateSettings: async patch => { saved = { ...saved, revision: saved.revision + 1, settings: { ...saved.settings, ...patch } }; return saved },
      updateOnboardingDraft: async draft => {
        if (failing) throw new Error("Draft delivery unavailable")
        saved = { ...saved, revision: saved.revision + 1, onboardingDraft: draft }
        return saved
      },
      flush: async () => {},
    })
    await store.initialize()
    const user = userEvent.setup()
    const view = render(onboarding(store, actions(), { scenario: "complete" }))
    await user.clear(screen.getByLabelText("Git name for dev"))
    await user.type(screen.getByLabelText("Git name for dev"), "Recover this author")
    expect(await screen.findByRole("alert")).toHaveTextContent("Draft delivery unavailable")
    expect(saved.onboardingDraft?.computerIdentities.dev?.name).not.toBe("Recover this author")
    failing = false
    await user.click(screen.getByRole("button", { name: "Retry saving settings" }))
    expect(saved.onboardingDraft?.computerIdentities.dev?.name).toBe("Recover this author")
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    view.unmount()
    store.dispose()
    vi.restoreAllMocks()
  })

  it("deletes the last computer, preserves the empty draft after restart, and finishes setup", async () => {
    const user = userEvent.setup()
    const configuration = onboardingScenarios.complete.computerConfigurations[0]
    const first = createMemorySettingsStore({}, { currentStep: "computers", computers: [configuration], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} })
    const handlers = { ...actions(), submitStep: vi.fn() }
    const view = render(onboarding(first, handlers, { scenario: "complete" }))
    await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
    await user.click(screen.getByRole("menuitem", { name: `Delete ${configuration.name}` }))
    await user.click(screen.getByRole("button", { name: /^Delete permanently$/ }))
    expect(screen.queryByRole("button", { name: `Delete ${configuration.name}` })).not.toBeInTheDocument()
    expect(first.getSnapshot().onboardingDraft?.computers).toEqual([])
    view.unmount()

    const restored = await restartStore(first)
    render(onboarding(restored, handlers, { scenario: "complete" }))
    expect(screen.queryByRole("button", { name: `Delete ${configuration.name}` })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Continue" }))
    expect(handlers.submitStep).toHaveBeenCalledWith("computers", expect.objectContaining({ computerConfiguration: { schemaVersion: 1, computers: [] } }))
    await user.click(screen.getByRole("tab", { name: /Review/ }))
    await user.click(screen.getByRole("button", { name: "Finish" }))
    expect(handlers.finishSetup).toHaveBeenCalledWith(expect.objectContaining({ computerConfiguration: { schemaVersion: 1, computers: [] } }))
  })

  it("fills untouched identities when host detection finishes without replacing manual edits", async () => {
    const user = userEvent.setup()
    const store = createMemorySettingsStore()
    const handlers = actions()
    const view = render(onboarding(store, handlers, { deviceIdentity: null }))
    await user.click(screen.getByRole("tab", { name: /GitHub/ }))
    expect(screen.getByLabelText("Git name for dev")).toHaveValue("")
    await user.type(screen.getByLabelText("Git name for playgrounds"), "My custom author")
    await user.click(screen.getByRole("checkbox", { name: "Apply Git identity to personal" }))
    const deviceIdentity = { name: "Detected Author", email: "detected@example.test" }
    view.rerender(onboarding(store, handlers, { deviceIdentity }))
    expect(screen.getByLabelText("Git name for dev")).toHaveValue(deviceIdentity.name)
    expect(screen.getByLabelText("Git email for dev")).toHaveValue(deviceIdentity.email)
    expect(screen.getByLabelText("Git name for playgrounds")).toHaveValue("My custom author")
    expect(screen.getByLabelText("Git email for playgrounds")).toHaveValue("")
    expect(screen.getByLabelText("Git name for personal")).toHaveValue("")
    await user.clear(screen.getByLabelText("Git name for dev"))
    await user.clear(screen.getByLabelText("Git email for dev"))
    view.rerender(onboarding(store, handlers, { deviceIdentity: { ...deviceIdentity } }))
    expect(screen.getByLabelText("Git name for dev")).toHaveValue("")
  })
  it("submits a restored computer draft once when Continue is clicked", async () => {
    const first = createMemorySettingsStore()
    const configuration = { ...onboardingScenarios.complete.computerConfigurations[0], name: "recovered" }
    await first.updateOnboardingDraft({ currentStep: "computers", computers: [configuration], unfinishedComputerEditor: null, computerSelections: { recovered: [] }, computerIdentities: { recovered: { name: "Saved Author", email: "saved@example.test", apply: true } } })
    const restored = await restartStore(first)
    const handlers = { ...actions(), submitStep: vi.fn() }
    render(onboarding(restored, handlers))
    expect(handlers.submitStep).not.toHaveBeenCalled()
    await userEvent.setup().click(screen.getByRole("button", { name: "Continue" }))
    expect(handlers.submitStep).toHaveBeenCalledOnce()
    expect(handlers.submitStep).toHaveBeenCalledWith("computers", expect.objectContaining({ computerConfiguration: { schemaVersion: 1, computers: [configuration] }, github: expect.objectContaining({ computers: [{ computer: "recovered", repositories: [], identity: { name: "Saved Author", email: "saved@example.test", apply: true } }] }) }))
    expect(handlers.saveComputerConfiguration).not.toHaveBeenCalled()
    expect(screen.getByRole("tab", { name: /GitHub/ })).toHaveAttribute("aria-selected", "true")
  })

  it("applies saved reduced motion and follows shared changes without remounting the shell", async () => {
    const store = createMemorySettingsStore({ reduceMotion: true })
    render(onboarding(store, actions()))
    const shell = screen.getByRole("region", { name: "Silo Setup" })
    expect(shell).toHaveAttribute("data-reduce-motion", "true")
    await act(async () => { await store.updateSettings({ reduceMotion: false }) })
    expect(shell).not.toHaveAttribute("data-reduce-motion")
    await act(async () => { await store.updateSettings({ reduceMotion: true }) })
    expect(screen.getByRole("region", { name: "Silo Setup" })).toBe(shell)
    expect(shell).toHaveAttribute("data-reduce-motion", "true")
  })

  it("recovers incomplete editor input and discards only the editor draft on Cancel", async () => {
    const user = userEvent.setup()
    const handlers = actions()
    const first = createMemorySettingsStore()
    const view = render(onboarding(first, handlers))
    await user.click(screen.getByRole("tab", { name: /Computers/ }))
    await user.click(screen.getByRole("button", { name: "Add" }))
    await user.click(screen.getByRole("menuitem", { name: "New computer" }))
    await user.clear(screen.getByRole("textbox", { name: "Computer name" }))
    await user.type(screen.getByRole("textbox", { name: "Computer name" }), "Unfinished.")
    view.unmount()

    const second = await restartStore(first)
    const restored = render(onboarding(second, handlers))
    expect(screen.getByRole("tab", { name: /Computers/ })).toHaveAttribute("aria-selected", "true")
    expect(screen.getByRole("textbox", { name: "Computer name" })).toHaveValue("Unfinished.")
    expect(handlers.saveComputerConfiguration).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(screen.getByText("Use 1–32 lowercase letters, numbers, or hyphens, starting with a letter.")).toBeVisible()
    expect(handlers.saveComputerConfiguration).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    restored.unmount()

    const third = await restartStore(second)
    render(onboarding(third, handlers))
    expect(screen.queryByRole("textbox", { name: "Computer name" })).not.toBeInTheDocument()
    expect(within(screen.getByRole("list", { name: "Configured computers" })).getAllByRole("listitem")).toHaveLength(3)
    expect(third.getSnapshot().onboardingDraft?.unfinishedComputerEditor).toBeNull()
  })

  it("restores configuration edits, ordering, repository access, identity choices, and the active step without applying them", async () => {
    const user = userEvent.setup()
    const handlers = actions()
    const first = createMemorySettingsStore()
    const view = render(onboarding(first, handlers))
    await user.click(screen.getByRole("tab", { name: /GitHub/ }))
    await user.clear(screen.getByLabelText("Git name for dev"))
    await user.type(screen.getByLabelText("Git name for dev"), "Recovered Author")
    await user.clear(screen.getByLabelText("Git email for dev"))
    await user.type(screen.getByLabelText("Git email for dev"), "unfinished@")
    await user.click(screen.getByRole("checkbox", { name: "Apply Git identity to dev" }))
    const pushes = within(screen.getByRole("table", { name: "Selected repositories for dev" })).getByRole("checkbox", { name: "Allow GitHub changes for acme/silo" })
    if (pushes.getAttribute("aria-checked") === "false") await user.click(pushes)

    await user.click(screen.getByRole("tab", { name: /Computers/ }))
    await user.click(screen.getByRole("button", { name: `More actions for dev` }))
    await user.click(screen.getByRole("menuitem", { name: `Edit dev` }))
    await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
    await user.click(screen.getByRole("button", { name: "Save" }))
    await user.click(screen.getByRole("button", { name: "Reorder personal" }))
    await user.keyboard("{ArrowUp}{ArrowUp}")
    await user.click(screen.getByRole("tab", { name: /Review/ }))
    view.unmount()

    const second = await restartStore(first)
    vi.mocked(handlers.saveComputerConfiguration).mockClear()
    const restored = render(onboarding(second, handlers, { githubConnectionState: "disconnected" }))
    expect(screen.getByRole("tab", { name: /Review/ })).toHaveAttribute("aria-selected", "true")
    const list = screen.getByRole("list", { name: "Computers" })
    expect(within(list).getAllByRole("listitem").map((row) => row.textContent)).toEqual([
      expect.stringContaining("personal"), expect.stringContaining("dev"), expect.stringContaining("playgrounds"),
    ])
    expect(screen.getByText("GitHub not connected")).toBeVisible()
    expect(handlers.connectGitHub).not.toHaveBeenCalled()
    expect(handlers.saveComputerConfiguration).not.toHaveBeenCalled()
    expect(handlers.finishSetup).not.toHaveBeenCalled()
    await user.click(screen.getByRole("tab", { name: /GitHub/ }))
    expect(screen.getByLabelText("Git name for dev")).toHaveValue("Recovered Author")
    expect(screen.getByLabelText("Git email for dev")).toHaveValue("unfinished@")
    expect(screen.getByRole("checkbox", { name: "Apply Git identity to dev" })).not.toBeChecked()
    restored.rerender(onboarding(second, handlers))
    expect(within(screen.getByRole("table", { name: "Selected repositories for dev" })).getByRole("checkbox", { name: "Allow GitHub changes for acme/silo" })).toBeChecked()
    expect(second.getSnapshot().onboardingDraft?.unfinishedComputerEditor).toBeNull()
  })

  it("saves shared application choices immediately and clears recovery only after confirmed completion", async () => {
    const user = userEvent.setup()
    const handlers = actions()
    const store = createMemorySettingsStore({ terminal: "iTerm", editor: "Cursor", browser: "Firefox" })
    const view = render(onboarding(store, handlers, { scenario: "complete" }))
    expect(screen.getByRole("combobox", { name: "Terminal" })).toHaveTextContent("iTerm")
    expect(screen.getByRole("combobox", { name: "Code editor" })).toHaveTextContent("Cursor")
    await user.click(screen.getByRole("combobox", { name: "Browser" }))
    await user.click(screen.getByRole("option", { name: "Google Chrome" }))
    expect(store.getSnapshot().settings.browser).toBe("Google Chrome")
    await act(async () => { await store.updateSettings({ editor: "Zed" }) })
    expect(screen.getByRole("combobox", { name: "Code editor" })).toHaveTextContent("Zed")
    await user.click(screen.getByRole("tab", { name: /Review/ }))
    await user.click(screen.getByRole("button", { name: "Finish" }))
    expect(handlers.finishSetup).toHaveBeenCalledOnce()
    expect(handlers.finishSetup).toHaveBeenCalledWith(expect.objectContaining({ applications: { terminal: "iTerm", editor: "Zed", browser: "Google Chrome", browserPath: "/fixture/Google Chrome.app", terminalUseSystemDefault: false, editorUseSystemDefault: false, browserUseSystemDefault: false } }))
    expect(store.getSnapshot().onboardingDraft?.currentStep).toBe("review")
    expect(store.getSnapshot().onboardingDraft).not.toHaveProperty("applications")
    view.rerender(onboarding(store, handlers, { completed: true, scenario: "complete" }))
    await act(async () => { await store.flush() })
    expect(store.getSnapshot().onboardingDraft).toBeNull()
    expect(store.getSnapshot().settings).toMatchObject({ terminal: "iTerm", editor: "Zed", browser: "Google Chrome", browserPath: "/fixture/Google Chrome.app" })
  })
})
