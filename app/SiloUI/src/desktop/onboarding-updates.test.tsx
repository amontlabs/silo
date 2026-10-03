import { act, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"
import type { UpdateSnapshot } from "@/features/updates/update-store"
import type { ProductionSource } from "./production-source"
import type { DependencyStore } from "./dependencies"
import { ProductionSurface } from "./production-surface"

const backend = vi.hoisted(() => ({ read: vi.fn(), subscribe: vi.fn(), install: vi.fn(), download: vi.fn(), openRelease: vi.fn() }))
vi.mock("./updates", () => ({ desktopUpdateBackend: backend }))
vi.mock("./shutdown-boundary", () => ({ ShutdownBoundary: ({ children }: { children: import("react").ReactNode }) => children }))
vi.mock("./runtime-migration-boundary", () => ({ RuntimeMigrationBoundary: ({ children }: { children: import("react").ReactNode }) => children }))
vi.mock("./use-main-route", () => ({ useMainRoute: () => undefined }))
vi.mock("./editor-include", () => ({ desktopEditorIncludeBackend: { read: async () => null, subscribe: async () => () => {} } }))
vi.mock("./production-source", async () => {
  const { applicationSourceForScenario } = await import("@/fixtures/application-scenarios")
  const { useUnavailableBackup } = await import("@/fixtures/application-backup")
  const source = applicationSourceForScenario("running")
  return { useProductionSource: () => ({ source, backup: useUnavailableBackup(source), loading: false }) }
})
vi.mock("./dependencies", () => {
  const dependencies = { checks: [], retry: vi.fn() }
  return { useDependencyStore: () => dependencies }
})
vi.mock("./production-onboarding", async () => {
  const { OnboardingPreview } = await import("@/fixtures/onboarding-preview")
  const { onboardingScenarios } = await import("@/fixtures/scenarios")
  return { ProductionOnboarding: ({ onOpenApp }: { onOpenApp: () => void }) => <OnboardingPreview source={onboardingScenarios.running} onOpenApp={onOpenApp} /> }
})

const available: UpdateSnapshot = {
  phase: "available", lastChecked: null, retryAction: null, currentVersion: "0.2.1", availableVersion: "0.2.2",
  releaseNotes: null, downloadedBytes: 0, totalBytes: null, automaticChecks: true, packageKind: "macos",
  releaseUrl: "https://github.com/amontlabs/silo/releases", error: null, errorDetails: null,
  installBlockReason: null, runningComputers: [], canInstall: true,
}
const source = { applicationActions: {}, statusActions: {} } as unknown as ProductionSource
function mount(onboardingComplete = false) {
  const store = createMemorySettingsStore({ onboardingComplete })
  render(<SettingsProvider store={store}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(store)}><ProductionSurface source={source} dependencyStore={{} as DependencyStore} /></SystemIntegrationProvider></SettingsProvider>)
  return { store, user: userEvent.setup() }
}
beforeEach(() => {
  vi.resetAllMocks()
  backend.read.mockResolvedValue(available)
  backend.subscribe.mockResolvedValue(() => {})
  backend.openRelease.mockResolvedValue(undefined)
  backend.download.mockResolvedValue({ ...available, phase: "ready" })
  backend.install.mockResolvedValue({ ...available, phase: "installing" })
})

it("shows the update during unfinished setup and downloads without losing the current step", async () => {
  const { user, store } = mount()
  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  expect(await screen.findByText("Silo 0.2.2 is available.")).toBeVisible()
  await user.click(screen.getByRole("button", { name: "View update" }))
  const dialog = within(screen.getByRole("dialog", { name: "Silo updates" }))
  await user.click(dialog.getByRole("button", { name: "Download update" }))
  expect(backend.download).toHaveBeenCalledOnce()
  await user.keyboard("{Escape}")
  expect(screen.getByRole("tab", { name: /Computers/ })).toHaveAttribute("aria-selected", "true")
  expect(store.getSnapshot().settings.onboardingComplete).toBe(false)
})

it("offers the manual package action during onboarding for system packages", async () => {
  backend.read.mockResolvedValue({ ...available, packageKind: "manual" })
  const { user } = mount()
  await user.click(await screen.findByRole("button", { name: "View update" }))
  await user.click(screen.getByRole("button", { name: "View installers on GitHub" }))
  expect(backend.openRelease).toHaveBeenCalledOnce()
  expect(backend.download).not.toHaveBeenCalled()
})

it("flushes onboarding settings before installing a ready update", async () => {
  backend.read.mockResolvedValue({ ...available, phase: "ready" })
  const { user, store } = mount()
  const order: string[] = []
  vi.spyOn(store, "flush").mockImplementation(async () => { order.push("flush") })
  backend.install.mockImplementation(async () => { order.push("install"); return { ...available, phase: "installing" } })
  await user.click(await screen.findByRole("button", { name: "View update" }))
  await user.click(screen.getByRole("button", { name: "Restart and update" }))
  expect(order).toEqual(["flush", "install"])
  expect(backend.install).toHaveBeenCalledWith(false)
  expect(screen.getByRole("navigation", { name: "Setup steps" }).closest("[inert]")).not.toBeNull()
  const status = screen.getAllByRole("status").find(element => element.textContent === "Installing update. Silo will restart…")
  expect(status).toBeVisible()
  expect(status?.closest("[inert]")).toBeNull()
})

it("protects the unfinished setup while flushing and restores it when saving fails", async () => {
  backend.read.mockResolvedValue({ ...available, phase: "ready" })
  const { user, store } = mount()
  let failFlush!: (error: Error) => void
  vi.spyOn(store, "flush").mockReturnValue(new Promise<void>((_, reject) => { failFlush = reject }))
  await user.click(await screen.findByRole("button", { name: "View update" }))
  await user.click(screen.getByRole("button", { name: "Restart and update" }))
  const steps = screen.getByRole("navigation", { name: "Setup steps" })
  expect(steps.closest("[inert]")).not.toBeNull()
  expect(screen.getByText("Preparing update…").closest("[inert]")).toBeNull()
  await act(async () => failFlush(new Error("disk full")))
  expect(steps.closest("[inert]")).toBeNull()
  expect(screen.queryByText("Preparing update…")).not.toBeInTheDocument()
  expect(backend.install).not.toHaveBeenCalled()
  expect(store.getSnapshot().settings.onboardingComplete).toBe(false)
})

it("dismisses the onboarding notice without finishing setup", async () => {
  const { user, store } = mount()
  await user.click(await screen.findByRole("button", { name: "Dismiss update notice" }))
  expect(screen.queryByRole("button", { name: "View update" })).not.toBeInTheDocument()
  expect(screen.getByRole("navigation", { name: "Setup steps" })).toBeVisible()
  expect(store.getSnapshot().settings.onboardingComplete).toBe(false)
})

it("keeps exactly one update notice after opening the main app", async () => {
  mount(true)
  expect(screen.getByRole("navigation", { name: "Silo navigation" })).toBeVisible()
  expect(await screen.findAllByRole("button", { name: "View update" })).toHaveLength(1)
})
