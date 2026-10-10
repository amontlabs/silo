import { createContext, useContext, useSyncExternalStore } from "react"
import { z } from "zod"

import type { MacosEditorState } from "@/features/computers/model/editor-drafts-context"
import type { ClipboardReport } from "@/desktop/viewer-clipboard-feedback"
import { bridgeErrorMessage, hasBridgeErrorCode } from "@/contracts/bridge-error"
import { parseRemoteComputerTarget, remoteComputerTarget } from "@/features/application/model/connections"

export const macosComputerStates = ["preparing", "copying", "downloading", "installing", "setting-up", "stopped", "starting", "running", "stopping", "failed"] as const
export type MacosComputerState = (typeof macosComputerStates)[number]

/** A saved checkpoint, in the words of the Linux checkpoint list: `full` includes memory. */
export const macosCheckpointSchema = z.object({
  id: z.string().min(1),
  name: z.string(),
  createdAt: z.string(),
  scope: z.enum(["full", "disk"]),
  reason: z.enum(["manual", "before-restore"]),
  sizeBytes: z.number().int().nonnegative().optional(),
})

export const macosCheckpointOperationSchema = z.object({
  kind: z.enum(["capture", "restore", "fork", "delete"]),
  status: z.enum(["running", "failed"]),
  stage: z.string(),
  error: z.string().optional(),
})

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
  installed: z.boolean(),
  setupComplete: z.boolean(),
  /** A copy of a template that still has the template's credentials: it can only be set up again or deleted. */
  needsPersonalizing: z.boolean().optional(),
  /** Newest first. */
  checkpoints: z.array(macosCheckpointSchema).optional(),
  checkpointOperation: macosCheckpointOperationSchema.nullable().optional(),
  /** A Restore that the next Start continues; `memory` when it brings the saved memory back. */
  pendingRestore: z.object({ checkpointId: z.string(), memory: z.boolean() }).nullable().optional(),
})
export type MacosComputer = z.infer<typeof macosComputerSchema>

/** The set-up computer that new macOS computers are copied from. */
export const macosTemplateSchema = z.object({
  macosVersion: z.string(),
  build: z.string(),
  /** Made by this version of the setup; an older one is kept but not copied from. */
  current: z.boolean(),
})
export type MacosTemplate = z.infer<typeof macosTemplateSchema>

