import { createContext, useContext, useSyncExternalStore } from "react"
import { errorMessage } from "@/lib/error-message"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { parseRemoteComputerTarget } from "@/features/application/model/connections"
import { chatGptAppStatusSchema, parseChatGptAppStatus, parseLinuxDesktopState, type ChatGptAppStatus, type ComputerUseApproval, type LinuxDesktopState } from "./linux-desktop-state"

/** What built-in computer use needs from its host: the native commands in production,
 * deterministic fixtures in the browser preview. Every device downloads the ChatGPT app
 * by itself; the commands only read its status and ask it to try again. They take the host
 * id of a remote device (omitted: this device). */
export interface ComputerUseBackend {
  readDesktopState(computer: string): Promise<unknown>
  setApproval(computer: string, mode: ComputerUseApproval): Promise<unknown>
  setup(computer: string): Promise<unknown>
  chatGptStatus(device?: string): Promise<unknown>
  /** Asks the device to download the app again now. Resolves at once; progress follows from the status. */
  retry(device?: string): Promise<unknown>
  /** Subscribes to `chatgpt-app-status` events of this device; resolves to an unsubscribe function. */
  listenStatus(handler: (status: unknown) => void): Promise<() => void>
}

export const nativeComputerUseBackend: ComputerUseBackend = {
  readDesktopState: computer => invoke("read_desktop_state", { computer }),
  setApproval: (computer, mode) => invoke("set_computer_use_approval", { computer, mode }),
  setup: computer => invoke("desktop_action", { computer, action: "setup-computer-use" }),
  chatGptStatus: device => invoke("chatgpt_app_status", { device }),
  retry: device => invoke("chatgpt_app_retry", { device }),
  listenStatus: handler => listen("chatgpt-app-status", event => handler(event.payload)),
}

/** Finishes the creation with this request ID without waiting for ChatGPT for Linux; computer use then sets up at first start. */
export async function skipComputerUseWait(requestId: string): Promise<void> {
  try { await invoke("skip_computer_use_wait", { requestId }) } catch { /* nothing waits when this is not the desktop app */ }
}

export interface ChatGptAppSnapshot {
  status: ChatGptAppStatus | null
  /** Set while a retry request is in flight. */
  busy: boolean
  /** The last retry request failed. Kept until the next request or `dismissError`. */
  error: string | null
  /** The status could not be read. Cleared by the next successful read or event. */
  loadError: string | null
  /** Local status events are unavailable. Cleared when listener registration succeeds. */
  subscriptionError: string | null
}

export interface ChatGptAppStore {
  subscribe(listener: () => void): () => void
  getSnapshot(): ChatGptAppSnapshot
  refresh(): Promise<void>
  /** Asks the device to try the download again now. */
  retry(): Promise<void>
  dismissError(): void
}

export interface ComputerUseBridge {
  readState(computer: string): Promise<LinuxDesktopState>
  setApproval(computer: string, mode: ComputerUseApproval): Promise<LinuxDesktopState>
  setup(computer: string): Promise<LinuxDesktopState>
  /** The ChatGPT app store of one device: this one (omitted) or the remote device with that device id. */
  chatGptFor(device?: string): ChatGptAppStore
}

const LOCAL = "local"
// A remote device's status has no events: it is read again on this schedule.
const REMOTE_BUSY_POLL_MS = 3000
const REMOTE_IDLE_POLL_MS = 15000
const REMOTE_FAILURE_POLL_MAX_MS = 30000
const working = (status: ChatGptAppStatus | null) => status?.state === "downloading" || status?.state === "verifying" || status?.state === "extracting" || status?.state === "idle"

/** The device id of the device that owns a computer target, undefined for a local computer. */
export function deviceOfComputer(computer: string | undefined): string | undefined {
  if (!computer) return undefined
  try { return parseRemoteComputerTarget(computer)?.deviceId } catch { return undefined }
}

