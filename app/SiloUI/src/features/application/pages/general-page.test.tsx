import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { GeneralPage } from "./general-page"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createMemorySettingsStore, createSettingsStore, SettingsProvider, type SettingsBackend, type SettingsSnapshot } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"
import { PreUpgradeBackupProvider } from "@/features/storage/pre-upgrade-backup"
import { createFixturePreUpgradeBackup } from "@/fixtures/pre-upgrade-backup"
import { Toaster } from "@/components/ui/sonner"
import { createComputerUseBridge, type ComputerUseBackend } from "@/desktop/computer-use-bridge"
import { ComputerUseProvider } from "@/desktop/computer-use-provider"

it.each(["default", "empty"] as const)("persists the %s startup selection when enabled without editing the selection", async (selection) => {
  const user = userEvent.setup()
  const source = applicationSourceForScenario("running")
  const expected = selection === "default" ? [source.computers.find(({ configuration }) => configuration.name === "dev")!.configuration.id] : []
  let saved: SettingsSnapshot = { revision: 0, settings: selection === "empty" ? { startupComputerIds: [] } : {}, onboardingDraft: null, saveError: null }
  const backend: SettingsBackend = {
    read: async () => saved,
    subscribe: async () => () => {},
    updateSettings: async (patch) => (saved = { ...saved, revision: saved.revision + 1, settings: { ...saved.settings, ...patch } }),
    updateOnboardingDraft: async () => saved,
    flush: async () => {},
  }
  const settings = createSettingsStore(backend)
  await settings.initialize()
  const view = render(<SettingsProvider store={settings}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(settings)}><GeneralPage source={source} applicationPreferences={source.preferences} onApplicationPreferencesChange={vi.fn()} reduceMotion={false} onReduceMotionChange={vi.fn()} /></SystemIntegrationProvider></SettingsProvider>)

  await user.click(screen.getByRole("switch", { name: "Start computers at launch" }))
  await settings.flush()
  expect(saved.settings).toMatchObject({ startComputersAtLaunch: true, startupComputerIds: expected })
  view.unmount()
  settings.dispose()

  const reopened = createSettingsStore(backend)
  await reopened.initialize()
  expect(reopened.getSnapshot().settings).toMatchObject({ startComputersAtLaunch: true, startupComputerIds: expected })
  reopened.dispose()
})

it("defaults the startup selection to a local computer even when a remote one is named dev", async () => {
  const user = userEvent.setup()
  const source = structuredClone(applicationSourceForScenario("running"))
  const [dev, playgrounds] = source.computers
  source.computers = [{ ...dev, configuration: { ...dev.configuration, id: "remote-dev" }, device: { id: "office", name: "Office Mac", address: "office.local", connected: true, computerId: dev.configuration.id } }, playgrounds]
  source.preferences = { ...source.preferences, startComputersAtLaunch: false, startupComputerIds: undefined }
  const settings = createMemorySettingsStore({})
  render(<SettingsProvider store={settings}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(settings)}><GeneralPage source={source} applicationPreferences={source.preferences} onApplicationPreferencesChange={vi.fn()} reduceMotion={false} onReduceMotionChange={vi.fn()} /></SystemIntegrationProvider></SettingsProvider>)
  await user.click(screen.getByRole("switch", { name: "Start computers at launch" }))
  expect(settings.getSnapshot().settings.startupComputerIds).toEqual([playgrounds.configuration.id])
  expect(screen.getByRole("button", { name: `Remove ${playgrounds.configuration.name}` })).toBeVisible()
})

