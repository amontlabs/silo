import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { createMemorySettingsStore, createSettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import type { ProductionSource } from "./production-source"
import type { DependencyStore } from "./dependencies"
import { ProductionSurface, StartupLoading } from "./production-surface"

const native = vi.hoisted(() => ({ invoke: vi.fn(async (_command: string, _args?: unknown): Promise<unknown> => undefined) }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: () => false }))
vi.mock("./shutdown-boundary", () => ({ ShutdownBoundary: ({ children, pendingWork }: { children: import("react").ReactNode; pendingWork?: string }) => <>{pendingWork && <p>Quit overlay: {pendingWork}</p>}{children}</> }))
vi.mock("./runtime-migration-boundary", () => ({ RuntimeMigrationBoundary: ({ children }: { children: import("react").ReactNode }) => children }))
const state = vi.hoisted(() => ({ source: {} as object | null, loading: false, error: null as string | null, checks: [] as Array<{ id: string; title: string; status: string; detail: string; remediation: string | null }>, retry: vi.fn(), setupDrain: undefined as string | undefined, localUpdating: false }))
vi.mock("./production-source", () => ({ localUpdatingNotice: "Local computers are updating.", useProductionSource: () => ({ source: state.source, backup: {}, loading: state.loading, error: state.error, setupDrain: state.setupDrain, localUpdating: state.localUpdating, savedConfigurations: [{ id: "saved", name: "saved-computer", kind: "ssh", host: "host", user: "user", port: 22 }] }) }))
vi.mock("./dependencies", () => ({ useDependencyStore: () => ({ checks: state.checks, retry: state.retry }) }))
const engine = vi.hoisted(() => ({ handlers: new Set<() => void>() }))
vi.mock("./engine-connection", () => ({ onEngineConnected: (handler: () => void) => { engine.handlers.add(handler); return () => { engine.handlers.delete(handler) } } }))
vi.mock("./production-onboarding", () => ({ ProductionOnboarding: ({ onOpenApp }: { onOpenApp: () => void }) => <button onClick={onOpenApp}>Open Silo</button> }))
vi.mock("@/features/application/application-app", () => ({ ApplicationApp: ({ source, actions }: { source: { runtimeRepair?: { reason: string; recovery: string; checking: boolean } }; actions: { retryRuntimeChecks: () => void } }) => <div>Main app{source.runtimeRepair && <div role="alert">{source.runtimeRepair.reason}{source.runtimeRepair.recovery}<button disabled={source.runtimeRepair.checking} onClick={actions.retryRuntimeChecks}>Retry checks</button></div>}</div> }))
vi.mock("./status-panel", () => ({ StatusPanel: ({ notice }: { notice?: string }) => <div>Status panel{notice && <p>Tray notice: {notice}</p>}</div> }))

const source = { applicationActions: {}, statusActions: {}, refresh: vi.fn(), initialize: vi.fn(() => Promise.resolve()) } as unknown as ProductionSource
const dependencyStore = {} as DependencyStore

beforeEach(() => { state.source = {}; state.loading = false; state.error = null; state.checks = []; state.setupDrain = undefined; state.localUpdating = false; engine.handlers.clear(); vi.clearAllMocks() })

