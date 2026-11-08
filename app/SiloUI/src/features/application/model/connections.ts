import { z } from "zod"
import type { ApplicationComputer } from "./application-source"

export const deviceSchema = z.object({ id: z.string().min(1), name: z.string().min(1), address: z.string().min(1), platform: z.enum(["macos", "linux"]).optional().catch(undefined) })
export const connectionsStatusSchema = z.object({ enabled: z.boolean(), deviceId: z.string(), name: z.string(), address: z.string(), addresses: z.array(z.object({ address: z.string().min(1), kind: z.enum(["name", "tailscale", "network"]) })).optional(), error: z.string().nullish(), cliLinkError: z.string().nullish() })
export type Device = z.infer<typeof deviceSchema> & { connected: boolean; busy?: boolean; error?: string; lastSeen?: number }
export type ConnectionsStatus = z.infer<typeof connectionsStatusSchema>
export type ComputerDevice = Device & { computerId: string }

export function remoteComputerTarget(deviceId: string, computerId: string): string {
  return `silo-remote:${encodeURIComponent(deviceId)}:${encodeURIComponent(computerId)}`
}
export function parseRemoteComputerTarget(target: string): { deviceId: string; computerId: string } | undefined {
  if (!target.startsWith("silo-remote:")) return undefined
  const parts = target.split(":")
  if (parts.length !== 3 || !parts[1] || !parts[2]) throw new Error("Silo could not identify the remote computer. Refresh its device and retry.")
  return { deviceId: decodeURIComponent(parts[1]), computerId: decodeURIComponent(parts[2]) }
}
export function computerTarget(computer: ApplicationComputer): string {
  return computer.device ? remoteComputerTarget(computer.device.id, computer.device.computerId) : computer.configuration.name
}
