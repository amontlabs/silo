import { createContext, useContext, useSyncExternalStore } from "react"
import { z } from "zod"

export const macosComputerStates = ["preparing", "downloading", "installing", "stopped", "starting", "running", "stopping", "failed"] as const
export type MacosComputerState = (typeof macosComputerStates)[number]

export const macosComputerSchema = z.object({
  id: z.string().min(1),
  name: z.string(),
  cpus: z.number().int().positive(),
  memoryGiB: z.number().int().positive(),
  diskGiB: z.number().int().positive(),
  osVersion: z.string().nullable(),
  state: z.enum(macosComputerStates),
  progress: z.number().min(0).max(1).nullable(),
  detail: z.string().nullable(),
  displayOpen: z.boolean(),
})
export type MacosComputer = z.infer<typeof macosComputerSchema>

export const macosComputersStateSchema = z.object({
  supported: z.boolean(),
  unsupportedReason: z.string().nullable(),
  computers: z.array(macosComputerSchema),
})
export type MacosComputersState = z.infer<typeof macosComputersStateSchema>

export function parseMacosComputersState(value: unknown): MacosComputersState {
  return macosComputersStateSchema.parse(value)
}

export interface MacosComputerRequest {
  name: string
  cpus: number
  memoryGiB: number
  diskGiB: number
}

export type MacosComputerAction = "start" | "stop" | "force-stop" | "delete"

/** What the section needs from its host: the native commands in production, fixtures in the browser preview. */
export interface MacosComputersBackend {
  read(): Promise<unknown>
  create(request: MacosComputerRequest): Promise<unknown>
  action(id: string, action: MacosComputerAction): Promise<void>
  openDisplay(id: string): Promise<void>
  /** Subscribes to state changes; resolves to an unsubscribe function. */
  listen(handler: (state: unknown) => void): Promise<() => void>
}

export interface MacosComputersSnapshot {
  state: MacosComputersState | null
  /** The state could not be read. Cleared by the next valid state. */
  error: string | null
  /** Updates are late or incomplete (a malformed event, or no change events); the state shown may be behind. */
  warning: string | null
}

export interface MacosComputersStore {
  subscribe(listener: () => void): () => void
  getSnapshot(): MacosComputersSnapshot
  /** Reads the state again. */
  refresh(): Promise<void>
  create(request: MacosComputerRequest): Promise<void>
  action(id: string, action: MacosComputerAction): Promise<void>
  openDisplay(id: string): Promise<void>
}

const initialSnapshot: MacosComputersSnapshot = { state: null, error: null, warning: null }

function failureMessage(error: unknown) {
  return error instanceof Error ? error.message : typeof error === "string" ? error : "The state of macOS computers could not be read."
}

const listenRetryMs = (attempt: number) => Math.min(30_000, 1_000 * 2 ** attempt)

/**
 * Registers for change events, then reads the state when the first listener subscribes, and follows
 * the events until the last one leaves. A read that finishes after a newer event is discarded. While
 * the event listener cannot be registered it is retried with backoff and every action and creation
 * reads the state again.
 */
export function createMacosComputersStore(backend: MacosComputersBackend): MacosComputersStore {
  let snapshot = initialSnapshot
  const listeners = new Set<() => void>()
  let stop: (() => void) | undefined
  let retry: ReturnType<typeof setTimeout> | undefined
  let generation = 0
  let eventCount = 0
  let listening = false

  function publish(next: MacosComputersSnapshot) {
    snapshot = next
    listeners.forEach(listener => listener())
  }

  function accept(value: unknown, fromEvent: boolean) {
    const parsed = macosComputersStateSchema.safeParse(value)
    if (parsed.success) publish({ state: parsed.data, error: null, warning: fromEvent || listening ? null : snapshot.warning })
    else publish({ ...snapshot, warning: "An update to the macOS computers was unreadable. Refreshing…" })
  }

  async function read(mine: number) {
    const startedAfter = eventCount
    try {
      const value = await backend.read()
      if (mine === generation && eventCount === startedAfter) accept(value, false)
    } catch (error) {
      if (mine === generation && eventCount === startedAfter) publish({ ...snapshot, error: failureMessage(error) })
    }
  }

  async function register(mine: number, attempt: number): Promise<void> {
    try {
      const unlisten = await backend.listen(payload => {
        if (mine !== generation) return
        eventCount++
        accept(payload, true)
        // A malformed event leaves the state behind; read it again.
        if (!macosComputersStateSchema.safeParse(payload).success) void read(mine)
      })
      if (mine !== generation) { unlisten(); return }
      stop = unlisten
      listening = true
      if (snapshot.warning) publish({ ...snapshot, warning: null })
    } catch {
      if (mine !== generation) return
      listening = false
      publish({ ...snapshot, warning: "Live updates of macOS computers are unavailable. Retrying…" })
      retry = setTimeout(() => { void register(mine, attempt + 1).then(() => { if (listening) void read(mine) }) }, listenRetryMs(attempt))
    }
  }

  function start() {
    const mine = ++generation
    listening = false
    void register(mine, 0).then(() => read(mine))
  }

  async function refresh() {
    await read(generation)
  }

  // Without change events, an operation's effect is only visible by reading again.
  async function readAfter<T>(operation: Promise<T>): Promise<T> {
    try { return await operation } finally { if (!listening && listeners.size > 0) void refresh() }
  }

  return {
    subscribe(listener) {
      listeners.add(listener)
      if (listeners.size === 1) start()
      return () => {
        listeners.delete(listener)
        if (listeners.size === 0) {
          generation++
          clearTimeout(retry)
          stop?.()
          stop = undefined
          listening = false
        }
      }
    },
    getSnapshot: () => snapshot,
    refresh,
    async create(request) {
      const created = macosComputerSchema.parse(await readAfter(backend.create(request)))
      // The change event normally arrives first; the returned row covers a missed one.
      const current = snapshot.state
      if (current && !current.computers.some(({ id }) => id === created.id)) publish({ ...snapshot, state: { ...current, computers: [...current.computers, created] } })
    },
    action: (id, action) => readAfter(backend.action(id, action)),
    openDisplay: id => backend.openDisplay(id),
  }
}

