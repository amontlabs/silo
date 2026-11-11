import { act, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"
import { ShutdownBoundary } from "./shutdown-boundary"

const native = vi.hoisted(() => ({ receive: vi.fn<(event: { payload: boolean }) => void>(), queueChanged: vi.fn<() => void>(), listen: vi.fn(), invoke: vi.fn(), stop: vi.fn() }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }))
// Bind `native.receive` to the shutdown-state listener specifically: the boundary also
// subscribes to the operation queue while quitting, and that second listener must not
// steal the handle the tests use to toggle shutdown.
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }))
const handlers = new Map<string, () => void>()
beforeEach(() => {
  vi.clearAllMocks()
  handlers.clear()
  native.listen.mockReset().mockImplementation(async (event: string, receive: typeof native.receive) => {
    if (event === "silo://shutdown-state-changed") native.receive = receive
    if (event === "silo://operation-queue-changed") native.queueChanged = receive as unknown as typeof native.queueChanged
    handlers.set(event, receive as unknown as () => void)
    return native.stop
  })
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "read_shutdown_state") return false
    if (command === "read_operation_queue") return { running: [], waiting: [] }
    throw new Error(`Unexpected command: ${command}`)
  })
})

it("shows shutdown progress and disables the existing screen until native failure cancels Quit", async () => {
  const user = userEvent.setup()
  render(<ShutdownBoundary><input aria-label="Computer name" defaultValue="My computer" /><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("read_shutdown_state"))
  screen.getByRole("button", { name: "Create computer" }).focus()
  act(() => native.receive({ payload: true }))
  expect(screen.getByRole("dialog", { name: "Quitting Silo" })).toBeVisible()
  expect(screen.getByRole("status")).toHaveTextContent("Stopping local computers…")
  expect(screen.getByText("Create computer").closest("[inert]")).not.toBeNull()
  await user.keyboard("{Escape}{Tab}{Enter}")
  expect(screen.getByRole("dialog", { name: "Quitting Silo" })).toHaveFocus()
  act(() => native.receive({ payload: false }))
  expect(screen.queryByRole("status")).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Create computer" }).closest("[inert]")).toBeNull()
  expect(screen.getByRole("textbox", { name: "Computer name" })).toHaveValue("My computer")
  await vi.waitFor(() => expect(screen.getByRole("button", { name: "Create computer" })).toHaveFocus())
})
it("reads active shutdown when a window opens after the event", async () => {
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "read_shutdown_state") return true
    if (command === "read_operation_queue") return { running: [], waiting: [] }
    throw new Error(`Unexpected command: ${command}`)
  })
  const view = render(<ShutdownBoundary compact><button>Quit Silo</button></ShutdownBoundary>)
  expect(await screen.findByRole("status")).toHaveTextContent("Stopping local computers…")
  view.unmount()
  // The boundary holds four subscriptions: shutdown state, engine connection, Quit prompt
  // and the operation queue it reads for the overlay. All are released on unmount.
  expect(native.stop).toHaveBeenCalledTimes(4)
})
it("names the running work Quit waits for and cancels only cancellable entries on request", async () => {
  const queue = { running: [
    { id: 7, label: "Stopping local computers", kind: "shutdown", computerId: null, computerName: null, sinceMs: 0, cancellable: false, expectedMs: null, blockedByHidden: false },
    { id: 8, label: "Backing up computers", computerId: null, computerName: null, sinceMs: 0, cancellable: true, expectedMs: null, blockedByHidden: false },
  ], waiting: [] }
  native.invoke.mockImplementation(async (name: string) => name === "read_operation_queue" ? queue : name === "read_shutdown_state" ? false : true)
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("read_shutdown_state"))
  act(() => native.receive({ payload: true }))
  expect(await screen.findByText("Waiting for Backing up computers…")).toBeVisible()
  await userEvent.setup().click(screen.getByRole("button", { name: "Cancel and quit" }))
  expect(native.invoke).toHaveBeenCalledWith("cancel_operation", { id: 8 })
  expect(native.invoke).not.toHaveBeenCalledWith("cancel_operation", { id: 7 })
})
it("names the local computer Quit is stopping", async () => {
  const queue = { running: [{ id: 7, label: "Stopping dev (1 of 2)", kind: "shutdown", computerId: null, computerName: null, sinceMs: 0, cancellable: false, expectedMs: null, blockedByHidden: false }], waiting: [] }
  native.invoke.mockImplementation(async (name: string) => name === "read_operation_queue" ? queue : name === "read_shutdown_state" ? false : true)
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("read_shutdown_state"))
  act(() => native.receive({ payload: true }))
  expect(await screen.findByText("Stopping dev (1 of 2)…")).toBeVisible()
  expect(screen.queryByRole("button", { name: "Cancel and quit" })).not.toBeInTheDocument()
})
it("waits for non-cancellable running work and offers no cancel control", async () => {
  const queue = { running: [{ id: 9, label: "Installing update", computerId: null, computerName: null, sinceMs: 0, cancellable: false, expectedMs: null, blockedByHidden: false }], waiting: [] }
  native.invoke.mockImplementation(async (name: string) => name === "read_operation_queue" ? queue : name === "read_shutdown_state" ? false : true)
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("read_shutdown_state"))
  act(() => native.receive({ payload: true }))
  expect(await screen.findByText("Waiting for Installing update…")).toBeVisible()
  expect(screen.queryByRole("button", { name: "Cancel and quit" })).not.toBeInTheDocument()
})
it("does not let an older snapshot replace a newer shutdown event", async () => {
  let resolve!: (value: boolean) => void
  const older = new Promise<boolean>(done => { resolve = done })
  native.invoke.mockImplementation((command: string) => {
    if (command === "read_shutdown_state") return older
    if (command === "read_operation_queue") return Promise.resolve({ running: [], waiting: [] })
    throw new Error(`Unexpected command: ${command}`)
  })
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalled())
  act(() => native.receive({ payload: true }))
  await act(async () => resolve(false))
  expect(screen.getByRole("status")).toBeVisible()
})
it("names pending setup work Quit waits for when no queued operation runs", async () => {
  native.invoke.mockImplementation(async (name: string) => name === "read_operation_queue" ? { running: [], waiting: [] } : false)
  const view = render(<ShutdownBoundary pendingWork="Finishing setup (verifying GitHub access)…"><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("read_shutdown_state"))
  await act(async () => { native.receive({ payload: true }) })
  expect(screen.getByRole("status")).toHaveTextContent("Finishing setup (verifying GitHub access)…")
  view.rerender(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  expect(screen.getByRole("status")).toHaveTextContent("Stopping local computers…")
})

it("keeps the newest queue read when an earlier one answers last", async () => {
  const entry = (id: number, label: string) => ({ id, label, computerId: null, computerName: null, sinceMs: 0, cancellable: false, expectedMs: null, blockedByHidden: false })
  const reads: Array<(value: unknown) => void> = []
  native.invoke.mockImplementation((name: string) => name === "read_operation_queue"
    ? new Promise(resolve => { reads.push(resolve) })
    : Promise.resolve(name === "read_shutdown_state" ? false : true))
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("read_shutdown_state"))
  act(() => native.receive({ payload: true }))
  await vi.waitFor(() => expect(reads).toHaveLength(1))
  act(() => native.queueChanged())
  await vi.waitFor(() => expect(reads).toHaveLength(2))
  // The newer read (after the queue changed) answers first; the older, outdated read last.
  await act(async () => reads[1]({ running: [entry(2, "Installing update")], waiting: [] }))
  await act(async () => reads[0]({ running: [entry(1, "Backing up computers")], waiting: [] }))
  expect(screen.getByText("Waiting for Installing update…")).toBeVisible()
  expect(screen.queryByText("Waiting for Backing up computers…")).not.toBeInTheDocument()
})

