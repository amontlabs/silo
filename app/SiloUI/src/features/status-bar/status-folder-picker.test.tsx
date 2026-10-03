import { act, render, screen } from "@testing-library/react"
import { StrictMode } from "react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { DirectoryPage } from "@/features/application/model/directory-store"
import { StatusFolderPicker } from "./status-folder-picker"

const computer = applicationSourceForScenario("running").computers[0]
const page = (names: string[], parent = "/workspace", nextOffset: number | null = null): DirectoryPage => ({
  snapshotId: parent, entries: names.map(name => ({ name, path: `${parent}/${name}`, kind: "folder" })), nextOffset,
})
function setup(loader = vi.fn().mockResolvedValue(page([]))) {
  const onOpen = vi.fn()
  const props = { computer, editor: "Cursor", onBack: vi.fn(), onOpen, listDirectory: loader }
  return { user: userEvent.setup(), loader, props, onOpen, ...render(<StatusFolderPicker {...props} />) }
}
describe("status folder picker live directories", () => {
  it("offers no file transfer actions", async () => {
    setup(vi.fn().mockResolvedValue({ snapshotId: "s", entries: [{ name: "src", path: "/workspace/src", kind: "folder" }, { name: "a.txt", path: "/workspace/a.txt", kind: "file" }], nextOffset: null }))
    await screen.findByText("src")
    expect(screen.queryByRole("button", { name: /Upload|Download/ })).toBeNull()
  })

  it("reveals complete sanitized folder names and the computer heading", async () => {
    const name = `${"folder-".repeat(30)}\u202E`
    setup(vi.fn().mockResolvedValue(page([name])))
    const displayed = name.replace("\u202E", "⟨U+202E⟩")
    expect(await screen.findByText(displayed)).toHaveAttribute("title", displayed)
    const heading = `${computer.configuration.name} folders`
    expect(screen.getByRole("heading", { name: heading })).toHaveAttribute("title", heading)
  })

  it("loads lazily, opens the exact path, and keeps cached folders on return", async () => {
    let resolve!: (value: DirectoryPage) => void
    const loader = vi.fn().mockImplementationOnce(() => new Promise<DirectoryPage>(done => { resolve = done })).mockResolvedValueOnce(page(["nested"], "/workspace/project"))
    const { user, onOpen } = setup(loader)
    expect(screen.getByRole("status", { name: "Loading folders" })).toBeVisible()
    expect(screen.getByRole("button", { name: "Open in Cursor" })).toBeDisabled()
    expect(loader).toHaveBeenCalledExactlyOnceWith(computer.configuration.name, "/workspace", 0, undefined)
    await act(async () => resolve(page(["project"])))
    await user.click(screen.getByRole("button", { name: "project" }))
    expect(await screen.findByRole("button", { name: "nested" })).toBeVisible()
    expect(loader).toHaveBeenLastCalledWith(computer.configuration.name, "/workspace/project", 0, undefined)
    await user.click(screen.getByRole("button", { name: "Open in Cursor" }))
    expect(onOpen).toHaveBeenCalledExactlyOnceWith("/workspace/project")
    loader.mockImplementation(() => new Promise(() => {}))
    await user.click(screen.getByRole("button", { name: "/workspace" }))
    expect(screen.getByRole("button", { name: "project" })).toBeVisible()
    expect(screen.queryByRole("status", { name: "Loading folders" })).not.toBeInTheDocument()
  })
  it("keeps a compact error stable during retry without raw runtime output", async () => {
    let resolve!: (value: DirectoryPage) => void
    const loader = vi.fn().mockRejectedValueOnce(new Error("private runtime details")).mockImplementationOnce(() => new Promise<DirectoryPage>(done => { resolve = done }))
    const { user } = setup(loader)
    expect(await screen.findByRole("alert")).toHaveTextContent(/^Could not load this folder\.$/)
    expect(screen.getByRole("button", { name: "Open in Cursor" })).toBeDisabled()
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1)
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(screen.getByRole("alert")).toHaveTextContent(/^Could not load this folder\.$/)
    expect(screen.queryByRole("status", { name: "Loading folders" })).not.toBeInTheDocument()
    await act(async () => resolve(page([])))
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    expect(screen.getByText("No subfolders here")).toBeVisible()
    expect(screen.getByRole("button", { name: "Open in Cursor" })).toBeEnabled()
  })
  it("keeps opening the shown folder when only a background refresh fails", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      const loader = vi.fn().mockResolvedValueOnce(page(["project"])).mockRejectedValue(new Error("temporary"))
      const { onOpen } = setup(loader)
      await act(async () => {})
      expect(screen.getByRole("button", { name: "project" })).toBeVisible()
      await act(async () => vi.advanceTimersByTime(10_000))
      expect(await screen.findByRole("alert")).toHaveTextContent("Could not refresh. Showing previous folders.")
      const open = screen.getByRole("button", { name: "Open in Cursor" })
      expect(open).toBeEnabled()
      act(() => open.click())
      expect(onOpen).toHaveBeenCalledExactlyOnceWith("/workspace")
    } finally { vi.useRealTimers() }
  })
  it("reveals hidden characters in folder names but opens the real path", async () => {
    const spoofed = "photos\u202Egpj.exe"
    const { user, onOpen, loader } = setup(vi.fn().mockResolvedValueOnce(page([spoofed])).mockResolvedValue(page([], `/workspace/${spoofed}`)))
    const folder = await screen.findByRole("button", { name: "photos⟨U+202E⟩gpj.exe" })
    await user.click(folder)
    expect(loader).toHaveBeenLastCalledWith(computer.configuration.name, `/workspace/${spoofed}`, 0, undefined)
    expect(screen.getByRole("navigation", { name: "Folder path" })).toHaveTextContent("photos⟨U+202E⟩gpj.exe")
    await user.click(screen.getByRole("button", { name: "Open in Cursor" }))
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(`/workspace/${spoofed}`)
  })
  it.each(["stopped", "stale"])("does not load or open a %s computer", (state) => {
    const loader = vi.fn()
    render(<StatusFolderPicker computer={{ ...computer, ...(state === "stopped" ? { state: "stopped" } : { freshness: "stale" }) }} editor="Cursor" onBack={vi.fn()} onOpen={vi.fn()} listDirectory={loader} />)
    expect(screen.getByText(state === "stopped" ? "Start this computer to browse its files." : "Reconnect to browse files.")).toBeVisible()
    expect(loader).not.toHaveBeenCalled()
    expect(screen.getByRole("button", { name: "Open in Cursor" })).toBeDisabled()
  })
  it("pages beyond files without a false empty state and filters loaded folders", async () => {
    const loader = vi.fn().mockResolvedValueOnce({ ...page([], "/workspace", 200), entries: [{ name: "readme", path: "/workspace/readme", kind: "file" }] }).mockResolvedValueOnce(page(["later", "another"]))
    const { user } = setup(loader)
    await screen.findByRole("button", { name: "Load more" })
    expect(screen.queryByText("No subfolders here")).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "readme" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Load more" }))
    expect(await screen.findByRole("button", { name: "later" })).toBeVisible()
    expect(loader).toHaveBeenLastCalledWith(computer.configuration.name, "/workspace", 200, "/workspace")
    await user.type(screen.getByRole("textbox", { name: "Filter folders" }), "LATE")
    expect(screen.getByRole("button", { name: "later" })).toBeVisible()
    expect(screen.queryByRole("button", { name: "another" })).not.toBeInTheDocument()
  })
  it("never falls back to fixture files without a loader", async () => {
    render(<StatusFolderPicker computer={computer} editor="Cursor" onBack={vi.fn()} onOpen={vi.fn()} />)
    expect(await screen.findByRole("alert")).toHaveTextContent("Files are unavailable.")
    expect(screen.queryByRole("button", { name: "projects" })).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Open in Cursor" })).toBeDisabled()
  })
  it("backs off failed reads, refreshes on focus, and restores the healthy interval", async () => {
    vi.useFakeTimers()
    try {
      const loader = vi.fn().mockRejectedValue(new Error("unavailable"))
      const { unmount } = setup(loader)
      await act(async () => {})
      let calls = 1
      for (const delay of [20_000, 40_000, 60_000, 60_000]) {
        await act(async () => vi.advanceTimersByTimeAsync(delay - 1))
        expect(loader).toHaveBeenCalledTimes(calls)
        await act(async () => vi.advanceTimersByTimeAsync(1))
        expect(loader).toHaveBeenCalledTimes(++calls)
      }
      loader.mockResolvedValue(page([]))
      await act(async () => window.dispatchEvent(new Event("focus")))
      expect(loader).toHaveBeenCalledTimes(++calls)
      await act(async () => vi.advanceTimersByTimeAsync(10_000))
      expect(loader).toHaveBeenCalledTimes(++calls)
      unmount()
      await act(async () => vi.advanceTimersByTimeAsync(60_000))
      expect(loader).toHaveBeenCalledTimes(calls)
    } finally { vi.useRealTimers() }
  })
  it("does not poll after the status panel loses focus or unmounts", async () => {
    vi.useFakeTimers()
    try {
      const { loader, unmount } = setup()
      await act(async () => {})
      act(() => window.dispatchEvent(new Event("blur")))
      await act(async () => vi.advanceTimersByTime(20_000))
      expect(loader).toHaveBeenCalledTimes(1)
      act(() => window.dispatchEvent(new Event("focus")))
      await act(async () => {})
      expect(loader).toHaveBeenCalledTimes(2)
      unmount()
      await act(async () => vi.advanceTimersByTime(20_000))
      expect(loader).toHaveBeenCalledTimes(2)
    } finally { vi.useRealTimers() }
  })
  it("cancels queued directory reads when the folder picker closes", async () => {
    const waiting: ((value: DirectoryPage) => void)[] = []
    const loader = vi.fn().mockResolvedValueOnce(page(["a", "b", "c"]))
      .mockImplementation(() => new Promise<DirectoryPage>(resolve => { waiting.push(resolve) }))
    const { user, unmount } = setup(loader)
    await user.click(await screen.findByRole("button", { name: "a" }))
    await user.click(screen.getByRole("button", { name: "/workspace" }))
    await user.click(screen.getByRole("button", { name: "b" }))
    await user.click(screen.getByRole("button", { name: "/workspace" }))
    await user.click(screen.getByRole("button", { name: "c" }))
    expect(loader.mock.calls.map(call => call[1])).toEqual(["/workspace", "/workspace/a", "/workspace", "/workspace/b"])
    unmount()
    await act(async () => waiting[0](page([], "/workspace/a")))
    expect(loader.mock.calls.map(call => call[1])).not.toContain("/workspace/c")
    await act(async () => waiting.slice(1).forEach(resolve => resolve(page([]))))
  })
  it("loads folders after StrictMode replays cleanup", async () => {
    const loader = vi.fn().mockResolvedValue(page(["project"]))
    render(<StrictMode><StatusFolderPicker computer={computer} editor="Cursor" onBack={vi.fn()} onOpen={vi.fn()} listDirectory={loader} /></StrictMode>)
    expect(await screen.findByRole("button", { name: "project" })).toBeVisible()
    expect(screen.getByRole("button", { name: "Open in Cursor" })).toBeEnabled()
  })
})
