import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { ApplicationPreview } from "@/fixtures/application-preview"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { createMemorySettingsStore, createSettingsStore, SettingsProvider, type SettingsSnapshot, type SettingsStore } from "@/features/preferences/settings-store"
import { createFixtureSettingsStore } from "@/fixtures/settings"

function application(store: SettingsStore, source: ApplicationSource) {
  return <SettingsProvider store={store}>
    <ApplicationPreview source={source} initialRoute={{ tab: "settings", settingsSection: "general" }} />
  </SettingsProvider>
}

function settingsPanel() {
  return within(screen.getByRole("region", { name: "Settings" }))
}

it("keeps every General and Notifications choice after source replacement and remount", async () => {
  const user = userEvent.setup()
  const source = applicationSourceForScenario("running")
  const dev = source.computers.find(({ configuration }) => configuration.name === "dev")!.configuration.id
  const store = createMemorySettingsStore({ ...source.preferences, startupComputerIds: [dev] })
  const view = render(application(store, source))
  const settings = settingsPanel()
  await settings.findByRole("combobox", { name: "Theme" })

  for (const [label, option] of [["Theme", "Dark"], ["Terminal", "iTerm"], ["Code editor", "Cursor"], ["Browser", "Firefox"]]) {
    await user.click(settings.getByRole("combobox", { name: label }))
    await user.click(screen.getByRole("option", { name: option }))
  }
  await user.click(settings.getByRole("switch", { name: "Launch Silo at login" }))
  await user.click(settings.getByRole("switch", { name: "Start computers at launch" }))
  await user.click(settings.getByRole("button", { name: "Remove dev" }))
  await user.click(settings.getByRole("combobox", { name: "Add computer at startup" }))
  await user.click(screen.getByRole("option", { name: "playgrounds" }))
  await user.click(settings.getByRole("switch", { name: "Reduce motion" }))

  const navigation = within(screen.getByRole("navigation", { name: "Silo navigation" }))
  await user.click(navigation.getByRole("button", { name: "Notifications" }))
  await user.click(await settings.findByRole("switch", { name: "Unexpected computer changes" }))
  await user.click(settings.getByRole("switch", { name: "Failures" }))
  await user.click(settings.getByRole("switch", { name: "Failures" }))
  await user.click(settings.getByRole("switch", { name: "Long tasks finished" }))
  await user.click(settings.getByRole("switch", { name: "Enable notifications" }))
  expect(settings.getByRole("switch", { name: "Failures" })).toBeChecked()
  expect(settings.getByRole("switch", { name: "Failures" })).toBeDisabled()
  expect(store.getSnapshot().settings).toMatchObject({
    theme: "dark",
    launchAtLogin: false,
    startComputersAtLaunch: true,
    startupComputerIds: [source.computers.find(({ configuration }) => configuration.name === "playgrounds")!.configuration.id],
    terminal: "iTerm",
    editor: "Cursor",
    browser: "Firefox",
    reduceMotion: true,
    notificationsEnabled: false,
    notifyChanges: false,
    notifyFailures: true,
    notifyCompletions: false,
  })

  view.rerender(application(store, { ...source, preferences: { ...source.preferences, browser: "Google Chrome" } }))
  await user.click(navigation.getByRole("button", { name: "General" }))
  expect(settings.getByRole("combobox", { name: "Browser" })).toHaveTextContent("Firefox")
  expect(settings.getByRole("switch", { name: "Reduce motion" })).toBeChecked()
  view.unmount()

  render(application(store, source))
  const restored = settingsPanel()
  for (const [label, option] of [["Theme", "Dark"], ["Terminal", "iTerm"], ["Code editor", "Cursor"], ["Browser", "Firefox"]]) {
    expect(restored.getByRole("combobox", { name: label })).toHaveTextContent(option)
  }
  expect(restored.getByRole("switch", { name: "Launch Silo at login" })).not.toBeChecked()
  expect(restored.getByRole("switch", { name: "Start computers at launch" })).toBeChecked()
  expect(restored.queryByRole("button", { name: "Remove dev" })).not.toBeInTheDocument()
  expect(restored.getByRole("button", { name: "Remove playgrounds" })).toBeVisible()
  expect(restored.getByRole("switch", { name: "Reduce motion" })).toBeChecked()
  expect(screen.getByRole("region", { name: "Silo" })).toHaveAttribute("data-reduce-motion", "true")

  await user.click(within(screen.getByRole("navigation", { name: "Silo navigation" })).getByRole("button", { name: "Notifications" }))
  expect(restored.getByRole("switch", { name: "Enable notifications" })).not.toBeChecked()
  expect(restored.getByRole("switch", { name: "Unexpected computer changes" })).not.toBeChecked()
  expect(restored.getByRole("switch", { name: "Failures" })).toBeChecked()
  expect(restored.getByRole("switch", { name: "Long tasks finished" })).not.toBeChecked()
  await user.click(restored.getByRole("switch", { name: "Enable notifications" }))
  expect(restored.getByRole("switch", { name: "Unexpected computer changes" })).not.toBeChecked()
  expect(restored.getByRole("switch", { name: "Failures" })).toBeEnabled()
  expect(restored.getByRole("switch", { name: "Long tasks finished" })).not.toBeChecked()
})

