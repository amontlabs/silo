import { createMacosComputersStore, type MacosComputer, type MacosComputersBackend, type MacosComputersState } from "@/features/macos-computers/model/macos-computers"

export const macosComputerFixtures: readonly MacosComputer[] = [
  { id: "mac-download", name: "sequoia-test", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: null, state: "downloading", progress: 0.42, detail: null, displayOpen: false },
  { id: "mac-install", name: "release-check", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "installing", progress: 0.63, detail: null, displayOpen: false },
  { id: "mac-stopped", name: "xcode-build", cpus: 6, memoryGiB: 16, diskGiB: 128, osVersion: "26.6.2 (25G83)", state: "stopped", progress: null, detail: null, displayOpen: false },
  { id: "mac-running", name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "running", progress: null, detail: null, displayOpen: false },
  { id: "mac-failed", name: "broken", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: null, state: "failed", progress: null, detail: "The macOS download did not finish. Check your connection and create the computer again.", displayOpen: false },
]

/** Keeps the computers in memory; start, stop and delete change them at once and create adds a downloading row. */
export function createFixtureMacosComputersBackend(initial: readonly MacosComputer[] = macosComputerFixtures): MacosComputersBackend {
  let state: MacosComputersState = { supported: true, unsupportedReason: null, computers: [...initial] }
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
      const created: MacosComputer = { id: `mac-${request.name}`, ...request, osVersion: null, state: "preparing", progress: null, detail: null, displayOpen: false }
      change([...state.computers, created])
      return created
    },
    action: async (id, action) => {
      if (action === "delete") change(state.computers.filter(computer => computer.id !== id))
      else update(id, { state: action === "start" ? "running" : "stopped", displayOpen: false })
    },
    openDisplay: async id => update(id, { displayOpen: true }),
  }
}

export const createFixtureMacosComputersStore = () => createMacosComputersStore(createFixtureMacosComputersBackend())
