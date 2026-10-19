import { fireEvent, screen, waitFor } from "@testing-library/react"
import { expect, it, vi } from "vitest"

const native = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock("../index.css", () => ({}))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: () => true }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "status" }) }))
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }))
vi.mock("react-dom/client", async () => {
  const { render } = await import("@testing-library/react")
  // One root, as in production: later renders replace the startup shell.
  return { createRoot: () => {
    let view: ReturnType<typeof render> | undefined
    return { render: (node: React.ReactNode) => { if (view) view.rerender(node); else view = render(node) }, unmount: () => view?.unmount() }
  } }
})
vi.mock("./dependencies", () => ({ createNativeDependencyStore: vi.fn() }))
vi.mock("./production-source", () => ({ createProductionSource: () => ({
  loadConfiguration: async () => {}, initialize: async () => {}, watchNetwork: () => () => {}, drainSetup: async () => {},
}) }))
vi.mock("./system-integrations", () => ({
  createDesktopSystemIntegrationStore: () => ({}), connectSystemIntegrationLifecycle: vi.fn(),
}))
vi.mock("@/features/preferences/system-integrations-store", () => ({
  SystemIntegrationProvider: ({ children }: { children: React.ReactNode }) => children,
}))
vi.mock("@/features/preferences/theme", () => ({ initializeTheme: () => () => {} }))
vi.mock("./production-surface", async () => {
  const { useSettings } = await import("@/features/preferences/settings-store")
  return {
    Unavailable: ({ message }: { message: string }) => <div>{message}</div>,
    StartupLoading: () => <p>Opening Silo…</p>,
    ProductionSurface: () => {
      const { settings } = useSettings()
      return <output>{JSON.stringify(settings)}</output>
    },
  }
})

it("boots the actual status entry with discovered apps and refreshes defaults without replacing an explicit choice", async () => {
  let editor = { name: "Zed", path: "/Applications/Zed.app" }
  native.invoke.mockImplementation(async (command) => {
    if (command === "read_settings") return {
      revision: 0, settings: { terminal: "Custom Terminal", terminalPath: "/Applications/Custom.app", terminalUseSystemDefault: false },
      onboardingDraft: null, saveError: null,
    }
    if (command === "list_applications") return {
      terminal: [{ name: "Ghostty", path: "/Applications/Ghostty.app" }], editor: [editor],
      browser: [{ name: "Zen", path: "/Applications/Zen.app" }],
      defaults: { terminal: "/Applications/Ghostty.app", editor: editor.path, browser: "/Applications/Zen.app" },
    }
    throw new Error(`Unexpected command: ${command}`)
  })
  // The tree mounts, then reads the applications on its own: those updates arrive outside any act scope.
  const environment = globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  environment.IS_REACT_ACT_ENVIRONMENT = false
  await import("../main")
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent('"editor":"Zed"'))
  expect(screen.getByRole("status")).toHaveTextContent('"browser":"Zen"')
  expect(screen.getByRole("status")).toHaveTextContent('"terminal":"Custom Terminal"')
  editor = { name: "Cursor", path: "/Applications/Cursor.app" }
  fireEvent.focus(window)
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent('"editor":"Cursor"'))
  expect(screen.getByRole("status")).toHaveTextContent('"editorPath":"/Applications/Cursor.app"')
  expect(screen.getByRole("status")).toHaveTextContent('"terminal":"Custom Terminal"')
  expect(native.invoke.mock.calls.map(([command]) => command)).not.toContain("update_settings")
  environment.IS_REACT_ACT_ENVIRONMENT = true
})
