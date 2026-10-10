import { remoteMacosComputerId, type MacosComputer, type MacosComputersSnapshot } from "@/features/macos-computers/model/macos-computers"
import { remoteComputerTarget, type Device } from "./connections"
import type { ApplicationComputer, ComputerState } from "./application-source"

function logState(state: MacosComputer["state"]): ComputerState {
  return state === "running" || state === "starting" || state === "failed" ? state : "stopped"
}

/**
 * A macOS computer of this device as the Logs page lists it: only its name, id and state matter
 * there, because its setup log is read by id like any other computer's retained logs.
 * One hosted by another device carries that device and takes the id a remote computer has, so its
 * logs are read from that device by the owner's own id.
 */
export function macosLogComputer(computer: MacosComputer, device?: Device): ApplicationComputer {
  return {
    ...(device && { device: { ...device, computerId: computer.id } }),
    configuration: {
      id: device ? remoteComputerTarget(device.id, computer.id) : computer.id,
      name: computer.name,
      cpus: computer.cpus,
      maxCPUs: computer.cpus,
      memoryGiB: computer.memoryGiB,
      maxMemoryGiB: computer.memoryGiB,
      workspaceStorageGiB: computer.diskGiB,
      runtimeStorageGiB: 1,
    },
    purpose: "macOS",
    state: logState(computer.state),
    stateDetail: computer.detail ?? "",
    freshness: "fresh",
    repositories: [],
    files: [],
    ports: [],
    logs: [],
    githubRepositories: [],
    secretNames: [],
  }
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