export const macosComputersStateSchema = z.object({
  supported: z.boolean(),
  unsupportedReason: z.string().nullable(),
  computers: z.array(macosComputerSchema),
  template: macosTemplateSchema.nullable(),
  /** A copy keeps its template's disk size, so a new computer cannot have less. */
  minDiskGiB: z.number().int().positive(),
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

export type MacosComputerAction = "start" | "stop" | "force-stop" | "delete" | "setup"

/** Which way an explicit clipboard transfer goes: this Mac to the computer, or back. */
export type MacosClipboardDirection = "paste-into" | "copy-from"

/** The commands for macOS computers hosted by another connected device; `computerId` is the owner's own id. */
export interface MacosRemoteBackend {
  /** The owner's macOS state, in the shape of `MacosComputersBackend.read`. */
  snapshot(deviceId: string): Promise<unknown>
  create(deviceId: string, request: MacosComputerRequest): Promise<unknown>
  action(deviceId: string, computerId: string, action: MacosComputerAction): Promise<void>
  openDisplay(deviceId: string, computerId: string): Promise<void>
}

/** What the section needs from its host: the native commands in production, fixtures in the browser preview. */
export interface MacosComputersBackend {
  /** Absent where this build cannot reach other devices. */
  remote?: MacosRemoteBackend
  read(): Promise<unknown>
  create(request: MacosComputerRequest): Promise<unknown>
  action(id: string, action: MacosComputerAction): Promise<void>
  openDisplay(id: string): Promise<void>
  /** Transfers the clipboard once and resolves to the outcome. */
  clipboard(id: string, direction: MacosClipboardDirection): Promise<ClipboardReport>
  /** Removes the template; computers made from it are not affected. */
  deleteTemplate(): Promise<void>
  createCheckpoint(id: string, name: string): Promise<void>
  restoreCheckpoint(id: string, checkpointId: string): Promise<void>
  /** Creates a new stopped computer from a checkpoint; resolves once it exists and is being set up. */
  forkCheckpoint(id: string, checkpointId: string, newName: string): Promise<void>
  deleteCheckpoint(id: string, checkpointId: string): Promise<void>
  /** Subscribes to state changes; resolves to an unsubscribe function. */
  listen(handler: (state: unknown) => void): Promise<() => void>
}

/** What the last read of one connected device's macOS computers returned. */
export interface MacosRemoteState {
  /** The owner's state as last read; kept while a later read fails. */
  state: MacosComputersState | null
  error: string | null
  /** The owner is changing its computer configuration; its state is read again afterwards. */
  updating: boolean
}

export interface MacosRemoteDevice {
  id: string
  connected: boolean
}

export interface MacosComputersSnapshot {
  state: MacosComputersState | null
  /** Other devices' macOS computers by device id, for the devices given to `setRemoteDevices`. */
  remote: Readonly<Record<string, MacosRemoteState>>
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
  /**
   * Sets the devices whose macOS computers `owner` (a surface showing them) needs: they are read once
   * now, then at an interval while anything subscribes. The devices of all owners are read together;
   * an empty list withdraws the owner. Disconnected devices keep their last state and are not read.
   */
  setRemoteDevices(owner: string, devices: readonly MacosRemoteDevice[]): void
  create(request: MacosComputerRequest): Promise<void>
  /** Creates a computer on another device. */
  createRemote(deviceId: string, request: MacosComputerRequest): Promise<void>
  /** `id` is a local id or a remote one from `remoteMacosComputerId`; it picks the host. */
  action(id: string, action: MacosComputerAction): Promise<void>
  openDisplay(id: string): Promise<void>
  /** Local computers only. */
  clipboard(id: string, direction: MacosClipboardDirection): Promise<ClipboardReport>
  deleteTemplate(): Promise<void>
  /** The checkpoint operations are for local computers only. */
  createCheckpoint(id: string, name: string): Promise<void>
  restoreCheckpoint(id: string, checkpointId: string): Promise<void>
  forkCheckpoint(id: string, checkpointId: string, newName: string): Promise<void>
  deleteCheckpoint(id: string, checkpointId: string): Promise<void>
}

const initialSnapshot: MacosComputersSnapshot = { state: null, remote: {}, error: null, warning: null }

/** The id a computer of another device has in the unified list. */
export const remoteMacosComputerId = remoteComputerTarget

/** How often the macOS computers of connected devices are read while something is subscribed. */
export const remoteMacosPollMs = 10_000

const notLocalMessage = "This is not supported for macOS computers on other devices."
const remoteUnavailableMessage = "This build cannot manage macOS computers on other devices."
const remoteUnreadableMessage = "The macOS computers of this device were unreadable."

function remoteFailureMessage(error: unknown) {
  return bridgeErrorMessage(error) ?? (error instanceof Error && error.message.trim() ? error.message : typeof error === "string" && error.trim() ? error : "The macOS computers of this device could not be read.")
}

function failureMessage(error: unknown) {
  return error instanceof Error ? error.message : typeof error === "string" ? error : "The state of macOS computers could not be read."
}

const unreadableMessage = "The state of the macOS computers was unreadable."

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
  let readCount = 0
  const remoteDevices = new Map<string, boolean>()
  const remoteOwners = new Map<string, readonly MacosRemoteDevice[]>()
  const remoteReads = new Map<string, Promise<void>>()
  const remoteReadAgain = new Set<string>()
  const remoteRevisions = new Map<string, number>()
  let remoteTimer: ReturnType<typeof setInterval> | undefined

  function publish(next: MacosComputersSnapshot) {
    snapshot = next
    listeners.forEach(listener => listener())
  }

  function accept(value: unknown, fromEvent: boolean) {
    const parsed = macosComputersStateSchema.safeParse(value)
    if (parsed.success) publish({ ...snapshot, state: parsed.data, error: null, warning: fromEvent || listening ? null : snapshot.warning })
    else if (fromEvent && snapshot.state) publish({ ...snapshot, warning: "An update to the macOS computers was unreadable. Refreshing…" })
    else publish({ ...snapshot, error: unreadableMessage })
  }

  async function read(mine: number) {
    // A read for an ended subscription must not displace the current one's.
    if (mine !== generation) return
    const startedAfter = eventCount
    const sequence = ++readCount
    // Only the newest read, with no event since it started, reflects the latest state.
    const current = () => mine === generation && eventCount === startedAfter && sequence === readCount
    try {
      const value = await backend.read()
      if (current()) accept(value, false)
    } catch (error) {
      if (current()) publish({ ...snapshot, error: failureMessage(error) })
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
    syncRemoteTimer()
    remoteDevices.forEach((connected, id) => { if (connected) void readRemote(id) })
  }

  const remoteRevision = (deviceId: string) => remoteRevisions.get(deviceId) ?? 0
  /** A change to a device's computers drops reads that started before it. */
  const bumpRemote = (deviceId: string) => remoteRevisions.set(deviceId, remoteRevision(deviceId) + 1)

  function setRemote(deviceId: string, next: MacosRemoteState) {
    publish({ ...snapshot, remote: { ...snapshot.remote, [deviceId]: next } })
  }

  function readRemote(deviceId: string): Promise<void> {
    const remote = backend.remote
    if (!remote || listeners.size === 0 || !remoteDevices.has(deviceId)) return Promise.resolve()
    const inFlight = remoteReads.get(deviceId)
    if (inFlight) { remoteReadAgain.add(deviceId); return inFlight }
    const mine = generation
    const stale = () => mine !== generation || !remoteDevices.has(deviceId)
    const run: Promise<void> = (async () => {
      do {
        remoteReadAgain.delete(deviceId)
        const revision = remoteRevision(deviceId)
        let outcome: { value: unknown } | { cause: unknown }
        try { outcome = { value: await remote.snapshot(deviceId) } } catch (cause) { outcome = { cause } }
        if (stale()) return
        // A change during the read makes it older than what is shown; read again.
        if (revision !== remoteRevision(deviceId)) { remoteReadAgain.add(deviceId); continue }
        const previous = snapshot.remote[deviceId]?.state ?? null
        if ("cause" in outcome) {
          // An owner that predates macOS computers on other devices has none to show.
          if (hasBridgeErrorCode(outcome.cause, "unsupported_remote_operation")) setRemote(deviceId, { state: null, error: null, updating: false })
          else if (hasBridgeErrorCode(outcome.cause, "update_in_progress")) setRemote(deviceId, { state: previous, error: null, updating: true })
          else setRemote(deviceId, { state: previous, error: remoteFailureMessage(outcome.cause), updating: false })
          continue
        }
        const parsed = macosComputersStateSchema.safeParse(outcome.value)
        setRemote(deviceId, parsed.success ? { state: parsed.data, error: null, updating: false } : { state: previous, error: remoteUnreadableMessage, updating: false })
      } while (remoteReadAgain.has(deviceId))
    })().finally(() => { if (remoteReads.get(deviceId) === run) remoteReads.delete(deviceId) })
    remoteReads.set(deviceId, run)
    return run
  }

  function syncRemoteTimer() {
    const wanted = listeners.size > 0 && remoteDevices.size > 0 && Boolean(backend.remote)
    if (!wanted) { clearInterval(remoteTimer); remoteTimer = undefined; return }
    remoteTimer ??= setInterval(() => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return
      remoteDevices.forEach((connected, id) => { if (connected) void readRemote(id) })
    }, remoteMacosPollMs)
  }

  function setRemoteDevices(owner: string, devices: readonly MacosRemoteDevice[]) {
    const previous = new Map(remoteDevices)
    if (devices.length > 0) remoteOwners.set(owner, devices)
    else remoteOwners.delete(owner)
    remoteDevices.clear()
    remoteOwners.forEach(owned => owned.forEach(({ id, connected }) => remoteDevices.set(id, connected || remoteDevices.get(id) === true)))
    const kept = Object.fromEntries(Object.entries(snapshot.remote).filter(([id]) => remoteDevices.has(id)))
    // Devices that left the list lose their state; with no surface left, the last state waits for the next one.
    if (remoteOwners.size > 0 && Object.keys(kept).length !== Object.keys(snapshot.remote).length) publish({ ...snapshot, remote: kept })
    syncRemoteTimer()
    remoteDevices.forEach((connected, id) => { if (connected && previous.get(id) !== true) void readRemote(id) })
  }

  /** Runs a command on another device, then reads that device's computers again. */
  async function remoteOperation<T>(deviceId: string, operation: (remote: MacosRemoteBackend) => Promise<T>): Promise<T> {
    if (!backend.remote) throw new Error(remoteUnavailableMessage)
    bumpRemote(deviceId)
    try { return await operation(backend.remote) } finally { bumpRemote(deviceId); void readRemote(deviceId) }
  }

  function localOnly(id: string) {
    if (parseRemoteComputerTarget(id)) throw new Error(notLocalMessage)
  }

  async function refresh() {
    await Promise.all([read(generation), ...[...remoteDevices].filter(([, connected]) => connected).map(([id]) => readRemote(id))])
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
          remoteReads.clear()
          remoteReadAgain.clear()
          syncRemoteTimer()
          clearTimeout(retry)
          stop?.()
          stop = undefined
          listening = false
        }
      }
    },
    getSnapshot: () => snapshot,
    refresh,
    setRemoteDevices,
    async createRemote(deviceId, request) {
      const created = macosComputerSchema.parse(await remoteOperation(deviceId, remote => remote.create(deviceId, request)))
      // The read after the creation normally shows it; the returned row covers a read that missed it.
      const current = snapshot.remote[deviceId]
      if (current?.state && !current.state.computers.some(({ id }) => id === created.id)) setRemote(deviceId, { ...current, state: { ...current.state, computers: [...current.state.computers, created] } })
    },
    async create(request) {
      const created = macosComputerSchema.parse(await readAfter(backend.create(request)))
      // The change event normally arrives first; the returned row covers a missed one.
      const current = snapshot.state
      if (current && !current.computers.some(({ id }) => id === created.id)) publish({ ...snapshot, state: { ...current, computers: [...current.computers, created] } })
    },
    action: (id, action) => {
      const target = parseRemoteComputerTarget(id)
      return target ? remoteOperation(target.deviceId, remote => remote.action(target.deviceId, target.computerId, action)) : readAfter(backend.action(id, action))
    },
    openDisplay: async id => {
      const target = parseRemoteComputerTarget(id)
      if (!target) return backend.openDisplay(id)
      if (!backend.remote) throw new Error(remoteUnavailableMessage)
      return backend.remote.openDisplay(target.deviceId, target.computerId)
    },
    clipboard: async (id, direction) => { localOnly(id); return backend.clipboard(id, direction) },
    deleteTemplate: () => readAfter(backend.deleteTemplate()),
    createCheckpoint: async (id, name) => { localOnly(id); return readAfter(backend.createCheckpoint(id, name)) },
    restoreCheckpoint: async (id, checkpointId) => { localOnly(id); return readAfter(backend.restoreCheckpoint(id, checkpointId)) },
    forkCheckpoint: async (id, checkpointId, newName) => { localOnly(id); return readAfter(backend.forkCheckpoint(id, checkpointId, newName)) },
    deleteCheckpoint: async (id, checkpointId) => { localOnly(id); return readAfter(backend.deleteCheckpoint(id, checkpointId)) },
  }
}

