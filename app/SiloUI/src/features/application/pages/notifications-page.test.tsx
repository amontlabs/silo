import { act, fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { NotificationsPage } from "./notifications-page"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import { createSystemIntegrationStore, SystemIntegrationProvider, type NotificationStatus, type SystemIntegrations } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"

function page() {
  const store = createMemorySettingsStore({ notificationsEnabled: true })
  return { store, ui: <SettingsProvider store={store}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(store)}><NotificationsPage /></SystemIntegrationProvider></SettingsProvider> }
}

it("describes the three categories and when notifications appear", async () => {
  render(page().ui)
  expect(await screen.findByText("Silo sends system notifications while its window is in the background. While you're using Silo, results appear in the app.")).toBeInTheDocument()
  expect(screen.getByText("Actions and background work that fail, such as start, push, computer export, or computer import.")).toBeInTheDocument()
  expect(screen.getByText("A computer stops, fails, or recovers without you asking.")).toBeInTheDocument()
  expect(screen.getByText("Work that took more than a few seconds finishes while Silo is in the background.")).toBeInTheDocument()
  for (const name of ["Failures", "Unexpected computer changes", "Long tasks finished"]) expect(screen.getByRole("switch", { name })).toBeChecked()
})

it("saves each category under its own setting", async () => {
  const user = userEvent.setup()
  const { store, ui } = page()
  render(ui)
  await user.click(await screen.findByRole("switch", { name: "Long tasks finished" }))
  await user.click(screen.getByRole("switch", { name: "Failures" }))
  expect(store.getSnapshot().settings).toMatchObject({ notifyCompletions: false, notifyFailures: false, notifyChanges: true })
})

it("disables blocked notifications without losing category choices and restores them after authorization", async () => {
  const settings = createMemorySettingsStore({ notificationsEnabled: true, notifyFailures: false })
  const initial: SystemIntegrations = { platform: "macos", loginItem: { state: "enabled", error: null }, notifications: { state: "denied", error: null } }
  const openSettings = vi.fn(async () => {})
  const requestNotifications = vi.fn(async (): Promise<NotificationStatus> => ({ state: "authorized", error: null }))
  const integrations = createSystemIntegrationStore({
    read: async () => ({ ...initial, notifications: { state: "authorized", error: null } }),
    setLoginItem: async () => initial.loginItem, requestNotifications, openSettings, showError: async () => {},
  }, settings, initial)
  render(<SettingsProvider store={settings}><SystemIntegrationProvider store={integrations}><NotificationsPage /></SystemIntegrationProvider></SettingsProvider>)
  expect(screen.getByRole("switch", { name: "Enable notifications" })).not.toBeChecked()
  for (const name of ["Failures", "Unexpected computer changes", "Long tasks finished"]) expect(screen.getByRole("switch", { name })).toBeDisabled()
  fireEvent.click(screen.getByRole("button", { name: "Open System Settings" }))
  expect(openSettings).toHaveBeenCalledExactlyOnceWith("notifications")

  await act(() => integrations.refresh())

  expect(screen.queryByText("Blocked in System Settings")).not.toBeInTheDocument()
  expect(screen.getByRole("switch", { name: "Enable notifications" })).toBeChecked()
  expect(screen.getByRole("switch", { name: "Failures" })).toBeEnabled()
  expect(screen.getByRole("switch", { name: "Failures" })).not.toBeChecked()
  expect(settings.getSnapshot().settings).toMatchObject({ notificationsEnabled: true, notifyFailures: false, notifyChanges: true, notifyCompletions: true })
  expect(requestNotifications).not.toHaveBeenCalled()
})

it("waits for verified permission before enabling categories and prevents duplicate authorization requests", async () => {
  const settings = createMemorySettingsStore({ notificationsEnabled: false, notifyCompletions: false })
  const initial: SystemIntegrations = { platform: "macos", loginItem: { state: "enabled", error: null }, notifications: { state: "notDetermined", error: null } }
  let permit!: (status: NotificationStatus) => void
  const requestNotifications = vi.fn(() => new Promise<NotificationStatus>(resolve => { permit = resolve }))
  const integrations = createSystemIntegrationStore({
    read: async () => initial, setLoginItem: async () => initial.loginItem,
    requestNotifications, openSettings: async () => {}, showError: async () => {},
  }, settings, initial)
  render(<SettingsProvider store={settings}><SystemIntegrationProvider store={integrations}><NotificationsPage /></SystemIntegrationProvider></SettingsProvider>)
  const enable = screen.getByRole("switch", { name: "Enable notifications" })
  await act(async () => fireEvent.click(enable))
  expect(enable).toBeDisabled()
  expect(enable).not.toBeChecked()
  expect(screen.getByRole("switch", { name: "Failures" })).toBeDisabled()
  fireEvent.click(enable)
  expect(requestNotifications).toHaveBeenCalledOnce()

  await act(async () => permit({ state: "provisional", error: null }))

  expect(enable).toBeEnabled()
  expect(enable).toBeChecked()
  expect(screen.getByRole("switch", { name: "Failures" })).toBeEnabled()
  expect(screen.getByRole("switch", { name: "Long tasks finished" })).not.toBeChecked()
  expect(settings.getSnapshot().settings.notificationsEnabled).toBe(true)
})

it("has a switch for requests from agents, on by default, saved under its own setting", async () => {
  const user = userEvent.setup()
  const { store, ui } = page()
  render(ui)
  expect(await screen.findByText("An agent in a computer asks for something only you can allow, such as a merge or a published port.")).toBeInTheDocument()
  const requests = screen.getByRole("switch", { name: "Requests from agents" })
  expect(requests).toBeChecked()
  await user.click(requests)
  expect(store.getSnapshot().settings).toMatchObject({ notifyApprovals: false, notifyFailures: true })
})