it("searches a long startup computer list and preserves selections when startup is toggled", async () => {
  const user = userEvent.setup()
  const source = applicationSourceForScenario("running")
  source.computers = Array.from({ length: 64 }, (_, index) => ({
    ...source.computers[0],
    configuration: { ...source.computers[0].configuration, id: `computer-${index + 1}`, name: `computer-${index + 1}` },
  }))
  const settings = createMemorySettingsStore(source.preferences)
  render(<SettingsProvider store={settings}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(settings)}><GeneralPage source={source} applicationPreferences={source.preferences} onApplicationPreferencesChange={vi.fn()} reduceMotion={false} onReduceMotionChange={vi.fn()} /></SystemIntegrationProvider></SettingsProvider>)
  const startup = screen.getByRole("switch", { name: "Start computers at launch" })
  if (!source.preferences.startComputersAtLaunch) await user.click(startup)
  const input = screen.getByRole("combobox", { name: "Add computer at startup" })
  await user.type(input, "computer-64")
  expect(screen.getAllByRole("option")).toHaveLength(1)
  await user.keyboard("{Enter}")
  expect(screen.getByRole("button", { name: "Remove computer-64" })).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Remove computer-1" }))
  await user.click(startup)
  expect(screen.queryByRole("combobox", { name: "Add computer at startup" })).not.toBeInTheDocument()
  await user.click(startup)
  expect(screen.getByRole("button", { name: "Remove computer-64" })).toBeVisible()
  expect(screen.queryByRole("button", { name: "Remove computer-1" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Clear" }))
  expect(screen.queryByRole("button", { name: "Remove computer-64" })).not.toBeInTheDocument()
})

function renderGeneralPage(backup?: ReturnType<typeof createFixturePreUpgradeBackup>) {
  const source = applicationSourceForScenario("running")
  const settings = createMemorySettingsStore(source.preferences)
  const page = <GeneralPage source={source} applicationPreferences={source.preferences} onApplicationPreferencesChange={vi.fn()} reduceMotion={false} onReduceMotionChange={vi.fn()} />
  return render(<SettingsProvider store={settings}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(settings)}><Toaster />{backup ? <PreUpgradeBackupProvider backend={backup}>{page}</PreUpgradeBackupProvider> : page}</SystemIntegrationProvider></SettingsProvider>)
}

it("lists the pre-upgrade backup under Storage, after the other sections, until it is deleted", async () => {
  const user = userEvent.setup()
  const backup = createFixturePreUpgradeBackup()
  renderGeneralPage(backup)
  const storage = await screen.findByRole("region", { name: "Storage" })
  const headings = screen.getAllByRole("heading", { level: 3 }).map(heading => heading.textContent)
  expect(headings.slice(-2)).toEqual(["Accessibility", "Storage"])
  expect(storage).toHaveTextContent("Pre-upgrade backup")
  expect(await screen.findByText("12.40 GiB · deleted on October 15, 2026")).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Delete now" }))
  await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
  await waitFor(() => expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument())
  expect(backup.calls).toContain("remove")
})

it("has no Storage section without a pre-upgrade backup", async () => {
  const backup = createFixturePreUpgradeBackup({ gone: true })
  renderGeneralPage(backup)
  await waitFor(() => expect(backup.calls).toEqual(["read"]))
  expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument()
  expect(screen.getByRole("heading", { name: "Accessibility" })).toBeVisible()
})

it("reports unsaved preferences and retries delivery without losing the selection", async () => {
  const user = userEvent.setup()
  const errors = vi.spyOn(console, "error").mockImplementation(() => {})
  const source = applicationSourceForScenario("running")
  let saved: SettingsSnapshot = { revision: 0, settings: { startComputersAtLaunch: false }, onboardingDraft: null, saveError: null }
  let failDelivery = true
  const settings = createSettingsStore({
    read: async () => saved, subscribe: async () => () => {},
    updateSettings: async (patch) => {
      if (failDelivery) throw new Error("Settings delivery unavailable")
      return (saved = { ...saved, revision: saved.revision + 1, settings: { ...saved.settings, ...patch } })
    },
    updateOnboardingDraft: async () => saved, flush: async () => {},
  }, {}, saved)
  render(<SettingsProvider store={settings}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(settings)}><GeneralPage source={source} applicationPreferences={source.preferences} onApplicationPreferencesChange={vi.fn()} reduceMotion={false} onReduceMotionChange={vi.fn()} /></SystemIntegrationProvider></SettingsProvider>)
  await user.click(screen.getByRole("switch", { name: "Start computers at launch" }))
  expect(errors).toHaveBeenCalledExactlyOnceWith("Silo settings:", "Settings delivery unavailable")
  errors.mockRestore()
  expect(await screen.findByRole("alert")).toHaveTextContent("Settings could not be saved")
  expect(screen.getByRole("switch", { name: "Start computers at launch" })).toBeChecked()
  expect(saved.settings.startComputersAtLaunch).toBe(false)
  failDelivery = false
  await user.click(screen.getByRole("button", { name: "Retry saving settings" }))
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument())
  expect(saved.settings.startComputersAtLaunch).toBe(true)
  settings.dispose()
})

it("explains write-protected settings without offering a save retry", () => {
  const source = applicationSourceForScenario("running")
  const saved: SettingsSnapshot = { revision: 0, settings: {}, onboardingDraft: null, saveError: "Settings use an unsupported file version", writeProtected: true }
  const settings = createSettingsStore({
    read: async () => saved, subscribe: async () => () => {}, updateSettings: async () => saved,
    updateOnboardingDraft: async () => saved, flush: async () => {},
  }, {}, saved)
  render(<SettingsProvider store={settings}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(settings)}><GeneralPage source={source} applicationPreferences={source.preferences} onApplicationPreferencesChange={vi.fn()} reduceMotion={false} onReduceMotionChange={vi.fn()} /></SystemIntegrationProvider></SettingsProvider>)
  expect(screen.getByRole("alert")).toHaveTextContent("Changes last for this session")
  expect(screen.queryByRole("button", { name: "Retry saving settings" })).not.toBeInTheDocument()
  settings.dispose()
})

it("offers the new-computer computer use approval where the other general settings are", async () => {
  const store = createMemorySettingsStore()
  const source = applicationSourceForScenario("running")
  const backend: ComputerUseBackend = {
    readDesktopState: async () => ({}), setApproval: async () => ({}), setup: async () => ({}),
    chatGptStatus: async () => ({ state: "ready", path: "/p", version: "1" }), retry: async () => ({}), listenStatus: async () => () => {},
  }
  render(<SettingsProvider store={store}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(store)}><ComputerUseProvider bridge={createComputerUseBridge(backend)}>
    <GeneralPage source={source} applicationPreferences={source.preferences} onApplicationPreferencesChange={vi.fn()} reduceMotion={false} onReduceMotionChange={vi.fn()} />
  </ComputerUseProvider></SystemIntegrationProvider></SettingsProvider>)
  const toggle = screen.getByRole("switch", { name: "Allow agents to use the desktop without asking in new computers" })
  expect(screen.getByRole("region", { name: "Computer use" })).toContainElement(toggle)
  await userEvent.setup().click(toggle)
  await waitFor(() => expect(store.getSnapshot().settings.computerUseAutoApproval).toBe(true))
})