export const MacosComputersContext = createContext<MacosComputersStore | null>(null)

const noopSubscribe = () => () => {}

/** The macOS computers of this device, or null when this build has none. */
export function useMacosComputers(): { store: MacosComputersStore; snapshot: MacosComputersSnapshot } | null {
  const store = useContext(MacosComputersContext)
  const snapshot = useSyncExternalStore(store ? store.subscribe : noopSubscribe, store ? store.getSnapshot : () => initialSnapshot)
  return store ? { store, snapshot } : null
}

export const macosNamePattern = /^[a-z][a-z0-9-]{0,31}$/

export interface MacosLimits {
  maxCPUs: number
  maxMemoryGiB: number
}

export const macosDefaults = { cpus: 4, memoryGiB: 8, diskGiB: 64 } as const
export const macosLimits = { minCPUs: 2, minMemoryGiB: 4, minDiskGiB: 32, maxDiskGiB: 1024, fallbackMaxCPUs: 32, fallbackMaxMemoryGiB: 512 } as const

export type MacosRequestErrors = Partial<Record<keyof MacosComputerRequest, string>>

export function validateMacosRequest(request: MacosComputerRequest, existingNames: readonly string[], limits: Partial<MacosLimits> = {}): MacosRequestErrors {
  const maxCPUs = Math.max(limits.maxCPUs ?? macosLimits.fallbackMaxCPUs, macosLimits.minCPUs)
  const maxMemoryGiB = Math.max(limits.maxMemoryGiB ?? macosLimits.fallbackMaxMemoryGiB, macosLimits.minMemoryGiB)
  const errors: MacosRequestErrors = {}
  if (!macosNamePattern.test(request.name)) errors.name = "Use 1 to 32 lowercase letters, digits or hyphens, starting with a letter."
  else if (existingNames.includes(request.name)) errors.name = "A macOS computer with this name exists."
  const within = (value: number, min: number, max: number) => Number.isSafeInteger(value) && value >= min && value <= max
  if (!within(request.cpus, macosLimits.minCPUs, maxCPUs)) errors.cpus = `Use ${macosLimits.minCPUs} to ${maxCPUs} CPUs.`
  if (!within(request.memoryGiB, macosLimits.minMemoryGiB, maxMemoryGiB)) errors.memoryGiB = `Use ${macosLimits.minMemoryGiB} to ${maxMemoryGiB} GiB of memory.`
  if (!within(request.diskGiB, macosLimits.minDiskGiB, macosLimits.maxDiskGiB)) errors.diskGiB = `Use ${macosLimits.minDiskGiB} to ${macosLimits.maxDiskGiB} GiB of disk.`
  return errors
}

export const isMacosCreating = (computer: MacosComputer) => computer.state === "preparing" || computer.state === "downloading" || computer.state === "installing"

export function macosStateLabel(computer: MacosComputer): string {
  const percent = computer.progress == null ? "" : ` ${Math.round(computer.progress * 100)}%`
  switch (computer.state) {
    case "preparing": return "Preparing"
    case "downloading": return `Downloading macOS${percent}`
    case "installing": return `Installing macOS${percent}`
    case "stopped": return "Stopped"
    case "starting": return "Starting"
    case "running": return "Running"
    case "stopping": return "Stopping"
    case "failed": return "Failed"
  }
}

export function macosResources(computer: MacosComputer): string {
  return `${computer.cpus} CPUs · ${computer.memoryGiB} GiB memory · ${computer.diskGiB} GiB disk`
}
