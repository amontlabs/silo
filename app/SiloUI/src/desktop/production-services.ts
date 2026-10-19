import { isUnsupportedRemote } from "@/features/application/model/logs"
import type { SshAccessRequest, NetworkState, SshAccessComputer, SshAccessState, ApplicationSource } from "@/features/application/model/application-source"
import { remoteComputerTarget, parseRemoteComputerTarget, type Device } from "@/features/application/model/connections"

import type { ProductionContext } from "./production-context"
import { NETWORK_AMBIENT_INTERVAL_MS, NETWORK_INTEREST_MS, REMOTE_READ_WAIT_MS, computerOwner, errorMessage } from "./production-helpers"
import { parseNetworkState, parseSshAccessState } from "./production-schemas"

/** What reading a device's services needs to know about the devices the source lists. */
export interface DeviceDirectory {
  devices: () => Device[]
  remoteSnapshots: ReadonlyMap<string, ApplicationSource>
  /** This device's own name, for rows shown while its SSH state is unavailable. */
  localName: () => string | undefined
  /** Whether live updates have started. */
  live: () => boolean
}

/** SSH access and network services of this device and its connected devices: their reads, saves and watchers. */
export function createDeviceServices({ native, snapshot, publish, disposed, devices, remoteSnapshots, localName, live }: ProductionContext & DeviceDirectory) {
  let sshAccess: SshAccessState | undefined
  let sshAccessError: string | null = null
  const sshReads = new Map<string, { dirty: boolean; promise: Promise<void> }>()
  const sshFailures = new Map<string, { delay: number; nextRead: number }>()
  const sshSaveRevisions = new Map<string, number>()
  const sshReadRevisions = new Map<string, number>()
  let network: NetworkState | undefined
  let networkError: string | null = null
  let networkWatchers = 0
  let networkAmbientWatchers = 0
  let lastNetworkReadAt = 0
  let networkInterestUntil = 0
  const networkReads = new Map<string, { dirty: boolean; promise: Promise<void> }>()
  const networkFailures = new Map<string, { delay: number; nextRead: number }>()
  const networkReadRevisions = new Map<string, number>()
  const networkSaveRevisions = new Map<string, number>()

  function unavailableSshRows(deviceId: string, deviceName: string, message: string): SshAccessComputer[] {
    const cached = sshAccess?.computers.filter(row => computerOwner(row.computer) === deviceId) ?? []
    const computers = deviceId ? remoteSnapshots.get(deviceId)?.computers ?? [] : snapshot().source?.computers.filter(w => !w.device) ?? []
    const rows = new Map(cached.map(row => [row.computer, row]))
    for (const computer of computers) {
      const target = deviceId ? remoteComputerTarget(deviceId, computer.configuration.id) : computer.configuration.name
      if (!rows.has(target)) rows.set(target, { computer: target, enabled: false, port: 2222, bindAddress: "127.0.0.1", keys: [], state: "error", message, fingerprint: null, deviceName, addresses: [] })
    }
    return [...rows.values()].map(row => ({ ...row, unavailable: message }))
  }
  /** Each service read gets the same bounded wait as a device-state read. */
  function waitForService<T>(read: Promise<T>): Promise<T> {
    let timer: ReturnType<typeof setTimeout> | undefined
    const timeout = new Promise<T>((_, reject) => {
      timer = setTimeout(() => reject(new Error("Device is not responding.")), REMOTE_READ_WAIT_MS)
    })
    return Promise.race([read, timeout]).finally(() => clearTimeout(timer))
  }

  function readSshOwner(owner: string, background = false): Promise<void> {
    if (background && (sshFailures.get(owner)?.nextRead ?? 0) > Date.now()) return Promise.resolve()
    const pending = sshReads.get(owner)
    if (pending) { if (!background) pending.dirty = true; return pending.promise }
    const entry = { dirty: false, promise: Promise.resolve() }
    entry.promise = (async () => { do {
      entry.dirty = false
      const revision = sshReadRevisions.get(owner)
      const device = devices().find(item => item.id === owner)
      let rows: SshAccessComputer[]
      let unavailable: string | null = null
      try {
        if (owner && !device?.connected) throw new Error("Device is offline.")
        const result = parseSshAccessState(await waitForService(native.invoke(owner ? "remote_ssh_access_state" : "read_ssh_access_state", owner ? { deviceId: owner } : undefined)))
        if (result.computers.some(row => computerOwner(row.computer) !== owner)) throw new Error("SSH response belongs to another device.")
        rows = result.computers
        sshFailures.delete(owner)
      } catch (cause) {
        const delay = Math.min((sshFailures.get(owner)?.delay ?? 5000) * 2, 60_000)
        sshFailures.set(owner, { delay, nextRead: Date.now() + delay })
        unavailable = !owner ? "Could not check SSH access."
          : isUnsupportedRemote(cause) ? `Update Silo on ${device?.name} to manage SSH access. That version does not support remote SSH management.`
          : `SSH status on ${device?.name} is unavailable. Reconnect and refresh before changing access.`
        rows = unavailableSshRows(owner, device?.name ?? localName() ?? "This device", unavailable)
      }
      if (disposed()) return
      if (revision !== sshReadRevisions.get(owner)) continue
      const current = devices().find(item => item.id === owner)
      if (owner && !current) return
      if (owner && !current?.connected) rows = unavailableSshRows(owner, current!.name, `SSH status on ${current!.name} is unavailable. Reconnect and refresh before changing access.`)
      sshAccess = { computers: [...(sshAccess?.computers.filter(row => computerOwner(row.computer) !== owner) ?? []), ...rows] }
      if (!owner) sshAccessError = unavailable
      publish({ ...snapshot() })
    } while (entry.dirty && !disposed()) })().finally(() => { if (sshReads.get(owner) === entry) sshReads.delete(owner) })
    sshReads.set(owner, entry)
    return entry.promise
  }

  function refreshSshAccess(options?: { background?: boolean }): Promise<void> {
    return Promise.all([readSshOwner("", options?.background), ...devices().map(device => readSshOwner(device.id, options?.background))]).then(() => {})
  }

  function unavailableNetworkRows(owner: string, error: string): NetworkState["computers"] {
    const rows = new Map((network?.computers.filter(row => computerOwner(row.computer) === owner) ?? []).map(row => [row.computer, row]))
    const computers = owner ? remoteSnapshots.get(owner)?.computers ?? [] : snapshot().source?.computers ?? []
    for (const computer of computers) {
      const target = owner ? remoteComputerTarget(owner, computer.configuration.id) : computer.configuration.name
      if (!rows.has(target)) rows.set(target, { computer: target, ports: [], error })
    }
    return [...rows.values()].map(row => ({ ...row, error }))
  }

  // Events received during an owner's read request one follow-up for that owner.
  function readNetworkOwner(owner: string, background = false): Promise<void> {
    if (background && (networkFailures.get(owner)?.nextRead ?? 0) > Date.now()) return Promise.resolve()
    const pending = networkReads.get(owner)
    if (pending) { if (!background) pending.dirty = true; return pending.promise }
    const entry = { dirty: false, promise: Promise.resolve() }
    entry.promise = (async () => { do {
      entry.dirty = false
      const revision = networkReadRevisions.get(owner)
      const device = devices().find(item => item.id === owner)
      let rows: NetworkState["computers"]
      let unavailable: string | null = null
      try {
        if (owner && !device?.connected) throw new Error(`${device?.name} is offline. Reconnect to see network services.`)
        const result = parseNetworkState(await waitForService(native.invoke(owner ? "remote_network_state" : "read_network_state", owner ? { deviceId: owner } : undefined)))
        if (result.computers.some(row => computerOwner(row.computer) !== owner)) throw new Error("Network response belongs to another device.")
        rows = result.computers
        networkFailures.delete(owner)
      } catch (cause) {
        const delay = Math.min((networkFailures.get(owner)?.delay ?? 5000) * 2, 60_000)
        networkFailures.set(owner, { delay, nextRead: Date.now() + delay })
        unavailable = !owner ? "Could not check network services." : isUnsupportedRemote(cause) ? `Update Silo on ${device?.name} to see network services.` : errorMessage(cause)
        rows = unavailableNetworkRows(owner, unavailable)
      }
      if (disposed()) return
      if (revision !== networkReadRevisions.get(owner)) continue
      const current = devices().find(item => item.id === owner)
      if (owner && !current) return
      if (owner && !current?.connected) rows = unavailableNetworkRows(owner, `${current!.name} is offline. Reconnect to see network services.`)
      network = { computers: [...(network?.computers.filter(row => computerOwner(row.computer) !== owner) ?? []), ...rows] }
      if (!owner) networkError = unavailable
      publish({ ...snapshot() })
    } while (entry.dirty && !disposed()) })().finally(() => { if (networkReads.get(owner) === entry) networkReads.delete(owner) })
    networkReads.set(owner, entry)
    return entry.promise
  }

  function readNetwork(options?: { background?: boolean }): Promise<void> {
    lastNetworkReadAt = Date.now()
    return Promise.all([readNetworkOwner("", options?.background), ...devices().map(device => readNetworkOwner(device.id, options?.background))]).then(() => {})
  }
  /** Network services are read only for a consumer that shows them: a watcher, or a page that requested them recently. */
  function networkWanted() { return networkWatchers > 0 || networkAmbientWatchers > 0 || Date.now() < networkInterestUntil }
  /** A consumer's own request: it also marks network data as wanted while that consumer keeps asking. */
  function refreshNetwork(options?: { background?: boolean }): Promise<void> {
    networkInterestUntil = Date.now() + NETWORK_INTEREST_MS
    return readNetwork(options)
  }
  /** Keeps network services read until the returned function is called: with every refresh, or when `ambient`, at most every 30s and on network events. */
  function watchNetwork(options?: { ambient?: boolean }): () => void {
    const ambient = options?.ambient === true
    if (ambient) networkAmbientWatchers++; else networkWatchers++
    // Before live updates start, the first refresh reads network services for the watcher.
    if (live() && !disposed()) void readNetwork()
    let watching = true
    return () => { if (watching) { watching = false; if (ambient) networkAmbientWatchers--; else networkWatchers-- } }
  }
  async function changeNetwork(command: string, arguments_: Record<string, unknown>) {
    const target = arguments_.computer as string
    const remote = parseRemoteComputerTarget(target)
    const owner = remote?.deviceId ?? ""
    const revision = (networkSaveRevisions.get(target) ?? 0) + 1
    networkSaveRevisions.set(target, revision)
    networkReadRevisions.set(owner, (networkReadRevisions.get(owner) ?? 0) + 1)
    const { computer: _computer, ...rest } = arguments_
    let result: NetworkState
    try {
      result = parseNetworkState(await native.invoke(remote ? `remote_${command}` : command, remote ? { ...rest, ...remote } : arguments_))
      if (result.computers.some(row => computerOwner(row.computer) !== owner)) throw new Error("Network response belongs to another device.")
    } catch (cause) {
      networkReadRevisions.set(owner, (networkReadRevisions.get(owner) ?? 0) + 1)
      if (!disposed()) void readNetworkOwner(owner)
      throw cause
    }
    if (disposed()) return
    networkReadRevisions.set(owner, (networkReadRevisions.get(owner) ?? 0) + 1)
    if (revision !== networkSaveRevisions.get(target)) {
      void readNetworkOwner(owner)
      return
    }
    // A device-wide reply can carry older rows for unrelated computer saves.
    const retained = network?.computers.filter(row => row.computer !== target) ?? []
    network = { computers: [...retained, ...result.computers.filter(row => row.computer === target)] }
    if (!owner) networkError = null
    publish({ ...snapshot() })
  }


  async function saveSshAccess(request: SshAccessRequest) {
    const remote = parseRemoteComputerTarget(request.computer)
    const owner = remote?.deviceId ?? ""
    if (sshAccess?.computers.find(row => row.computer === request.computer)?.unavailable || (remote && !devices().find(device => device.id === remote.deviceId)?.connected)) throw new Error("Refresh SSH status before changing access.")
    const revision = (sshSaveRevisions.get(request.computer) ?? 0) + 1
    sshSaveRevisions.set(request.computer, revision)
    sshReadRevisions.set(owner, (sshReadRevisions.get(owner) ?? 0) + 1)
    const { computer: _computer, ...settings } = request
    const result = parseSshAccessState(await native.invoke(remote ? "remote_save_ssh_access" : "save_ssh_access", remote ? { ...remote, ...settings } : { ...request }))
    if (result.computers.some(row => computerOwner(row.computer) !== owner)) throw new Error("SSH response belongs to another device.")
    if (disposed() || sshSaveRevisions.get(request.computer) !== revision) return
    if (remote && !devices().find(device => device.id === remote.deviceId)?.connected) return
    sshReadRevisions.set(owner, (sshReadRevisions.get(owner) ?? 0) + 1)
    const retained = sshAccess?.computers.filter(row => row.computer !== request.computer) ?? []
    sshAccess = { computers: [...retained, ...result.computers.filter(row => row.computer === request.computer)] }
    if (!remote) sshAccessError = null
    publish({ ...snapshot() })
  }

  /** Whether an ambient or continuous watcher wants a network read now, which is the case on every poll for a watcher and at a lower rate for an ambient one. */
  function networkReadDue() {
    return networkWatchers > 0 || (networkAmbientWatchers > 0 && Date.now() - lastNetworkReadAt >= NETWORK_AMBIENT_INTERVAL_MS)
  }

  /** Drops the read backoff of every device that is no longer listed or whose address changed. */
  function forgetFailures(changed: (deviceId: string) => boolean) {
    for (const failures of [sshFailures, networkFailures]) for (const id of failures.keys()) {
      if (id && changed(id)) failures.delete(id)
    }
  }

  return {
    sshAccess: () => sshAccess,
    sshAccessError: () => sshAccessError,
    network: () => network,
    networkError: () => networkError,
    refreshSshAccess,
    saveSshAccess,
    readNetwork,
    refreshNetwork,
    networkWanted,
    networkReadDue,
    watchNetwork,
    changeNetwork,
    forgetFailures,
  }
}
