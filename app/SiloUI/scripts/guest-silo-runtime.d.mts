export const ZIG_VERSION: string
export const GUEST_SILO_MANIFEST_VERSION: number
export function guestSiloTarget(targetTriple: string): { architecture: 'arm64' | 'amd64'; rustTarget: string; zigTarget: string }
export function guestSiloRequired(environment: NodeJS.ProcessEnv): boolean
export function stageGuestSilo(options: {
  appRoot: string
  targetTriple: string
  environment?: NodeJS.ProcessEnv
  run?: (program: string, args: string[], options?: { cwd?: string; env?: NodeJS.ProcessEnv; maxBuffer?: number }) => Promise<{ stdout: string }>
  required?: boolean
  log?: (message: string) => void
}): Promise<{ available: boolean; reason?: string; root: string; binaryPath: string; manifestPath: string }>