function createChatGptAppStore(backend: ComputerUseBackend, device: string | undefined, pollMs = { busy: REMOTE_BUSY_POLL_MS, idle: REMOTE_IDLE_POLL_MS }): ChatGptAppStore {
  let snapshot: ChatGptAppSnapshot = { status: null, busy: false, error: null, loadError: null, subscriptionError: null }
  const listeners = new Set<() => void>()
  let stopListening: (() => void) | null = null
  let timer: number | undefined
  let onVisibilityChange: (() => void) | undefined
  // Every event bumps `events`; a read that began before one is older than it and is dropped.
  let events = 0
  let reads = 0
  let generation = 0
  let failureDelay = 0
  const set = (next: Partial<ChatGptAppSnapshot>) => {
    const updated = { ...snapshot, ...next }
    if (JSON.stringify(updated) === JSON.stringify(snapshot)) return
    snapshot = updated
    listeners.forEach(listener => listener())
  }
  // An event this Silo cannot read leaves the last status in place.
  const receive = (value: unknown) => {
    const parsed = chatGptAppStatusSchema.safeParse(value)
    if (!parsed.success) return
    events += 1
    set({ status: parsed.data, loadError: null })
  }
  const refresh = async () => {
    const read = ++reads
    const seenEvents = events
    try {
      const value = await backend.chatGptStatus(device)
      if (read !== reads || seenEvents !== events) return
      const status = parseChatGptAppStatus(value)
      if (status) { failureDelay = 0; set({ status, loadError: null }) }
      else {
        failureDelay = Math.min(Math.max(failureDelay, working(snapshot.status) ? pollMs.busy : pollMs.idle) * 2, REMOTE_FAILURE_POLL_MAX_MS)
        set({ loadError: "Silo could not read the ChatGPT for Linux status." })
      }
    } catch (cause) {
      if (read === reads && seenEvents === events) {
        failureDelay = Math.min(Math.max(failureDelay, working(snapshot.status) ? pollMs.busy : pollMs.idle) * 2, REMOTE_FAILURE_POLL_MAX_MS)
        set({ loadError: errorMessage(cause) })
      }
    }
  }
  const retry = async () => {
    set({ busy: true, error: null })
    try { await backend.retry(device) } catch (cause) { set({ error: errorMessage(cause) }) } finally { set({ busy: false }) }
    await refresh()
  }
  const schedule = (mine: number) => {
    if (device === undefined) return
    window.clearTimeout(timer)
    if (document.visibilityState === "hidden") return
    timer = window.setTimeout(() => {
      if (mine !== generation || document.visibilityState === "hidden") return
      void refresh().finally(() => { if (mine === generation) schedule(mine) })
    }, Math.max(failureDelay, working(snapshot.status) ? pollMs.busy : pollMs.idle))
  }
  const start = () => {
    const mine = ++generation
    const begin = () => {
      if (mine !== generation || (device !== undefined && document.visibilityState === "hidden")) return
      void refresh().finally(() => { if (mine === generation) schedule(mine) })
    }
    let retryDelay = 1000
    let registering = false
    const register = () => {
      if (mine !== generation || registering) return
      registering = true
      backend.listenStatus(payload => { if (mine === generation && device === undefined) receive(payload) })
        .then(stop => {
          if (mine !== generation) { stop(); return }
          registering = false
          stopListening = stop
          set({ subscriptionError: null })
          // Read only once events are being heard, so none can fall between the two.
          begin()
        }, cause => {
          if (mine !== generation) return
          registering = false
          if (device === undefined) {
            set({ subscriptionError: `Silo could not subscribe to ChatGPT for Linux updates: ${errorMessage(cause)}` })
            if (document.visibilityState !== "hidden") {
              timer = window.setTimeout(register, retryDelay)
              retryDelay = Math.min(retryDelay * 2, 30_000)
            }
          }
          begin()
        })
    }
    onVisibilityChange = () => {
      window.clearTimeout(timer)
      if (document.visibilityState === "hidden") return
      if (device !== undefined) begin()
      else if (snapshot.subscriptionError) register()
    }
    document.addEventListener("visibilitychange", onVisibilityChange)
    if (device === undefined) register()
    else begin()
  }
  return {
    subscribe(listener) {
      listeners.add(listener)
      if (listeners.size === 1) start()
      return () => {
        listeners.delete(listener)
        if (listeners.size === 0) {
          generation += 1
          reads += 1
          stopListening?.()
          stopListening = null
          window.clearTimeout(timer)
          if (onVisibilityChange) document.removeEventListener("visibilitychange", onVisibilityChange)
          onVisibilityChange = undefined
        }
      }
    },
    getSnapshot: () => snapshot,
    refresh,
    retry,
    dismissError: () => set({ error: null }),
  }
}

export function createComputerUseBridge(backend: ComputerUseBackend, pollMs?: { busy: number; idle: number }): ComputerUseBridge {
  const stores = new Map<string, ChatGptAppStore>()
  return {
    readState: async computer => parseLinuxDesktopState(await backend.readDesktopState(computer)),
    setApproval: async (computer, mode) => parseLinuxDesktopState(await backend.setApproval(computer, mode)),
    setup: async computer => parseLinuxDesktopState(await backend.setup(computer)),
    chatGptFor(device) {
      const key = device || LOCAL
      let store = stores.get(key)
      if (!store) {
        store = createChatGptAppStore(backend, device || undefined, pollMs)
        stores.set(key, store)
      }
      return store
    },
  }
}

export const ComputerUseContext = createContext<ComputerUseBridge | null>(null)

export function useComputerUseBridge() { return useContext(ComputerUseContext) }

const emptySnapshot: ChatGptAppSnapshot = { status: null, busy: false, error: null, loadError: null, subscriptionError: null }
const noopSubscribe = () => () => {}
const emptyStore = () => emptySnapshot
export function useChatGptApp(store: ChatGptAppStore | undefined, active = true): ChatGptAppSnapshot {
  return useSyncExternalStore(store && active ? store.subscribe : noopSubscribe, store ? store.getSnapshot : emptyStore)
}
