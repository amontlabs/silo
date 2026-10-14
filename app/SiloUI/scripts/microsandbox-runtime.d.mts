import type { FetchStream } from "./build-input.mjs"

export interface RuntimeTarget {
  platform: string
  arch: string
  executableAsset: string
  executableSha256: string
  agentdAsset: string
  agentdSha256: string
  libraryAsset: string
  libraryName: string
  librarySha256: string
}

export interface LicenseArtifact {
  name: string
  url: string
  sha256: string
}

export const MICRO_SANDBOX_VERSION: string
export const LIBKRUNFW_VERSION: string
export const RELEASE_BASE_URL: string
export const MICROSANDBOX_SOURCE_URL: string
export const MICROSANDBOX_SOURCE_SHA256: string
export const MICROSANDBOX_PATCHES: readonly { path: string; sha256: string }[]
export const MICROSANDBOX_BUILD_TOOLCHAIN: string
export const MICROSANDBOX_BUILD_FEATURES: string
export const runtimeTargets: Readonly<Record<string, Readonly<RuntimeTarget>>>
export const licenseArtifacts: readonly Readonly<LicenseArtifact>[]
export function resolveRuntimeTarget(
  environment: Readonly<Record<string, string | undefined>>,
  hostTriple: () => string,
): string
export function selectRuntime(targetTriple: string): Readonly<RuntimeTarget>
export function sha256(bytes: ArrayBufferView): string
export function applyRuntimePatch(sourceRoot: string, patchPath: string): void
export function verifySha256(bytes: ArrayBufferView, expected: string, label: string): void
export function buildPatchedExecutable(options: {
  appRoot?: string
  targetTriple: string
  hostTriple: string
  sourceArchive: string
  patches: ArrayBufferView[]
  agentd: ArrayBufferView
  cacheRoot: string
}): Promise<Buffer | { executable: Buffer; macosGateway: Buffer }>
export function stageRuntime(options: {
  appRoot: string
  targetTriple: string
  hostTriple?: string
  fetchStream: FetchStream
  selected?: RuntimeTarget
  licenses?: LicenseArtifact[]
  sourceArtifact?: { url: string; sha256: string }
  buildExecutable?: (options: {
    appRoot: string
    targetTriple: string
    hostTriple: string
    sourceArchive: string
    patches: ArrayBufferView[]
    agentd: ArrayBufferView
    cacheRoot: string
  }) => Promise<ArrayBufferView | { executable: ArrayBufferView; macosGateway?: ArrayBufferView }>
  verifyExecutable?: boolean
}): Promise<{
  targetTriple: string
  executablePath: string
  gatewayPath: string | null
  libraryPath: string
  manifestPath: string
}>
