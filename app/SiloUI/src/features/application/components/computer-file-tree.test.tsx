import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { ComputerFileTree } from "./computer-file-tree"
import type { FileTransferControls } from "./use-file-transfers"
import { createDirectoryStore, type DirectoryPage } from "../model/directory-store"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"

const computer = applicationSourceForScenario("running").computers[0]
const page = (name: string, kind: "file" | "folder" | "symlink" = "file"): DirectoryPage => ({
  entries: [{ name, path: `/workspace/${name}`, kind }], nextOffset: null, snapshotId: "test",
})

describe("live file tree", () => {
  it("reveals complete sanitized names when tree labels are truncated", async () => {
    const names = ["folder-".repeat(30), "file-".repeat(30), "link-\u202E".repeat(20)]
    const kinds = ["folder", "file", "symlink"] as const
    const store = createDirectoryStore(vi.fn().mockResolvedValue({
      entries: names.map((name, index) => ({ name, path: `/workspace/${name}`, kind: kinds[index] })),
      nextOffset: null, snapshotId: "long-names",
    }))
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active />)
    for (const name of names) {
      const displayed = name.replaceAll("\u202E", "⟨U+202E⟩")
      expect(await screen.findByText(displayed)).toHaveAttribute("title", displayed)
    }
    expect(screen.getByText(computer.configuration.name)).toHaveAttribute("title", computer.configuration.name)
  })

  it("opens and copies exact folder paths without toggling expansion", async () => {
    const user = userEvent.setup()
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined)
    const onOpenEditor = vi.fn()
    const store = createDirectoryStore(vi.fn().mockResolvedValue(page("src", "folder")))
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active onOpenEditor={onOpenEditor} />)
    const folder = await screen.findByRole("button", { name: "Folder src" })
    const row = within(folder.parentElement!)
    await user.click(row.getByRole("button", { name: "Open in Cursor" }))
    expect(onOpenEditor).toHaveBeenCalledWith(computer.configuration.name, "/workspace/src")
    expect(folder).toHaveAttribute("aria-expanded", "false")
    await user.click(row.getByRole("button", { name: "Copy path" }))
    expect(writeText).toHaveBeenCalledWith("/workspace/src")
    expect(row.getByRole("button", { name: "Path copied" })).toBeInTheDocument()
    expect(folder).toHaveAttribute("aria-expanded", "false")
    const root = screen.getByRole("button", { name: computer.configuration.name })
    await user.click(within(root.parentElement!).getByRole("button", { name: "Open in Cursor" }))
    expect(onOpenEditor).toHaveBeenLastCalledWith(computer.configuration.name, "/workspace")
  })

  it("reveals a soft hyphen in a folder label while opening its exact original path", async () => {
    const user = userEvent.setup()
    const onOpenEditor = vi.fn()
    const store = createDirectoryStore(vi.fn().mockResolvedValue(page("con\u00ADfig", "folder")))
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active onOpenEditor={onOpenEditor} />)
    const folder = await screen.findByRole("button", { name: "Folder con⟨U+00AD⟩fig" })
    await user.click(within(folder.parentElement!).getByRole("button", { name: "Open in Cursor" }))
    expect(onOpenEditor).toHaveBeenCalledWith(computer.configuration.name, "/workspace/con\u00ADfig")
  })

  it("reveals an invisible Unicode tag while copying the original folder path", async () => {
    const user = userEvent.setup()
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined)
    const store = createDirectoryStore(vi.fn().mockResolvedValue(page("con\u{E0061}fig", "folder")))
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active onOpenEditor={vi.fn()} />)
    const folder = await screen.findByRole("button", { name: "Folder con⟨U+E0061⟩fig" })
    await user.click(within(folder.parentElement!).getByRole("button", { name: "Copy path" }))
    expect(writeText).toHaveBeenCalledWith("/workspace/con\u{E0061}fig")
  })

  it("shows skeletons, lazily opens folders and immediately reuses cached contents", async () => {
    const user = userEvent.setup()
    let resolve!: (value: DirectoryPage) => void
    const loader = vi.fn()
      .mockImplementationOnce(() => new Promise<DirectoryPage>((done) => { resolve = done }))
      .mockResolvedValue({ entries: [{ name: "hello.txt", path: "/workspace/src/hello.txt", kind: "file" }], nextOffset: null, snapshotId: "child" })
    const store = createDirectoryStore(loader)
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active />)
    expect(screen.getByRole("status", { name: "Loading folder" })).toBeVisible()
    await act(async () => resolve(page("src", "folder")))
    expect(loader).toHaveBeenCalledTimes(1)
    await user.click(screen.getByRole("button", { name: "Folder src" }))
    expect(await screen.findByText("hello.txt")).toBeVisible()
    expect(loader).toHaveBeenLastCalledWith(computer.configuration.name, "/workspace/src", 0, undefined)
    await user.click(screen.getByRole("button", { name: "Folder src" }))
    loader.mockImplementation(() => new Promise(() => {}))
    await user.click(screen.getByRole("button", { name: "Folder src" }))
    expect(screen.getByText("hello.txt")).toBeVisible()
    expect(screen.queryByRole("status", { name: "Loading folder" })).not.toBeInTheDocument()
  })

  it("does not request files for stopped, stale or hidden computers", () => {
    const loader = vi.fn()
    const store = createDirectoryStore(loader)
    const { rerender } = render(<ComputerFileTree editor="Cursor" computer={{ ...computer, state: "stopped" }} store={store} active />)
    expect(screen.getByText("Start this computer to browse its files.")).toBeVisible()
    rerender(<ComputerFileTree editor="Cursor" computer={{ ...computer, freshness: "stale" }} store={store} active />)
    expect(screen.getByText("Reconnect to browse files.")).toBeVisible()
    rerender(<ComputerFileTree editor="Cursor" computer={computer} store={store} active={false} />)
    expect(loader).not.toHaveBeenCalled()
  })

  it("distinguishes empty folders, permission errors and links without following them", async () => {
    const user = userEvent.setup()
    const loader = vi.fn().mockRejectedValueOnce("Permission denied.").mockResolvedValueOnce({ entries: [], nextOffset: null, snapshotId: "empty" })
    const store = createDirectoryStore(loader)
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active />)
    expect(await screen.findByRole("alert")).toHaveTextContent("Permission denied.")
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(await screen.findByText("Empty folder.")).toBeVisible()
    loader.mockResolvedValue(page("shortcut", "symlink"))
    await act(async () => { await store.load(computer.configuration.name, "/workspace", { refresh: true }) })
    expect(screen.getByLabelText("Symbolic link")).toBeVisible()
    expect(screen.queryByRole("button", { name: /shortcut/ })).not.toBeInTheDocument()
  })

  it("keeps errors stable during automatic refresh and clears them after recovery", async () => {
    let resolve!: (value: DirectoryPage) => void
    const loader = vi.fn().mockRejectedValueOnce(new Error("private runtime details"))
      .mockImplementationOnce(() => new Promise<DirectoryPage>((done) => { resolve = done }))
    const store = createDirectoryStore(loader)
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active />)
    expect(await screen.findByRole("alert")).toHaveTextContent(/^Could not load this folder\.$/)
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1)
    act(() => { window.dispatchEvent(new Event("focus")) })
    expect(screen.getByRole("alert")).toHaveTextContent(/^Could not load this folder\.$/)
    expect(screen.queryByRole("status", { name: "Loading folder" })).not.toBeInTheDocument()
    await act(async () => resolve(page("recovered.txt")))
    expect(screen.getByText("recovered.txt")).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })

  it("backs off failing folders while healthy visible folders keep refreshing", async () => {
    vi.useFakeTimers()
    let failing = false
    const loader = vi.fn(async (_computer: string, path: string): Promise<DirectoryPage> => {
      if (path === "/workspace" && failing) throw new Error("Folder unavailable")
      return path === "/workspace" ? page("src", "folder") : { entries: [], nextOffset: null, snapshotId: "src" }
    })
    const store = createDirectoryStore(loader)
    const view = render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active />)
    const advance = async (ms: number) => { await act(() => vi.advanceTimersByTimeAsync(ms)) }
    const reads = (path: string) => loader.mock.calls.filter(([, loaded]) => loaded === path).length
    try {
      await advance(0)
      fireEvent.click(screen.getByRole("button", { name: "Folder src" }))
      await advance(0)
      failing = true
      await advance(10000)
      expect(reads("/workspace")).toBe(2)
      for (const delay of [20000, 40000, 60000, 60000]) {
        const rootReads = reads("/workspace")
        const childReads = reads("/workspace/src")
        await advance(delay - 1)
        expect(reads("/workspace")).toBe(rootReads)
        await advance(1)
        expect(reads("/workspace")).toBe(rootReads + 1)
        expect(reads("/workspace/src")).toBe(childReads + delay / 10000)
      }
      failing = false
      await advance(60000)
      const rootReads = reads("/workspace")
      await advance(10000)
      expect(reads("/workspace")).toBe(rootReads + 1)
      view.unmount()
      const calls = loader.mock.calls.length
      await advance(60000)
      expect(loader).toHaveBeenCalledTimes(calls)
    } finally { view.unmount(); vi.useRealTimers() }
  })

  it("polls the whole visible tree from its root instead of once per expanded folder", async () => {
    const user = userEvent.setup()
    const loader = vi.fn(async (_computer: string, path: string): Promise<DirectoryPage> => path === "/workspace"
      ? { entries: [{ name: "src", path: "/workspace/src", kind: "folder" }, { name: "docs", path: "/workspace/docs", kind: "folder" }], nextOffset: null, snapshotId: "root" }
      : path === "/workspace/src"
        ? { entries: [{ name: "lib", path: "/workspace/src/lib", kind: "folder" }], nextOffset: null, snapshotId: "src" }
        : { entries: [], nextOffset: null, snapshotId: path })
    const intervals = vi.spyOn(window, "setInterval")
    const listeners = vi.spyOn(window, "addEventListener")
    const store = createDirectoryStore(loader)
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active />)
    await user.click(await screen.findByRole("button", { name: "Folder src" }))
    await user.click(await screen.findByRole("button", { name: "Folder lib" }))
    await user.click(screen.getByRole("button", { name: "Folder src" }))
    await waitFor(() => expect(screen.queryByRole("button", { name: "Folder lib" })).not.toBeInTheDocument())
    expect(intervals.mock.calls.filter(([, delay]) => delay === 10_000)).toHaveLength(1)
    expect(listeners.mock.calls.filter(([type]) => type === "focus")).toHaveLength(1)

    loader.mockClear()
    await act(async () => { window.dispatchEvent(new Event("focus")) })
    // Only the root is visible: src is collapsed, so neither it nor lib is refreshed.
    expect(loader.mock.calls.map(([, path]) => path)).toEqual(["/workspace"])
    intervals.mockRestore()
    listeners.mockRestore()
  })

  it("refreshes each visible folder on the root's timer and stops while hidden", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      const loader = vi.fn(async (_computer: string, path: string): Promise<DirectoryPage> => path === "/workspace"
        ? { entries: [{ name: "src", path: "/workspace/src", kind: "folder" }], nextOffset: null, snapshotId: "root" }
        : { entries: [], nextOffset: null, snapshotId: "src" })
      const store = createDirectoryStore(loader)
      const view = render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active />)
      fireEvent.click(await screen.findByRole("button", { name: "Folder src" }))
      await screen.findByText("Empty folder.")
      loader.mockClear()
      await act(async () => { await vi.advanceTimersByTimeAsync(10_000) })
      expect(loader.mock.calls.map(([, path]) => path).sort()).toEqual(["/workspace", "/workspace/src"])

      loader.mockClear()
      view.rerender(<ComputerFileTree editor="Cursor" computer={computer} store={store} active={false} />)
      await act(async () => { await vi.advanceTimersByTimeAsync(30_000) })
      act(() => { window.dispatchEvent(new Event("focus")) })
      expect(loader).not.toHaveBeenCalled()
    } finally { vi.useRealTimers() }
  })

  it("appends skeletons while paging without hiding current files", async () => {
    const user = userEvent.setup()
    let resolve!: (value: DirectoryPage) => void
    const loader = vi.fn().mockResolvedValueOnce({ ...page("first"), nextOffset: 200 }).mockImplementationOnce(() => new Promise<DirectoryPage>((done) => { resolve = done }))
    const store = createDirectoryStore(loader)
    render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active />)
    await user.click(await screen.findByRole("button", { name: "Load more" }))
    expect(screen.getByText("first")).toBeVisible()
    expect(screen.getByRole("status", { name: "Loading folder" })).toBeVisible()
    await act(async () => resolve(page("second")))
    await waitFor(() => expect(screen.getByText("second")).toBeVisible())
    expect(screen.getByText("first")).toBeVisible()
  })

  describe("file transfer actions", () => {
    const controls = (overrides: Partial<FileTransferControls> = {}): FileTransferControls => ({ busy: false, upload: vi.fn(), download: vi.fn(), ...overrides })
    const entries = { entries: [
      { name: "src", path: "/workspace/src", kind: "folder" as const },
      { name: "a b.txt", path: "/workspace/a b.txt", kind: "file" as const },
      { name: "link", path: "/workspace/link", kind: "symlink" as const },
    ], nextOffset: null, snapshotId: "t" }

    it("uploads into the root and into a folder row, and downloads a file row with its exact path", async () => {
      const user = userEvent.setup()
      const transfers = controls()
      const store = createDirectoryStore(vi.fn().mockResolvedValue(entries))
      render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active onOpenEditor={vi.fn()} transfers={transfers} />)
      const folder = await screen.findByRole("button", { name: "Folder src" })
      await user.click(within(folder.parentElement!).getByRole("button", { name: "Upload files here" }))
      expect(transfers.upload).toHaveBeenCalledWith(computer.configuration.name, "/workspace/src", computer.configuration.name)
      const root = screen.getByRole("button", { name: computer.configuration.name })
      await user.click(within(root.parentElement!).getByRole("button", { name: "Upload files here" }))
      expect(transfers.upload).toHaveBeenLastCalledWith(computer.configuration.name, "/workspace", computer.configuration.name)
      await user.click(screen.getByRole("button", { name: "Download a b.txt" }))
      expect(transfers.download).toHaveBeenCalledWith(computer.configuration.name, "/workspace/a b.txt", computer.configuration.name)
    })

    it("offers downloads only for regular files", async () => {
      const store = createDirectoryStore(vi.fn().mockResolvedValue(entries))
      render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active transfers={controls()} />)
      await screen.findByText("link")
      expect(screen.getAllByRole("button", { name: /^Download / })).toHaveLength(1)
    })

    it("disables every transfer action while one is running", async () => {
      const store = createDirectoryStore(vi.fn().mockResolvedValue(entries))
      render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active onOpenEditor={vi.fn()} transfers={controls({ busy: true })} />)
      await screen.findByText("a b.txt")
      for (const button of [...screen.getAllByRole("button", { name: "Upload files here" }), screen.getByRole("button", { name: "Download a b.txt" })]) expect(button).toBeDisabled()
    })

    it("shows no transfer actions where the tree is not given the controls", async () => {
      const store = createDirectoryStore(vi.fn().mockResolvedValue(entries))
      render(<ComputerFileTree editor="Cursor" computer={computer} store={store} active onOpenEditor={vi.fn()} />)
      await screen.findByText("a b.txt")
      expect(screen.queryByRole("button", { name: /Upload files here|^Download / })).toBeNull()
    })
  })
})