export const MacosComputersContext = createContext<MacosComputersStore | null>(null)

const noopSubscribe = () => () => {}

/** The macOS computers of this device, or null when this build has none. */
export function useMacosComputers(enabled = true): { store: MacosComputersStore; snapshot: MacosComputersSnapshot } | null {
  const context = useContext(MacosComputersContext)
  const store = enabled ? context : null
  const snapshot = useSyncExternalStore(store ? store.subscribe : noopSubscribe, store ? store.getSnapshot : () => initialSnapshot)
  return store ? { store, snapshot } : null
}

export const macosNamePattern = /^[a-z][a-z0-9-]{0,31}$/

export interface MacosLimits {
  /** The disk of the template new computers are copied from, when it is larger than the general minimum. */
  minDiskGiB: number
  maxCPUs: number
  maxMemoryGiB: number
  /** Names of this device's computers of other kinds; macOS names stay unique across kinds. */
  otherNames: readonly string[]
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
  else if (limits.otherNames?.some(name => name.toLowerCase() === request.name)) errors.name = "Computer names must be unique."
  const within = (value: number, min: number, max: number) => Number.isSafeInteger(value) && value >= min && value <= max
  if (!within(request.cpus, macosLimits.minCPUs, maxCPUs)) errors.cpus = `Use ${macosLimits.minCPUs} to ${maxCPUs} CPUs.`
  if (!within(request.memoryGiB, macosLimits.minMemoryGiB, maxMemoryGiB)) errors.memoryGiB = `Use ${macosLimits.minMemoryGiB} to ${maxMemoryGiB} GiB of memory.`
  const minDiskGiB = Math.max(limits.minDiskGiB ?? macosLimits.minDiskGiB, macosLimits.minDiskGiB)
  if (!within(request.diskGiB, minDiskGiB, macosLimits.maxDiskGiB)) errors.diskGiB = `Use ${minDiskGiB} to ${macosLimits.maxDiskGiB} GiB of disk${minDiskGiB > macosLimits.minDiskGiB ? `, as new computers are copied from a template of ${minDiskGiB} GiB` : ""}.`
  return errors
}

