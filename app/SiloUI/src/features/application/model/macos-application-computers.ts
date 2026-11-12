import { useCallback, useMemo, useState } from "react"

import { showActionFailure } from "@/lib/operation-toast"
import {
  macosStateLabel,
  remoteMacosComputerId,
  useMacosComputers,
  type MacosComputer,
  type MacosComputersSnapshot,
  type MacosComputersStore,
} from "@/features/macos-computers/model/macos-computers"
import { computerTarget, remoteComputerTarget, type Device } from "./connections"
import type { ApplicationActions, ApplicationComputer, ApplicationSource, ComputerState } from "./application-source"

function computerState(state: MacosComputer["state"]): ComputerState {
  if (state === "running" || state === "failed") return state
  return state === "stopped" ? "stopped" : "starting"
}

/** What the application knows about the computers of this device that the macOS record does not carry. */
export interface MacosListingContext {
  /** Repositories each computer may use, by computer name. */
  repositories?: ReadonlyMap<string, readonly string[]>
  /** Names of the secrets assigned to each computer, by computer name. */
  secrets?: ReadonlyMap<string, readonly string[]>
  /** The lifecycle action in flight, by the computer's target. */
  pending?: ReadonlyMap<string, "start" | "stop">
  capacity?: ApplicationSource["deviceCapacity"]
}

/**
 * A macOS computer as the application lists it: the same shape as a Linux computer, with the
 * setup state in `stateDetail` and the macOS record in `macos`. One hosted by another device
 * carries that device and takes the id a remote computer has, so its logs are read from that
 * device by the owner's own id.
 */
export function macosApplicationComputer(computer: MacosComputer, device?: Device, context: MacosListingContext = {}): ApplicationComputer {
  const local = !device
  const listed: ApplicationComputer = {
    ...(device && { device: { ...device, computerId: computer.id } }),
    os: "macos",
    // A computer of another device is addressed by its target, like the store does.
    macos: device ? { ...computer, id: remoteComputerTarget(device.id, computer.id) } : computer,
    configuration: {
      id: device ? remoteComputerTarget(device.id, computer.id) : computer.id,
      name: computer.name,
      cpus: computer.cpus,
      maxCPUs: Math.max(context.capacity?.logicalCpus ?? 0, computer.cpus),
      memoryGiB: computer.memoryGiB,
      maxMemoryGiB: Math.max(context.capacity?.maxMemoryGib ?? 0, computer.memoryGiB),
      workspaceStorageGiB: computer.diskGiB,
      runtimeStorageGiB: 1,
    },
    purpose: "macOS",
    state: computerState(computer.state),
    stateDetail: computer.state === "stopping" ? "Stopping" : macosStateLabel(computer),
    freshness: "fresh",
    repositories: [],
    files: [],
    ports: [],
    logs: [],
    githubRepositories: local ? [...(context.repositories?.get(computer.name) ?? [])] : [],
    secretNames: local ? [...(context.secrets?.get(computer.name) ?? [])] : [],
    ...(computer.state === "failed" && computer.detail && { attention: { level: "error" as const, message: computer.detail } }),
    ...(computer.checkpointOperation && { checkpointOperation: computer.checkpointOperation }),
    ...(computer.quiet && { quiet: computer.quiet }),
  }
  const pending = context.pending?.get(computerTarget(listed))
  return pending ? { ...listed, lifecycleAction: pending } : listed
}

/** The ids of the macOS computers the Logs page may still filter on: this device's, and those of devices that are still connected. */
export function selectableMacosIds(snapshot: MacosComputersSnapshot | undefined, devices: readonly Pick<Device, "id">[] | undefined): string[] {
  return [
    ...(snapshot?.state?.computers.map(({ id }) => id) ?? []),
    ...Object.entries(snapshot?.remote ?? {})
      .filter(([deviceId]) => devices?.some(({ id }) => id === deviceId))
      .flatMap(([deviceId, remote]) => remote.state?.computers.map(({ id }) => remoteMacosComputerId(deviceId, id)) ?? []),
  ]
}