it("observes work that starts while the queue listener is still registering", async () => {
  let subscribe!: () => void
  native.listen.mockImplementation(async (event: string, receive: typeof native.receive) => {
    if (event === "silo://shutdown-state-changed") { native.receive = receive; return native.stop }
    return new Promise<() => void>((resolve) => {
      subscribe = () => {
        native.queueChanged = receive as unknown as typeof native.queueChanged
        resolve(native.stop)
      }
    })
  })
  let queue: { running: unknown[]; waiting: unknown[] } = { running: [], waiting: [] }
  native.invoke.mockImplementation(async (command: string) => command === "read_operation_queue" ? queue : false)
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("read_shutdown_state"))
  await act(async () => { native.receive({ payload: true }) })
  await vi.waitFor(() => expect(subscribe).toBeTypeOf("function"))
  // This native queue change precedes registration, so it cannot deliver an event.
  queue = { running: [{ id: 8, label: "Backing up computers", computerId: null, computerName: null, sinceMs: 0, cancellable: true, expectedMs: null, blockedByHidden: false }], waiting: [] }
  await act(async () => subscribe())
  expect(await screen.findByText("Waiting for Backing up computers…")).toBeVisible()
  expect(screen.getByRole("button", { name: "Cancel and quit" })).toBeVisible()
})

