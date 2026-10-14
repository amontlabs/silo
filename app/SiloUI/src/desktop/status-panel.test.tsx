import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { StatusBarActions } from "@/features/status-bar/status-bar-types"
import { StatusPanel } from "./status-panel"
import { createMemorySettingsStore, SettingsProvider, type SettingsStore } from "@/features/preferences/settings-store"

const native = vi.hoisted(() => ({ invoke: vi.fn().mockResolvedValue(undefined), listen: vi.fn(), opened: () => {}, menu: vi.fn(), popup: vi.fn().mockResolvedValue(undefined), close: vi.fn().mockResolvedValue(undefined) }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }))
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }))
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: native.menu } }))
beforeEach(() => {
  vi.clearAllMocks()
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }))
  native.menu.mockResolvedValue({ popup: native.popup, close: native.close })
  native.listen.mockImplementation((name, callback) => { if (name === "desktop:status-opened") native.opened = callback; return Promise.resolve(vi.fn()) })
})

function setup(store?: SettingsStore, reactStrictMode = false) {
  const actions: StatusBarActions = {
    openSilo: vi.fn(), quit: vi.fn(), refresh: vi.fn(), startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn(), openTerminal: vi.fn(), openEditor: vi.fn(), openSite: vi.fn(), pushRepository: vi.fn(), dismissRepositoryPush: vi.fn(),
  }
  const panel = <StatusPanel source={applicationSourceForScenario("running")} actions={actions} />
  const view = render(store ? <SettingsProvider store={store}>{panel}</SettingsProvider> : panel, { reactStrictMode })
  return { ...view, actions, user: userEvent.setup() }
}

it("ignores status-open events from disposed listeners during StrictMode registration", async () => {
  const opened: Array<() => void> = []
  const register: Array<(stop: () => void) => void> = []
  native.listen.mockImplementation((name, callback) => {
    if (name !== "desktop:status-opened") return Promise.resolve(vi.fn())
    opened.push(callback)
    return new Promise<() => void>(resolve => { register.push(resolve) })
  })
  const view = setup(undefined, true)
  const focus = vi.spyOn(screen.getByRole("dialog", { name: "Silo" }), "focus")
  expect(opened).toHaveLength(2)
  act(() => { opened[0]() })
  expect(focus).not.toHaveBeenCalled()
  const staleStop = vi.fn()
  const liveStop = vi.fn()
  await act(async () => { register[0](staleStop); register[1](liveStop) })
  expect(staleStop).toHaveBeenCalledOnce()
  act(() => { opened[1]() })
  expect(focus).toHaveBeenCalledOnce()
  view.unmount()
  act(() => { opened[1]() })
  expect(focus).toHaveBeenCalledOnce()
  expect(liveStop).toHaveBeenCalledOnce()
})

it("handles a rejected status-open listener registration", async () => {
  const error = new Error("Listener unavailable")
  const log = vi.spyOn(console, "error").mockImplementation(() => {})
  native.listen.mockImplementation(name => name === "desktop:status-opened" ? Promise.reject(error) : Promise.resolve(vi.fn()))
  setup()
  await waitFor(() => expect(log).toHaveBeenCalledWith("Silo status events:", error))
})

it("propagates resolved apps to shortcuts, native menus, and an already-open folder picker", async () => {
  const store = createMemorySettingsStore()
  store.updateDefaults({ editor: "Zed", editorPath: "/Applications/Zed.app", terminal: "Ghostty" })
  const { user } = setup(store)
  expect(screen.getByRole("button", { name: "Open dev in Ghostty" })).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Actions for dev" }))
  expect(native.menu.mock.calls[0][0].items).toEqual(expect.arrayContaining([
    expect.objectContaining({ text: "Open in Zed…" }),
    expect.objectContaining({ text: "Open in Ghostty" }),
  ]))
  await user.click(screen.getByRole("button", { name: "Open dev in Zed" }))
  expect(screen.getByRole("button", { name: "Open in Zed" })).toBeVisible()
  await act(async () => { await store.updateSettings({ editor: "Cursor", editorUseSystemDefault: false }) })
  expect(screen.getByRole("button", { name: "Open in Cursor" })).toBeVisible()
  act(() => native.opened())
  expect(screen.getByRole("button", { name: "Open dev in Cursor" })).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Actions for dev" }))
  expect(native.menu.mock.lastCall![0].items).toEqual(expect.arrayContaining([
    expect.objectContaining({ text: "Open in Cursor…" }),
  ]))
})