describe("production completion routing", () => {
  it("shows the actual shell and saved rows while live state loads, then replaces skeletons", async () => {
    state.source = null
    state.loading = true
    const settings = createMemorySettingsStore({ onboardingComplete: true, reduceMotion: true })
    const view = () => <SettingsProvider store={settings}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>
    const application = render(view())
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    expect(screen.queryByText("Silo could not load")).not.toBeInTheDocument()
    expect(screen.getByRole("navigation", { name: "Silo navigation" })).toBeVisible()
    expect(screen.getByText("saved-computer")).toBeVisible()
    expect(screen.getByText("Loading computer state")).toHaveClass("sr-only")
    expect(screen.getByRole("button", { name: "Add" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Search or jump to" })).toBeDisabled()
    expect(application.container.querySelector(".animate-pulse")).toBeNull()
    state.loading = false
    state.source = {}
    await act(async () => { application.rerender(view()) })
    expect(screen.getByText("Main app")).toBeVisible()
    expect(screen.queryByText("Loading computer state")).not.toBeInTheDocument()
  })

  it("does not disguise a real startup failure as a skeleton", () => {
    state.source = null
    state.error = "Runtime inspection failed."
    render(<SettingsProvider store={createMemorySettingsStore({ onboardingComplete: true })}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>)
    expect(screen.getByRole("alert")).toHaveTextContent("Runtime inspection failed.")
    expect(screen.getByRole("button", { name: "Retry checks" })).toBeEnabled()
  })

  it("keeps onboarding mounted after Finish saves completion until Open Silo", async () => {
    const settings = createMemorySettingsStore()
    render(<SettingsProvider store={settings}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>)
    expect(screen.getByRole("button", { name: "Open Silo" })).toBeVisible()
    await act(async () => { await settings.updateSettings({ onboardingComplete: true }) })
    expect(screen.getByRole("button", { name: "Open Silo" })).toBeVisible()
    expect(screen.queryByText("Main app")).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(screen.getByText("Main app")).toBeVisible()
  })

  it.each([
    ["could not be read", { read: () => Promise.reject(new Error("settings unavailable")) }],
    ["are damaged and write-protected", { read: async () => ({ revision: 0, settings: {}, onboardingDraft: null, saveError: "Settings could not be read; the file was left unchanged.", writeProtected: true }) }],
  ])("never routes to onboarding while settings %s", async (_case, backend) => {
    const settings = createSettingsStore({
      subscribe: async () => () => {},
      updateSettings: () => Promise.reject(new Error("unused")),
      updateOnboardingDraft: () => Promise.reject(new Error("unused")),
      flush: async () => {},
      ...backend,
    })
    vi.spyOn(console, "error").mockImplementation(() => {})
    await settings.initialize()
    expect(settings.getSnapshot().settings.onboardingComplete).toBe(false)
    render(<SettingsProvider store={settings}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>)
    expect(screen.getByText("Main app")).toBeVisible()
    expect(screen.queryByRole("button", { name: "Open Silo" })).not.toBeInTheDocument()
  })

  it("opens the main app when relaunched after saved completion", async () => {
    const settings = createMemorySettingsStore({ onboardingComplete: true })
    await act(async () => { render(<SettingsProvider store={settings}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>) })
    expect(screen.getByText("Main app")).toBeVisible()
    expect(screen.queryByRole("button", { name: "Open Silo" })).not.toBeInTheDocument()
  })

  it("names the setup work Quit is draining in the main window's shutdown overlay", async () => {
    state.setupDrain = "Finishing setup (verifying GitHub access)…"
    await act(async () => { render(<SettingsProvider store={createMemorySettingsStore({ onboardingComplete: true })}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>) })
    expect(screen.getByText("Quit overlay: Finishing setup (verifying GitHub access)…")).toBeVisible()
  })

  it("keeps the status window outside onboarding", () => {
    const settings = createMemorySettingsStore()
    render(<SettingsProvider store={settings}><ProductionSurface source={source} dependencyStore={null} statusPanel /></SettingsProvider>)
    expect(screen.getByText("Status panel")).toBeVisible()
    expect(screen.queryByRole("button", { name: "Open Silo" })).not.toBeInTheDocument()
  })
})


describe("status panel without application state", () => {
  it("shows a panel-sized error with Retry, Open Silo and Quit instead of the full window", async () => {
    state.source = null
    state.error = "Runtime inspection failed."
    render(<SettingsProvider store={createMemorySettingsStore({ onboardingComplete: true })}><ProductionSurface source={source} dependencyStore={null} statusPanel /></SettingsProvider>)
    const panel = screen.getByRole("dialog", { name: "Silo" })
    expect(panel).toHaveClass("status-panel")
    expect(screen.queryByRole("region", { name: "Silo unavailable" })).not.toBeInTheDocument()
    expect(screen.getByRole("alert")).toHaveTextContent("Runtime inspection failed.")
    fireEvent.click(screen.getByRole("button", { name: "Retry" }))
    expect(source.initialize).toHaveBeenCalledOnce()
    fireEvent.click(screen.getByRole("button", { name: "Open Silo" }))
    fireEvent.click(screen.getByRole("button", { name: "Quit Silo" }))
    await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("quit_app"))
    expect(native.invoke).toHaveBeenCalledWith("open_main")
  })
})

describe("local computers updating while connected devices are shown", () => {
  it("explains the missing local computers in the main window and the tray", () => {
    state.source = { computers: [], devices: [{ id: "office", connected: true }] }
    state.localUpdating = true
    const settings = createMemorySettingsStore({ onboardingComplete: true })
    const main = render(<SettingsProvider store={settings}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>)
    expect(screen.getByText("Main app")).toBeVisible()
    expect(screen.getByRole("status")).toHaveTextContent("Local computers are updating.")
    main.unmount()
    render(<SettingsProvider store={settings}><ProductionSurface source={source} dependencyStore={null} statusPanel /></SettingsProvider>)
    expect(screen.getByText("Tray notice: Local computers are updating.")).toBeVisible()
  })
})

describe("status panel while application state loads", () => {
  it("shows loading instead of an empty inventory before saved computers are read", () => {
    render(<SettingsProvider store={createMemorySettingsStore({ onboardingComplete: true })}><StartupLoading statusPanel /></SettingsProvider>)
    expect(screen.getByText("Loading computers…")).toBeVisible()
    expect(screen.queryByText("No computers yet")).not.toBeInTheDocument()
    expect(screen.queryByText("Add your first computer in Silo.")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Open Silo" })).toBeEnabled()
  })

  it("keeps Open Silo and Quit usable in the tray skeleton", async () => {
    state.source = null
    state.loading = true
    render(<SettingsProvider store={createMemorySettingsStore({ onboardingComplete: true })}><ProductionSurface source={source} dependencyStore={null} statusPanel /></SettingsProvider>)
    expect(screen.getByText("Loading computer state")).toHaveClass("sr-only")
    expect(screen.getByText("saved-computer")).toBeVisible()
    expect(screen.getByRole("button", { name: "Open Silo" })).toBeEnabled()
    fireEvent.click(screen.getByRole("button", { name: "Quit Silo" }))
    await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("quit_app"))
  })
})

describe("production dependency recovery", () => {
  const failure = { id: "runtime-microsandbox", title: "MicroSandbox runtime", status: "unavailable", detail: "Bundled runtime is missing.", remediation: "Reinstall Silo. Keep your computers and settings." }
  it("shows recovery even when runtime failure prevents reading application state", () => {
    state.source = null
    state.checks = [failure]
    render(<SettingsProvider store={createMemorySettingsStore({ onboardingComplete: true })}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>)
    expect(screen.getByRole("alert")).toHaveTextContent(failure.remediation)
    fireEvent.click(screen.getByRole("button", { name: "Retry checks" }))
    expect(state.retry).toHaveBeenCalledOnce()
    // Retry starts live updates again (a failed subscription is retried), not just one read.
    expect(source.initialize).toHaveBeenCalledOnce()
    expect(screen.queryByRole("button", { name: /repair/i })).not.toBeInTheDocument()
  })
  it("does not block remote-only use when this device lacks virtualization", async () => {
    state.source = { computers: [], devices: [{ id: "office", connected: true }] }
    state.checks = [failure]
    await act(async () => { render(<SettingsProvider store={createMemorySettingsStore({ onboardingComplete: true })}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>) })
    expect(screen.getByText("Main app")).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })
  it("retains recovery while rechecking and clears the issue only after passing", async () => {
    state.checks = [failure]
    const settings = createMemorySettingsStore({ onboardingComplete: true })
    const view = () => <SettingsProvider store={settings}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>
    let application!: ReturnType<typeof render>
    await act(async () => { application = render(view()) })
    expect(screen.getByRole("alert")).toHaveTextContent(failure.detail)
    fireEvent.click(screen.getByRole("button", { name: "Retry checks" }))
    expect(state.retry).toHaveBeenCalledOnce()
    state.checks = [{ ...failure, status: "pending" }]
    await act(async () => { application.rerender(view()) })
    expect(screen.getByRole("alert")).toHaveTextContent(failure.remediation)
    expect(screen.getByRole("button", { name: "Retry checks" })).toBeDisabled()
    state.checks = [{ ...failure, status: "pass" }]
    await act(async () => { application.rerender(view()) })
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    state.checks = [{ ...failure, status: "pending" }]
    await act(async () => { application.rerender(view()) })
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })
})

describe("engine connection", () => {
  const failure = { id: "runtime-microsandbox", title: "Computer runtime", status: "unavailable", detail: "Dependency checks are unavailable because the desktop bridge did not respond.", remediation: "Retry checks." }
  const surface = () => <SettingsProvider store={createMemorySettingsStore({ onboardingComplete: true })}><ProductionSurface source={source} dependencyStore={dependencyStore} /></SettingsProvider>
  const connect = () => act(async () => { engine.handlers.forEach(handler => handler()) })

  it("runs the failed checks and the load again when the engine becomes reachable", async () => {
    state.checks = [failure]
    await act(async () => { render(surface()) })
    await connect()
    expect(state.retry).toHaveBeenCalledOnce()
    expect(source.initialize).toHaveBeenCalledOnce()
  })
  it("loads the application again when the engine appears after Silo could not load", async () => {
    state.source = null
    state.error = "Silo's engine is not reachable yet. Retry in a moment."
    await act(async () => { render(surface()) })
    await connect()
    expect(source.initialize).toHaveBeenCalledOnce()
  })
  it("leaves a healthy application alone when the engine reconnects", async () => {
    state.checks = [{ ...failure, status: "pass" }]
    await act(async () => { render(surface()) })
    await connect()
    expect(state.retry).not.toHaveBeenCalled()
    expect(source.initialize).not.toHaveBeenCalled()
  })
  it("checks again when the engine appears while a check is still running", async () => {
    state.checks = [{ ...failure, status: "pending" }]
    await act(async () => { render(surface()) })
    await connect()
    expect(state.retry).toHaveBeenCalledOnce()
  })
  describe("without an engine event", () => {
    // Each timer's state update renders between steps, as it does in the app.
    const advance = async (ms: number) => { for (let elapsed = 0; elapsed < ms; elapsed += 1_000) await act(async () => { await vi.advanceTimersByTimeAsync(Math.min(1_000, ms - elapsed)) }) }
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })

    it("keeps loading again after the engine connected too early, until Silo loads", async () => {
      state.source = null
      state.error = "Silo's engine is not reachable yet. Retry in a moment."
      const view = render(surface())
      await act(async () => {})
      await connect()
      expect(source.initialize).toHaveBeenCalledTimes(1)
      await advance(30_000)
      expect(vi.mocked(source.initialize).mock.calls.length).toBeGreaterThan(2)
      const tries = vi.mocked(source.initialize).mock.calls.length
      state.source = {}
      state.error = null
      await act(async () => { view.rerender(surface()) })
      await advance(60_000)
      expect(source.initialize).toHaveBeenCalledTimes(tries)
    })
    it("runs the failed checks again on its own until the runtime answers", async () => {
      state.checks = [failure]
      const view = render(surface())
      await act(async () => {})
      await advance(30_000)
      expect(vi.mocked(state.retry).mock.calls.length).toBeGreaterThan(2)
      const tries = vi.mocked(state.retry).mock.calls.length
      state.checks = [{ ...failure, status: "pass" }]
      await act(async () => { view.rerender(surface()) })
      await advance(60_000)
      expect(state.retry).toHaveBeenCalledTimes(tries)
    })
    it("waits while a check is running instead of starting another", async () => {
      state.checks = [{ ...failure, status: "pending" }]
      await act(async () => { render(surface()) })
      await advance(60_000)
      expect(state.retry).not.toHaveBeenCalled()
    })
    it("gives up after a bounded number of tries and starts again on the next engine event", async () => {
      state.source = null
      state.error = "Silo's engine is not reachable yet. Retry in a moment."
      await act(async () => { render(surface()) })
      await advance(30 * 60_000)
      const bound = vi.mocked(source.initialize).mock.calls.length
      expect(bound).toBeGreaterThan(3)
      expect(bound).toBeLessThan(60)
      await advance(30 * 60_000)
      expect(source.initialize).toHaveBeenCalledTimes(bound)
      await connect()
      expect(source.initialize).toHaveBeenCalledTimes(bound + 1)
      await advance(30_000)
      expect(vi.mocked(source.initialize).mock.calls.length).toBeGreaterThan(bound + 2)
    })
    it("does not poll a healthy application", async () => {
      await act(async () => { render(surface()) })
      await advance(60_000)
      expect(state.retry).not.toHaveBeenCalled()
      expect(source.initialize).not.toHaveBeenCalled()
    })
  })
  it("stops listening when the surface goes away", async () => {
    state.checks = [failure]
    const view = render(surface())
    await act(async () => { view.unmount() })
    expect(engine.handlers.size).toBe(0)
  })
})
