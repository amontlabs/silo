import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { createSettingsStore, SettingsProvider, type SettingsSnapshot } from "@/features/preferences/settings-store"
import { SettingsSaveNotice } from "./settings-save-notice"

describe("settings save notice", () => {
  it("offers a confirmed reset when the settings file is write-protected", async () => {
    const protectedSnapshot: SettingsSnapshot = { revision: 1, settings: {}, onboardingDraft: null, saveError: "Settings could not be read.", writeProtected: true }
    const resetProtected = vi.fn().mockResolvedValue({ ...protectedSnapshot, revision: 2, saveError: null, writeProtected: false })
    const store = createSettingsStore({
      subscribe: async () => () => {}, read: async () => protectedSnapshot, flush: async () => {},
      updateSettings: async () => protectedSnapshot, updateOnboardingDraft: async () => protectedSnapshot, resetProtected,
    })
    await store.initialize()
    render(<SettingsProvider store={store}><SettingsSaveNotice /></SettingsProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Reset settings…" }))
    expect(resetProtected).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Reset settings" }))
    expect(resetProtected).toHaveBeenCalledOnce()
    await vi.waitFor(() => expect(screen.queryByRole("alert")).toBeNull())
    store.dispose()
  })
})
