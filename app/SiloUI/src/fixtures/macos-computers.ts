import { createMacosComputersStore, type MacosComputer, type MacosComputersBackend, type MacosComputersState } from "@/features/macos-computers/model/macos-computers"

export const macosComputerFixtures: readonly MacosComputer[] = [
  { id: "mac-download", name: "sequoia-test", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: null, state: "downloading", progress: 0.42, detail: null, displayOpen: false, installed: false, setupComplete: true },
  { id: "mac-copy", name: "fast-one", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "copying", progress: null, detail: null, displayOpen: false, installed: false, setupComplete: false },
  { id: "mac-install", name: "release-check", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "installing", progress: 0.63, detail: null, displayOpen: false, installed: false, setupComplete: true },
  { id: "mac-setup", name: "agent-box", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "setting-up", progress: null, detail: "Creating the account", displayOpen: false, installed: true, setupComplete: false },
  { id: "mac-stopped", name: "xcode-build", cpus: 6, memoryGiB: 16, diskGiB: 128, osVersion: "26.6.2 (25G83)", state: "stopped", progress: null, detail: null, displayOpen: false, installed: true, setupComplete: true,
    checkpoints: [
      { id: "00000000-0000-4000-8000-000000000002", name: "Xcode installed", createdAt: "2026-10-09T10:00:00Z", scope: "full", reason: "manual", sizeBytes: 6_400_000_000 },
      { id: "00000000-0000-4000-8000-000000000001", name: "Fresh install", createdAt: "2026-10-08T10:00:00Z", scope: "disk", reason: "manual" },
    ] },
  { id: "mac-running", name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "running", progress: null, detail: null, displayOpen: false, installed: true, setupComplete: true },
  { id: "mac-failed", name: "broken", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: null, state: "failed", progress: null, detail: "The macOS download did not finish. Check your connection and create the computer again.", displayOpen: false, installed: false, setupComplete: false },
]

/** Keeps the computers in memory; start, stop and delete change them at once and create adds a downloading row. */
export function createFixtureMacosComputersBackend(initial: readonly MacosComputer[] = macosComputerFixtures): MacosComputersBackend {
  let state: MacosComputersState = { supported: true, unsupportedReason: null, computers: [...initial], template: { macosVersion: "26.6.2", build: "25G83", current: true }, minDiskGiB: 64 }
  const handlers = new Set<(state: unknown) => void>()
  const change = (computers: MacosComputer[]) => {
    state = { ...state, computers }
    handlers.forEach(handler => handler(state))
  }
  const update = (id: string, patch: Partial<MacosComputer>) => change(state.computers.map(computer => computer.id === id ? { ...computer, ...patch } : computer))
  return {
    read: async () => state,
    listen: async handler => {
      handlers.add(handler)
      return () => { handlers.delete(handler) }
    },
    create: async request => {
      const created: MacosComputer = { id: `mac-${request.name}`, ...request, osVersion: null, state: "preparing", progress: null, detail: null, displayOpen: false, installed: false, setupComplete: false }
      change([...state.computers, created])
      return created
    },
    action: async (id, action) => {
      if (action === "delete") change(state.computers.filter(computer => computer.id !== id))
      else if (action === "setup") update(id, { state: "setting-up", detail: "Creating the account" })
      else update(id, { state: action === "start" ? "running" : "stopped", displayOpen: false, installed: true, setupComplete: true })
    },
    openDisplay: async id => update(id, { displayOpen: true }),
    createCheckpoint: async (id, name) => {
      const computer = state.computers.find(candidate => candidate.id === id)
      if (!computer) throw new Error("This computer no longer exists.")
      const checkpoint = { id: crypto.randomUUID(), name, createdAt: new Date().toISOString(), scope: computer.state === "running" ? "full" : "disk", reason: "manual" } as const
      update(id, { checkpoints: [checkpoint, ...(computer.checkpoints ?? [])] })
    },
    restoreCheckpoint: async (id, checkpointId) => {
      const computer = state.computers.find(candidate => candidate.id === id)
      const target = computer?.checkpoints?.find(checkpoint => checkpoint.id === checkpointId)
      if (!computer || !target) throw new Error("This checkpoint no longer exists.")
      const recovery = { id: crypto.randomUUID(), name: "Before restore", createdAt: new Date().toISOString(), scope: computer.state === "running" ? "full" : "disk", reason: "before-restore" } as const
      update(id, { state: "stopped", checkpoints: [recovery, ...(computer.checkpoints ?? [])], pendingRestore: { checkpointId, memory: target.scope === "full" } })
    },
    forkCheckpoint: async (id, checkpointId, newName) => {
      const computer = state.computers.find(candidate => candidate.id === id)
      if (!computer?.checkpoints?.some(checkpoint => checkpoint.id === checkpointId)) throw new Error("This checkpoint no longer exists.")
      change([...state.computers, { ...computer, id: `mac-${newName}`, name: newName, state: "setting-up", detail: "Personalizing the computer", setupComplete: false, checkpoints: [], checkpointOperation: null, pendingRestore: null }])
    },
    deleteCheckpoint: async (id, checkpointId) => {
      const computer = state.computers.find(candidate => candidate.id === id)
      update(id, { checkpoints: (computer?.checkpoints ?? []).filter(checkpoint => checkpoint.id !== checkpointId) })
    },
    deleteTemplate: async () => {
      state = { ...state, template: null, minDiskGiB: 32 }
      handlers.forEach(handler => handler(state))
    },
    clipboard: async (_id, direction) => direction === "paste-into"
      ? { action: "paste", status: "pasted", content: "text", message: null }
      : { action: "copy", status: "copied", content: "text", message: null },
  }
}

export const createFixtureMacosComputersStore = () => createMacosComputersStore(createFixtureMacosComputersBackend())
