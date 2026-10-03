import { act, render, screen, waitFor } from "@testing-library/react"
import { expect, it, vi } from "vitest"
import { ApplicationPreview } from "@/fixtures/application-preview"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { UpdatesProvider, type UpdateBackend, type UpdateSnapshot } from "@/features/updates/update-store"
import type { AppMenuState } from "@/desktop/app-menu"

const menu = vi.hoisted(() => ({ receive: (_command: string) => {}, state: {} as AppMenuState }))
vi.mock("@/desktop/app-menu", () => ({ useAppMenu: (state: AppMenuState, receive: (command: string) => void) => {
  menu.state = state; menu.receive = receive; return true
} }))

it("routes native navigation and uses the real update controller only on request", async () => {
  const state: UpdateSnapshot = { phase: "idle", currentVersion: "0.1.0", lastChecked: null,
    retryAction: null, availableVersion: null, releaseNotes: null, downloadedBytes: 0, totalBytes: null,
    automaticChecks: true, packageKind: "macos", releaseUrl: "https://github.com/amontlabs/silo/releases",
    error: null, errorDetails: null, installBlockReason: null, runningComputers: [], canInstall: true }
  const backend: UpdateBackend = { read: async () => state, subscribe: async () => () => {},
    check: vi.fn(async () => ({ ...state, phase: "error" as const, error: "The update service is unavailable. Try again later." })),
    download: vi.fn(), install: vi.fn(), setAutomaticChecks: vi.fn(), openRelease: vi.fn() }
  render(<UpdatesProvider backend={backend}><ApplicationPreview source={applicationSourceForScenario("running")} /></UpdatesProvider>)
  await waitFor(() => expect(menu.state.canCheckUpdates).toBe(true))
  expect(backend.check).not.toHaveBeenCalled()
  act(() => menu.receive("go-secrets"))
  expect(screen.getByRole("region", { name: "Secrets" })).toBeVisible()
  act(() => menu.receive("check-updates"))
  expect(await screen.findByText("The update service is unavailable. Try again later.")).toBeVisible()
  expect(backend.check).toHaveBeenCalledTimes(1)
  expect(backend.install).not.toHaveBeenCalled()
  act(() => menu.receive("go-back"))
  expect(screen.getByRole("region", { name: "Secrets" })).toBeVisible()
})

it("opens the command palette through the menu without double handling its native shortcut", async () => {
  render(<ApplicationPreview source={applicationSourceForScenario("running")} />)
  act(() => menu.receive("search"))
  expect(await screen.findByRole("dialog", { name: "Commands" })).toBeVisible()
  act(() => menu.receive("search"))
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Commands" })).not.toBeInTheDocument())
})
