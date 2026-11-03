import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { ComputerUseSettings } from "./computer-use-settings"
import { createComputerUseBridge, type ComputerUseBackend } from "@/desktop/computer-use-bridge"
import { ComputerUseProvider } from "@/desktop/computer-use-provider"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"

describe("New computer approval default", () => {
  function withBridge(store = createMemorySettingsStore()) {
    const backend: ComputerUseBackend = { readDesktopState: async () => ({}), setApproval: async () => ({}), setup: async () => ({}) }
    render(<SettingsProvider store={store}><ComputerUseProvider bridge={createComputerUseBridge(backend)}>
      <ComputerUseSettings />
    </ComputerUseProvider></SettingsProvider>)
    return store
  }

  it("is off by default and saves the choice", async () => {
    const store = withBridge()
    const toggle = screen.getByRole("switch", { name: "Allow agents to use the desktop without asking in new computers" })
    expect(toggle).not.toBeChecked()
    expect(screen.getByText("Claude Code, Codex and similar agents stop asking before using the computer’s desktop. Not a security boundary.")).toBeVisible()
    fireEvent.click(toggle)
    await waitFor(() => expect(store.getSnapshot().settings.computerUseAutoApproval).toBe(true))
    expect(toggle).toBeChecked()
  })

  it("is not offered when this build has no built-in computer use", () => {
    render(<ComputerUseSettings />)
    expect(screen.queryByRole("switch", { name: /without asking/ })).not.toBeInTheDocument()
  })
})
