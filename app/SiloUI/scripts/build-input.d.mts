import type { Readable } from "node:stream"

export type FetchStream = (url: string, options: { signal: AbortSignal }) => Promise<Readable>
export interface InputLimits {
  bytes?: number
  maxBytes?: number
  timeoutMs?: number
}
export const fetchStream: FetchStream
export function verifyFile(path: string, expectedSha256: string, label: string, limits?: InputLimits): Promise<void>
export function isVerifiedFile(path: string, expectedSha256: string, label: string, limits?: InputLimits): Promise<boolean>
export function writeVerifiedFile(path: string, openSource: (signal: AbortSignal) => Promise<Readable>, expectedSha256: string, label: string, limits?: InputLimits): Promise<string>
export function fetchVerifiedFile(fetchInput: FetchStream, url: string, expectedSha256: string, label: string, path: string, limits?: InputLimits): Promise<string>
export function runtimeCacheRoot(tauriRoot: string, environment?: Record<string, string | undefined>): string