it("uses the real quit command and keeps the existing status content", async () => {
  const { user, actions } = setup()
  expect(screen.getByRole("list", { name: "Computers" })).toBeVisible()
  expect(screen.queryByRole("button", { name: "Silo status bar" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Quit Silo" }))
  expect(native.invoke).not.toHaveBeenCalledWith("quit_app", expect.anything())
  await user.click(screen.getByRole("button", { name: "Keep running" }))
  expect(native.invoke).toHaveBeenCalledWith("quit_app", { choice: "keep" })
  expect(actions.quit).not.toHaveBeenCalled()
})

it("updates the native tray when computer health changes and skips unchanged health", async () => {
  const actions: StatusBarActions = {
    openSilo: vi.fn(), quit: vi.fn(), refresh: vi.fn(), startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn(), openTerminal: vi.fn(), openEditor: vi.fn(), openSite: vi.fn(), pushRepository: vi.fn(), dismissRepositoryPush: vi.fn(),
  }
  const source = applicationSourceForScenario("running")
  const { rerender } = render(<StatusPanel source={source} actions={actions} />)
  expect(native.invoke).toHaveBeenCalledWith("update_tray", { tone: "success", label: "Ready" })
  const trayCalls = () => native.invoke.mock.calls.filter(([command]) => command === "update_tray")
  expect(trayCalls()).toHaveLength(1)
  rerender(<StatusPanel source={structuredClone(source)} actions={actions} />)
  expect(trayCalls()).toHaveLength(1)
  const failed = structuredClone(source)
  failed.computers[0].state = "failed"
  rerender(<StatusPanel source={failed} actions={actions} />)
  expect(native.invoke).toHaveBeenCalledWith("update_tray", { tone: "error", label: "Computer error" })
  expect(trayCalls()).toHaveLength(2)
})

it("uses an OS popup and retains stop confirmation in the panel", async () => {
  const { user, actions } = setup()
  await user.click(screen.getByRole("button", { name: "Actions for dev" }))
  expect(native.popup).toHaveBeenCalledOnce()
  expect(screen.queryByRole("menu")).not.toBeInTheDocument()
  const items = native.menu.mock.calls[0][0].items
  act(() => items.find((item: { text: string }) => item.text === "Stop…").action())
  expect(actions.stopComputer).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Stop" }))
  expect(actions.stopComputer).toHaveBeenCalledWith("dev")
  expect(native.close).toHaveBeenCalledOnce()
})

it("routes a native site selection through panel dismissal", async () => {
  const { user, actions } = setup()
  await user.click(screen.getByRole("button", { name: "Actions for dev" }))
  const sites = native.menu.mock.calls[0][0].items.find((item: { text: string }) => item.text === "Open in browser")
  expect(sites.enabled).toBe(true)
  const port = sites.items.find((item: { text?: string }) => item.text?.startsWith("Port "))
  expect(port).toBeDefined()
  await act(async () => port.action())
  expect(native.invoke).toHaveBeenCalledWith("hide_status")
  expect(actions.openSite).toHaveBeenCalledWith("dev", Number(port.text.slice(5)))
})

it("reports native menu failure and allows retry", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {})
  native.popup.mockRejectedValueOnce(new Error("Popup failed"))
  const { user } = setup()
  await user.click(screen.getByRole("button", { name: "Actions for dev" }))
  expect(await screen.findByText("Could not open computer actions")).toBeInTheDocument()
  expect(screen.getByText("Popup failed")).toBeInTheDocument()
  expect(native.close).toHaveBeenCalledOnce()
  await user.click(screen.getByRole("button", { name: "Retry" }))
  expect(native.popup).toHaveBeenCalledTimes(2)
  error.mockRestore()
})

it("cancels quit confirmation before Escape dismisses the status panel", async () => {
  const { user } = setup()
  await user.click(screen.getByRole("button", { name: "Quit Silo" }))
  expect(screen.getByRole("alertdialog", { name: "Quit Silo?" })).toBeVisible()
  native.invoke.mockClear()
  await user.keyboard("{Escape}")
  expect(screen.queryByRole("alertdialog", { name: "Quit Silo?" })).not.toBeInTheDocument()
  expect(native.invoke).not.toHaveBeenCalledWith("hide_status")
  await user.keyboard("{Escape}")
  expect(native.invoke).toHaveBeenCalledWith("hide_status")
})

it("dismisses with Escape after native menu tracking ends", async () => {
  const { user } = setup()
  await user.click(screen.getByRole("button", { name: "Actions for dev" }))
  await user.keyboard("{Escape}")
  expect(native.invoke).toHaveBeenCalledWith("hide_status")
})

it("returns to computer rows when the status item is opened again", async () => {
  const { user } = setup()
  await user.click(screen.getByRole("button", { name: "Open dev in Visual Studio Code" }))
  expect(screen.queryByRole("button", { name: "Quit Silo" })).not.toBeInTheDocument()
  act(() => native.opened())
  expect(screen.getByRole("button", { name: "Quit Silo" })).toBeVisible()
})

it.each([false, true])("keeps focus and natural page sizing through folder navigation with Reduce Motion %s", async (reduceMotion) => {
  const { user } = setup(createMemorySettingsStore({ reduceMotion }))
  const panel = screen.getByRole("dialog", { name: "Silo" })
  await user.click(screen.getByRole("button", { name: "Open dev in Visual Studio Code" }))
  expect(panel.querySelector<HTMLDivElement>(".status-page")!.style.height).toBe("")
  expect(screen.getByRole("button", { name: "Back to computers" })).toHaveFocus()
  expect(panel).toHaveAttribute("data-reduce-motion", String(reduceMotion))
  await user.type(screen.getByRole("textbox", { name: "Filter folders" }), "no-matching-folder")
  expect(panel.querySelector<HTMLDivElement>(".status-page")!.style.height).toBe("")
  await user.click(screen.getByRole("button", { name: "Back to computers" }))
  expect(panel.querySelector<HTMLDivElement>(".status-page")!.style.height).toBe("")
  expect(panel).toHaveFocus()
  expect(screen.getByRole("button", { name: "Quit Silo" })).toBeVisible()
  act(() => native.opened())
  expect(panel.querySelector<HTMLDivElement>(".status-page")!.style.height).toBe("")
})