export const isMacosCreating = (computer: MacosComputer) => computer.state === "preparing" || computer.state === "copying" || computer.state === "downloading" || computer.state === "installing"

/** Checkpoints need a computer whose setup has finished and that is not changing state or being installed. */
export const canCheckpointMacos = (computer: MacosComputer) => computer.installed && computer.setupComplete && (computer.state === "stopped" || computer.state === "running" || computer.state === "failed")

export const isMacosSettingUp = (computer: MacosComputer) => computer.state === "setting-up"

/** An installed computer that stopped before its setup finished can run the remaining steps again. */
export const canRetryMacosSetup = (computer: MacosComputer) => computer.installed && !computer.setupComplete && (computer.state === "stopped" || computer.state === "failed")

export function macosStateLabel(computer: MacosComputer): string {
  const percent = computer.progress == null ? "" : ` ${Math.round(computer.progress * 100)}%`
  switch (computer.state) {
    case "preparing": return "Preparing"
    case "copying": return "Copying macOS"
    case "downloading": return `Downloading macOS${percent}`
    case "installing": return `Installing macOS${percent}`
    case "setting-up": return "Setting up macOS"
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

function parseWholeNumber(text: string) {
  return /^\d+$/.test(text.trim()) ? Number(text.trim()) : Number.NaN
}

export type MacosFormFields = Pick<MacosEditorState, "name" | "cpus" | "memoryGiB" | "diskGiB">

/** The starting values of the macOS fields, within what the device offers. */
export function defaultMacosFields(capacity?: { logicalCPUs: number; memoryGiB: number }): MacosFormFields {
  return {
    name: "",
    cpus: String(Math.min(macosDefaults.cpus, capacity?.logicalCPUs ?? macosDefaults.cpus)),
    memoryGiB: String(Math.min(macosDefaults.memoryGiB, capacity?.memoryGiB ?? macosDefaults.memoryGiB)),
    diskGiB: String(macosDefaults.diskGiB),
  }
}

export function macosRequestFrom(fields: MacosFormFields): MacosComputerRequest {
  return { name: fields.name, cpus: parseWholeNumber(fields.cpus), memoryGiB: parseWholeNumber(fields.memoryGiB), diskGiB: parseWholeNumber(fields.diskGiB) }
}