/** The macOS computers of this device and of the source's connected devices, in the order the pages list them. */
export function macosApplicationComputers(snapshot: MacosComputersSnapshot | undefined, source: Pick<ApplicationSource, "devices">, context: MacosListingContext = {}): ApplicationComputer[] {
  const local = snapshot?.state?.supported ? snapshot.state.computers.map(computer => macosApplicationComputer(computer, undefined, context)) : []
  const remote = (source.devices ?? []).flatMap(device => {
    const hosted = snapshot?.remote[device.id]?.state
    return hosted?.supported ? hosted.computers.map(computer => macosApplicationComputer(computer, device, context)) : []
  })
  return [...local, ...remote]
}

/** `source` with the macOS computers listed after the Linux ones. */
export function withMacosComputers(source: ApplicationSource, snapshot: MacosComputersSnapshot | undefined, pending?: MacosListingContext["pending"]): ApplicationSource {
  const repositories = new Map(source.github.computers?.map(policy => [policy.computer, policy.repositories.map(({ repository }) => repository)]))
  const secrets = new Map<string, string[]>()
  for (const secret of source.secrets) for (const name of secret.computers) secrets.set(name, [...(secrets.get(name) ?? []), secret.name])
  const macos = macosApplicationComputers(snapshot, source, { repositories, secrets, pending, capacity: source.deviceCapacity })
  return macos.length ? { ...source, computers: [...source.computers, ...macos] } : source
}

type LifecycleKind = "start" | "stop"

/**
 * The application's actions with the ones that name a macOS computer sent to the macOS store: Start,
 * Stop and Show screen. Terminal, editor, files, ports, secrets and GitHub reach the engine by the
 * computer's name for either system.
 */
export function macosApplicationActions(actions: ApplicationActions, computers: readonly ApplicationComputer[], store: MacosComputersStore, run: (target: string, kind: LifecycleKind, task: () => Promise<void>) => void): ApplicationActions {
  const macos = new Map(computers.filter(computer => computer.macos).map(computer => [computerTarget(computer), computer]))
  const lifecycle = (kind: LifecycleKind, fallback: (target: string) => void) => (target: string) => {
    const computer = macos.get(target)
    if (!computer?.macos) return fallback(target)
    run(target, kind, () => store.action(computer.configuration.id, kind))
  }
  return {
    ...actions,
    startComputer: lifecycle("start", actions.startComputer),
    stopComputer: lifecycle("stop", actions.stopComputer),
    openDesktop: async target => {
      const computer = macos.get(target)
      if (!computer) return actions.openDesktop?.(target)
      try { await store.openDisplay(computer.configuration.id) } catch (error) { showActionFailure(`Could not show the screen of ${computer.configuration.name}`, error, undefined, { native: false }) }
    },
  }
}

/** The source and actions every page reads, with this device's macOS computers listed beside the Linux ones. */
export function useMacosApplication(source: ApplicationSource, actions: ApplicationActions): { source: ApplicationSource; actions: ApplicationActions } {
  const macos = useMacosComputers()
  const snapshot = macos?.snapshot
  const store = macos?.store
  const [pending, setPending] = useState<ReadonlyMap<string, LifecycleKind>>(new Map())
  const merged = useMemo(() => snapshot ? withMacosComputers(source, snapshot, pending) : source, [source, snapshot, pending])
  const run = useCallback((target: string, kind: LifecycleKind, task: () => Promise<void>) => {
    const name = target.startsWith("silo-remote:") ? "the computer" : target
    setPending(current => new Map(current).set(target, kind))
    task()
      .catch((error: unknown) => showActionFailure(`Could not ${kind} ${name}`, error, undefined, { native: false }))
      .finally(() => setPending(current => { const next = new Map(current); next.delete(target); return next }))
  }, [])
  const wrapped = useMemo(() => store ? macosApplicationActions(actions, merged.computers, store, run) : actions, [actions, merged.computers, store, run])
  return { source: merged, actions: wrapped }
}
