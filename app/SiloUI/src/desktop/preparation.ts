import { createContext, createElement, useContext, useSyncExternalStore, type ReactNode } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { z } from "zod"

import { useChatGptApp, useComputerUseBridge } from "./computer-use-bridge"
import type { ChatGptAppStatus } from "./linux-desktop-state"

const taskSchema = z.object({
  state: z.enum(["pending", "running", "ready", "failed"]),
  fraction: z.number().nullish().catch(null),
  message: z.string().nullish().catch(null),
  retryable: z.boolean().catch(false),
})
export const preparationStatusSchema = z.object({ image: taskSchema, lcu: taskSchema })
export type PreparationTask = z.infer<typeof taskSchema>
export type PreparationStatus = z.infer<typeof preparationStatusSchema>

/** What this device prepares in the background at launch: the native commands in
 * production, deterministic fixtures in the browser preview. */
export interface PreparationBackend {
  read(): Promise<unknown>
  /** Starts again whatever failed. Resolves at once; progress follows from the status. */
  retry(): Promise<unknown>
  /** Subscribes to `silo://preparation-status` events; resolves to an unsubscribe function. */
  listen(handler: (status: unknown) => void): Promise<() => void>
}

export const nativePreparationBackend: PreparationBackend = {
  read: () => invoke("read_preparation_status"),
  retry: () => invoke("retry_preparation"),
  listen: handler => listen("silo://preparation-status", event => handler(event.payload)),
}

export interface PreparationStore {
  subscribe(listener: () => void): () => void
  getSnapshot(): PreparationStatus | null
  retry(): Promise<void>
}

export function createPreparationStore(backend: PreparationBackend): PreparationStore {
  let snapshot: PreparationStatus | null = null
  const listeners = new Set<() => void>()
  let stop: (() => void) | null = null
  let generation = 0
  // A read that began before an event is older than it and is dropped.
  let events = 0
  const set = (next: PreparationStatus) => {
    if (JSON.stringify(next) === JSON.stringify(snapshot)) return
    snapshot = next
    listeners.forEach(listener => listener())
  }
  const receive = (value: unknown, fromEvent: boolean) => {
    const parsed = preparationStatusSchema.safeParse(value)
    if (!parsed.success) return
    if (fromEvent) events += 1
    set(parsed.data)
  }
  const read = async () => {
    const seen = events
    try {
      const value = await backend.read()
      if (seen === events) receive(value, false)
    } catch { /* The next event or retry reads again. */ }
  }
  const start = () => {
    const mine = ++generation
    backend.listen(value => { if (mine === generation) receive(value, true) }).then(unlisten => {
      if (mine !== generation) { unlisten(); return }
      stop = unlisten
      // Read once events are heard, so none can fall between the two.
      void read()
    }, () => { void read() })
  }
  return {
    subscribe(listener) {
      listeners.add(listener)
      if (listeners.size === 1) start()
      return () => {
        listeners.delete(listener)
        if (listeners.size === 0) { generation += 1; stop?.(); stop = null }
      }
    },
    getSnapshot: () => snapshot,
    async retry() {
      try { receive(await backend.retry(), true) } catch { await read() }
    },
  }
}

export const PreparationContext = createContext<PreparationStore | null>(null)

export function PreparationProvider({ store, children }: { store: PreparationStore; children: ReactNode }) {
  return createElement(PreparationContext.Provider, { value: store }, children)
}

export type PreparationItemId = "image" | "lcu" | "chatgpt"

export interface PreparationItem {
  id: PreparationItemId
  state: "running" | "failed"
  /** The current work in plain words, or the failure's short message. */
  text: string
  /** 0-1 when known. */
  progress: number | null
  retryable: boolean
}

const noop = () => () => {}
const nothing = () => null

function percent(received: number, total: number | null | undefined) {
  return total && total > 0 ? Math.min(1, received / total) : null
}

/** The items that still need the user's attention, in the order they are prepared. A failure
 * is an item until the next attempt starts. Items that are ready or only waiting are absent. */
export function describePreparation(status: PreparationStatus | null, chatgpt: ChatGptAppStatus | null): PreparationItem[] {
  const items: PreparationItem[] = []
  const task = (id: "image" | "lcu", running: (fraction: number | null) => string, failed: string, value: PreparationTask | undefined) => {
    if (value?.state === "running") items.push({ id, state: "running", text: running(value.fraction ?? null), progress: value.fraction == null ? null : value.fraction / 100, retryable: false })
    if (value?.state === "failed") items.push({ id, state: "failed", text: value.message || failed, progress: null, retryable: value.retryable })
  }
  // The image reports a fraction while it downloads, then none while it is verified and imported.
  task("image", fraction => fraction === null ? "Preparing the VM image (first time only)" : `Downloading the VM image · ${Math.floor(fraction)}%`, "Silo could not prepare its VM image.", status?.image)
  task("lcu", () => "Downloading LCU", "Silo could not download LCU.", status?.lcu)
  switch (chatgpt?.state) {
    case "downloading": {
      const fraction = percent(chatgpt.receivedBytes, chatgpt.totalBytes)
      items.push({ id: "chatgpt", state: "running", text: fraction === null ? "Downloading ChatGPT for Linux" : `Downloading ChatGPT for Linux · ${Math.floor(fraction * 100)}%`, progress: fraction, retryable: false })
      break
    }
    case "verifying": items.push({ id: "chatgpt", state: "running", text: "Verifying ChatGPT for Linux", progress: null, retryable: false }); break
    case "extracting": items.push({ id: "chatgpt", state: "running", text: "Preparing ChatGPT for Linux", progress: null, retryable: false }); break
    case "failed": items.push({ id: "chatgpt", state: "failed", text: chatgpt.reason, progress: null, retryable: chatgpt.retryable }); break
    default: break
  }
  return items
}

export interface PreparationState {
  status: PreparationStatus | null
  chatgpt: ChatGptAppStatus | null
  items: PreparationItem[]
  /** False until the backend has answered and while anything is still running or waiting. */
  ready: boolean
  /** Starts again whatever failed (the VM image, LCU and the ChatGPT for Linux download). */
  retry(): void
}

/** What this device is preparing in the background, for the toast and for any action that
 * shows a "waiting for X" step. Without a provider everything reads as ready. */
export function usePreparationStatus(): PreparationState {
  const store = useContext(PreparationContext)
  const status = useSyncExternalStore(store ? store.subscribe : noop, store ? store.getSnapshot : nothing)
  const bridge = useComputerUseBridge()
  const chatGptStore = bridge?.chatGptFor()
  const chatgpt = useChatGptApp(chatGptStore, store !== null).status
  const items = describePreparation(status, chatgpt)
  const waiting = [status?.image, status?.lcu].some(task => task?.state === "pending") || chatgpt?.state === "idle"
  return {
    status,
    chatgpt,
    items,
    ready: status !== null && items.length === 0 && !waiting,
    retry() {
      void store?.retry()
      if (chatgpt?.state === "failed") void chatGptStore?.retry()
    },
  }
}