it("retains saved startup IDs absent from telemetry and preserves an explicitly empty selection", async () => {
  const user = userEvent.setup()
  const source = applicationSourceForScenario("running")
  const store = createMemorySettingsStore({ ...source.preferences, startComputersAtLaunch: true, startupComputerIds: ["temporarily-unavailable"] })
  const view = render(application(store, source))
  const settings = settingsPanel()
  await settings.findByRole("switch", { name: "Start computers at launch" })
  expect(settings.queryByRole("button", { name: "Remove dev" })).not.toBeInTheDocument()
  await user.click(settings.getByRole("switch", { name: "Start computers at launch" }))
  await user.click(settings.getByRole("switch", { name: "Start computers at launch" }))
  await user.click(settings.getByRole("combobox", { name: "Add computer at startup" }))
  await user.click(screen.getByRole("option", { name: "dev" }))

  const restoredSource = {
    ...source,
    computers: [...source.computers, { ...source.computers[0], configuration: { ...source.computers[0].configuration, id: "temporarily-unavailable", name: "archive" } }],
  }
  view.rerender(application(store, restoredSource))
  expect(settings.getByRole("button", { name: "Remove archive" })).toBeVisible()
  expect(settings.getByRole("button", { name: "Remove dev" })).toBeVisible()
  await user.click(settings.getByRole("button", { name: "Clear" }))
  view.unmount()

  render(application(store, restoredSource))
  const restored = settingsPanel()
  expect(restored.getByRole("switch", { name: "Start computers at launch" })).toBeChecked()
  expect(restored.queryByRole("button", { name: "Remove dev" })).not.toBeInTheDocument()
  expect(restored.queryByRole("button", { name: "Remove archive" })).not.toBeInTheDocument()
})

it("honors an explicitly empty startup selection supplied by the initial source", async () => {
  const source = applicationSourceForScenario("running")
  render(<ApplicationPreview source={{ ...source, preferences: { ...source.preferences, startComputersAtLaunch: true, startupComputerIds: [] } }} initialRoute={{ tab: "settings" }} />)
  expect(await settingsPanel().findByRole("combobox", { name: "Add computer at startup" })).toBeVisible()
  expect(settingsPanel().queryByRole("button", { name: "Remove dev" })).not.toBeInTheDocument()
})