it("recovers shutdown visibility on focus after initial event registration fails", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => {})
  native.listen.mockRejectedValueOnce(new Error("Event bridge not ready"))
  native.invoke.mockImplementation(async (command: string) => command === "read_shutdown_state" ? true : { running: [], waiting: [] })
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(logged).toHaveBeenCalledWith("Silo shutdown status:", expect.any(Error)))
  expect(screen.getByRole("button", { name: "Create computer" }).closest("[inert]")).toBeNull()
  await act(async () => { window.dispatchEvent(new Event("focus")) })
  expect(await screen.findByRole("dialog", { name: "Quitting Silo" })).toBeVisible()
  expect(screen.getByText("Create computer").closest("[inert]")).not.toBeNull()
  act(() => native.receive({ payload: false }))
  expect(screen.queryByRole("dialog", { name: "Quitting Silo" })).not.toBeInTheDocument()
  logged.mockRestore()
})


it("keeps running work and cancellation visible when the queue subscription fails", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => {})
  native.listen.mockImplementation(async (event: string, receive: typeof native.receive) => {
    if (event === "silo://operation-queue-changed") throw new Error("Event bridge not ready")
    if (event === "silo://shutdown-state-changed") native.receive = receive
    return native.stop
  })
  const queue = { running: [{ id: 8, label: "Backing up computers", computerId: null, computerName: null, sinceMs: 0, cancellable: true, expectedMs: null, blockedByHidden: false }], waiting: [] }
  native.invoke.mockImplementation(async (command: string) => command === "read_operation_queue" ? queue : false)
  try {
    render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
    await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("read_shutdown_state"))
    await act(async () => { native.receive({ payload: true }) })
    expect(await screen.findByText("Waiting for Backing up computers…")).toBeVisible()
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel and quit" }))
    expect(native.invoke).toHaveBeenCalledWith("cancel_operation", { id: 8 })
  } finally { logged.mockRestore() }
})


it("retries an unread shutdown snapshot on focus without duplicating its subscription", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => {})
  let failed = false
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "read_operation_queue") return { running: [], waiting: [] }
    if (!failed) { failed = true; throw new Error("State bridge not ready") }
    return true
  })
  try {
    render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
    await vi.waitFor(() => expect(logged).toHaveBeenCalledWith("Silo shutdown status:", expect.any(Error)))
    await act(async () => { window.dispatchEvent(new Event("focus")) })
    expect(await screen.findByRole("dialog", { name: "Quitting Silo" })).toBeVisible()
    expect(native.listen.mock.calls.filter(([event]) => event === "silo://shutdown-state-changed")).toHaveLength(1)
  } finally { logged.mockRestore() }
})

it("clears an engine-initiated shutdown overlay when the engine connection drops", async () => {
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(handlers.has("silo://engine-connection")).toBe(true))
  act(() => native.receive({ payload: true }))
  expect(screen.getByRole("status")).toHaveTextContent("Stopping local computers…")
  await act(async () => handlers.get("silo://engine-connection")!())
  expect(screen.queryByRole("status")).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Create computer" }).closest("[inert]")).toBeNull()
})
it("clears the overlay when the engine reconnects without a shutdown under way", async () => {
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(handlers.has("silo://engine-connection")).toBe(true))
  act(() => native.receive({ payload: true }))
  expect(screen.getByRole("status")).toBeVisible()
  await act(async () => handlers.get("silo://engine-connection")!())
  expect(screen.queryByRole("dialog", { name: "Quitting Silo" })).not.toBeInTheDocument()
})
it("keeps the overlay across a connection change while this app's own Quit is under way", async () => {
  native.invoke.mockImplementation(async (command: string) => command === "read_operation_queue" ? { running: [], waiting: [] } : true)
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  expect(await screen.findByRole("status")).toBeVisible()
  await act(async () => handlers.get("silo://engine-connection")!())
  expect(screen.getByRole("status")).toBeVisible()
})
it("lets the Quit prompt through an engine-initiated overlay so Cancel leaves the window usable", async () => {
  render(<ShutdownBoundary><button>Create computer</button></ShutdownBoundary>)
  await vi.waitFor(() => expect(handlers.has("silo://quit-requested")).toBe(true))
  act(() => native.receive({ payload: true }))
  expect(screen.getByRole("status")).toBeVisible()
  act(() => handlers.get("silo://quit-requested")!())
  expect(screen.queryByRole("status")).not.toBeInTheDocument()
})
