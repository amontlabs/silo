import { act, fireEvent, screen } from "@testing-library/react"
import { expect, it, vi } from "vitest"

// The main window's entry point, booted for real with its native calls faked.
const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  production: {
    loadConfiguration: vi.fn(() => new Promise<void>(() => undefined)),
    initialize: vi.fn(async () => {}),
    drainSetup: vi.fn(async () => {}),
    dispose: vi.fn(),
  },
  integrations: { initialize: vi.fn(async () => {}), dispose: vi.fn() },
}))
vi.mock("../index.css", () => ({}))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: () => true }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "main" }) }))
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }))
vi.mock("react-dom/client", async () => {
  const { render } = await import("@testing-library/react")
  return { createRoot: () => {
    let view: ReturnType<typeof render> | undefined
    return { render: (node: React.ReactNode) => { if (view) view.rerender(node); else view = render(node) }, unmount: () => view?.unmount() }
  } }
})
vi.mock("./dependencies", () => ({ createNativeDependencyStore: () => ({ retry: vi.fn(), dispose: vi.fn(), getSnapshot: () => [], subscribe: () => () => {} }) }))
vi.mock("./production-source", () => ({ createProductionSource: () => native.production }))
vi.mock("./system-integrations", () => ({
  createDesktopSystemIntegrationStore: () => native.integrations, connectSystemIntegrationLifecycle: () => () => {},
}))
vi.mock("@/features/preferences/system-integrations-store", () => ({
  SystemIntegrationProvider: ({ children }: { children: React.ReactNode }) => children,
}))
vi.mock("@/features/preferences/theme", () => ({ initializeTheme: () => () => {} }))
vi.mock("./production-surface", () => ({
  StartupLoading: () => <p>Opening Silo…</p>,
  Unavailable: ({ message, retry, retryLabel }: { message: string; retry?: () => void; retryLabel?: string }) => <div role="alert">{message}{retry && <button onClick={retry}>{retryLabel}</button>}</div>,
  ProductionSurface: () => <p>Silo application</p>,
}))

it("paints at once, does not wait for the saved computer list, and retries a failed start", async () => {
  let releaseSettings!: () => void
  const settingsReady = new Promise<void>((resolve) => { releaseSettings = resolve })
  let failures = 1
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "initialize_settings") {
      await settingsReady
      if (failures-- > 0) throw new Error("settings lock unavailable")
      return {}
    }
    if (command === "read_settings") return { revision: 0, settings: { onboardingComplete: true }, onboardingDraft: null, saveError: null }
    if (command === "list_applications") return { terminal: [], editor: [], browser: [], defaults: {} }
    return undefined
  })
  await act(async () => { await import("../main") })
  // Nothing native has answered yet, but the shown window is not blank.
  expect(screen.getByText("Opening Silo…")).toBeInTheDocument()
  // Independent steps start together rather than one after another.
  expect(native.production.loadConfiguration).toHaveBeenCalledOnce()
  expect(native.integrations.initialize).toHaveBeenCalledOnce()
  // Live state loads while the settings do.
  expect(native.production.initialize).toHaveBeenCalledOnce()
  await act(async () => { releaseSettings() })
  expect(await screen.findByRole("alert")).toHaveTextContent("Silo startup failed: settings lock unavailable. No computer state changed.")
  fireEvent.click(screen.getByRole("button", { name: "Retry" }))
  // The saved list never answered: it only feeds the loading skeleton, so it cannot block startup.
  expect(await screen.findByText("Silo application")).toBeInTheDocument()
  expect(native.production.initialize).toHaveBeenCalledOnce()
  expect(native.invoke.mock.calls.filter(([command]) => command === "initialize_settings")).toHaveLength(2)
})