it.each(["native", "fixture"] as const)("updates unsaved startup defaults after the %s onboarding handoff without saving them", async (mode) => {
  const source = applicationSourceForScenario("running")
  const oldDev = source.computers.find(({ configuration }) => configuration.name === "dev")!.configuration.id
  const snapshot: SettingsSnapshot = { revision: 0, settings: {}, onboardingDraft: null, saveError: null }
  const write = vi.fn(async () => snapshot)
  const store = mode === "fixture"
    ? createFixtureSettingsStore({ ...source, preferences: { ...source.preferences, startComputersAtLaunch: true } })
    : createSettingsStore({
      read: async () => snapshot,
      subscribe: async () => () => {},
      updateSettings: write,
      updateOnboardingDraft: write,
      flush: async () => {},
    }, { ...source.preferences, startComputersAtLaunch: true, startupComputerIds: [oldDev] }, snapshot)
  const update = vi.spyOn(store, "updateSettings")
  const view = render(application(store, source))
  expect(await settingsPanel().findByRole("button", { name: "Remove dev" })).toBeVisible()

  const alpha = { ...source.computers[0], configuration: { ...source.computers[0].configuration, id: "onboarded-alpha", name: "alpha" } }
  view.rerender(application(store, { ...source, computers: [alpha] }))
  expect(settingsPanel().getByRole("button", { name: "Remove alpha" })).toBeVisible()
  expect(store.getSnapshot().settings.startupComputerIds).toEqual(["onboarded-alpha"])

  const dev = { ...source.computers[0], configuration: { ...source.computers[0].configuration, id: "onboarded-dev", name: "dev" } }
  view.rerender(application(store, { ...source, computers: [alpha, dev] }))
  expect(settingsPanel().getByRole("button", { name: "Remove dev" })).toBeVisible()
  expect(settingsPanel().queryByRole("button", { name: "Remove alpha" })).not.toBeInTheDocument()
  expect(store.getSnapshot().settings.startupComputerIds).toEqual(["onboarded-dev"])
  expect(update).not.toHaveBeenCalled()
  expect(write).not.toHaveBeenCalled()
})

it("puts the connections switch under Connections and preserves it across settings navigation", async () => {
  const user = userEvent.setup()
  const source = applicationSourceForScenario("running")
  source.connections = { enabled: false, deviceId: "office", name: "Office Mac", address: "owner@office" }
  const connectDevice = vi.fn().mockResolvedValue(undefined)
  const setConnectionsEnabled = vi.fn().mockResolvedValue(undefined)
  render(<ApplicationPreview source={source} actions={{ connectDevice, setConnectionsEnabled }} initialRoute={{ tab: "settings", settingsSection: "general" }} />)
  const navigation = within(screen.getByRole("navigation", { name: "Silo navigation" }))
  const settings = settingsPanel()
  expect(settings.queryByRole("switch", { name: "Allow connections from other devices" })).not.toBeInTheDocument()

  await user.click(navigation.getByRole("button", { name: "Connections" }))
  expect(navigation.getByRole("button", { name: "Connections" })).toHaveAttribute("aria-current", "page")
  expect(settings.getByRole("heading", { name: "Connections", level: 2 })).toBeVisible()
  expect(settings.queryByRole("combobox", { name: "Theme" })).not.toBeInTheDocument()
  await user.click(settings.getByRole("switch", { name: "Allow connections from other devices" }))
  expect(setConnectionsEnabled).toHaveBeenCalledWith(true)
  await user.click(settings.getByRole("button", { name: "Connect device…" }))
  await user.type(settings.getByRole("textbox", { name: "Device address" }), "owner@office")

  await user.click(navigation.getByRole("button", { name: "Notifications" }))
  expect(settings.queryByRole("switch", { name: "Allow connections from other devices" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Go back" }))
  expect(settings.getByRole("textbox", { name: "Device address" })).toHaveValue("owner@office")
  await user.click(settings.getByRole("button", { name: "Connect" }))
  expect(connectDevice).toHaveBeenCalledWith("owner@office")
})

it("opens Connections directly from the command palette", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} actions={{ connectDevice: vi.fn() }} />)
  await user.keyboard("{Control>}k{/Control}")
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "connections")
  await user.keyboard("{Enter}")
  expect(settingsPanel().getByRole("heading", { name: "Connections", level: 2 })).toBeVisible()
})
