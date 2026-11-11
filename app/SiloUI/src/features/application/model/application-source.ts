import type { LifecycleStep } from "./lifecycle-progress"
import type { OperationQueue } from "./operation-queue"
import type { WorkspaceStorageState } from "./workspace-storage"
import type { ComputerStep, ComputerTreeNode } from "@/desktop/production-schemas"
import type { MacosComputer, QuietState } from "../../macos-computers/model/macos-computers"
import type { CheckpointUsage, PendingCheckpointRestore, UnfinishedRestore, ComputerCheckpoint, ComputerCheckpointOperation } from "./checkpoint-source"
import type { LogLoader, LogQuery } from "./logs"
import type { Device, ConnectionsStatus, ComputerDevice } from "./connections"
import type { DirectoryLoader } from "./directory-store"
import type { FileTransferActions } from "./file-transfer"
import type {
  SetupComputerConfiguration,
  SetupComputerConfigurationRequest,
  SiloBootstrapResult,
  SiloProgressEvent,
  SiloProtocolError,
} from "@/contracts/silo"
import type { AgentId, AgentInfo, AgentInstallProgress, Approval, ApprovalJob, ApprovalList, ApprovalState, AuditCallerKind, AuditPage, ComputerTools, DeviceResources, NetworkPolicy, NetworkPolicyState, RequestList, SshName } from "@/desktop/production-schemas"
import type { ApplicationPreferenceSelection } from "@/features/preferences/model/application-preferences"

export type ApplicationTab = "computers" | "changes" | "github" | "secrets" | "system" | "settings"
export type SettingsSection = "general" | "connections" | "notifications"
export type ComputerSection = "overview" | "files" | "logs" | "network" | "activity"
export type ComputerDetailSection = Exclude<ComputerSection, "overview">
/** Tabs on a single computer's detail page, reached from the Computers overview. */
export type ComputerDetailTab = "overview" | "checkpoints" | "storage" | "access"
export type ComputerState = "running" | "starting" | "stopped" | "failed"
/** The system a computer runs. */
export type ComputerOs = "linux" | "macos"

export type RuntimeRepairPresentation = {
  status: "needed" | "unavailable"
  reason: string
  recovery?: string
  checking?: boolean
}

export type ActiveRuntimeRepairPresentation = RuntimeRepairPresentation

export type ComputerConfigurationOperation =
  | {
      id: string
      status: "applying"
      candidate: SetupComputerConfigurationRequest
      progressEvents: readonly SiloProgressEvent[]
      result: null
      error: null
    }
  | {
      id: string
      status: "awaiting-approval"
      candidate: SetupComputerConfigurationRequest
      progressEvents: readonly SiloProgressEvent[]
      result: SiloBootstrapResult
      error: null
    }
  | {
      id: string
      status: "failed"
      candidate: SetupComputerConfigurationRequest
      progressEvents: readonly SiloProgressEvent[]
      result: null
      error: SiloProtocolError
    }

export interface ApplicationRepository {
  path: string
  branch: string
  ahead: number
  behind: number
  dirty: boolean
  /** GitHub `owner/name` of the `origin` remote; absent when it is not a GitHub repository or the owner predates push binding. */
  repository?: string | null
  /** Commit at the tip of `branch`. */
  head?: string | null
}

/** The repository, branch and commit the user confirmed; the host pushes exactly these or nothing. */
export interface RepositoryPushTarget {
  repository: string
  branch: string
  commit: string
}

export type RepositoryPushOperation = {
  /** The host-owned push this result belongs to; older hosts may omit it. */
  operationId?: string
  computer: string
  repositoryPath: string
  commitCount: number
  /** What this push publishes, as confirmed by the user. */
  target?: RepositoryPushTarget
} & (
  | { status: "pushing"; message?: string }
  | { status: "unknown"; message: string }
  | { status: "succeeded" }
  | { status: "failed"; message: string; diagnosticDetails?: string }
)

export interface ApplicationLog {
  line: string
  occurredAt: string
}

export interface NetworkPort {
  configuredHostPort?: number | null
  port: number
  hostPort: number | null
  scheme: "http" | "https" | null
  state: "reachable" | "waiting" | "unpublished" | "unknown"
  configured: boolean
  message?: string | null
}
export interface NetworkState { computers: { computer: string; ports: NetworkPort[]; error: string | null; /** Host name published websites open at; absent means 127.0.0.1. */ host?: string | null }[] }
export interface SshAccessComputer {
  unavailable?: string
  computer: string; enabled: boolean; port: number; bindAddress: string; keys: string[]
  state: "disabled" | "waiting" | "listening" | "error"; message: string | null
  fingerprint: string | null; deviceName: string; addresses: string[]
  /** Guest account SSH clients log in as; older owners omit it. */
  user?: string
}
export interface SshAccessState { computers: SshAccessComputer[] }
export type SshAccessRequest = Pick<SshAccessComputer, "computer" | "enabled" | "port" | "bindAddress"> & { keys?: string[] }
export interface NetworkPortRequest { computer: string; port: number; hostPort: number | null; scheme: "http" | "https" | null }

export interface ApplicationPort {
  /** Host supplied by port forwarding; absent means 127.0.0.1. */
  host?: string | null
  hostPort?: number | null
  scheme?: "http" | "https" | null
  configured?: boolean
  port: number
  listening: boolean | null
}

export interface ApplicationFileEntry {
  name: string
  kind: "folder" | "file"
  children?: ApplicationFileEntry[]
}

export type ApplicationActivityCategory = "computer" | "git" | "backup" | "secrets" | "github" | "system"

export type ApplicationActivityStatus = "running" | "completed"

export interface ApplicationActivity {
  /** Filtered runtime output for a Details disclosure. */
  diagnostic?: string
  /** Completed setup changes were retained after a later failure. */
  partial?: boolean
  id: string
  category: ApplicationActivityCategory
  title: string
  detail: string
  occurredAt: string
  time: string
  tone: "success" | "neutral" | "warning" | "danger"
  status: ApplicationActivityStatus
  computer?: string
  progress?: number
  progressLabel?: string
  /** A start, stop or restart the user cancelled: neither a failure nor a success. */
  cancelled?: boolean
}

export interface ApplicationComputer {
  device?: ComputerDevice
  /** The system the computer runs; absent means Linux. */
  os?: ComputerOs
  /** A macOS computer's own record (setup state, progress, checkpoints); absent on Linux. */
  macos?: MacosComputer
  configuration: SetupComputerConfiguration
  purpose: string
  state: ComputerState
  stateDetail: string
  canDismissError?: boolean
  lifecycleFailure?: string
  /** Filtered runtime output for a Details disclosure; never inline summary text. */
  lifecycleFailureDiagnostic?: string
  /** The lifecycle action that failed, so the UI can offer a matching Retry that
   * re-submits the same intent (re-reading fresh state server-side). */
  lifecycleFailureAction?: "start" | "stop" | "restart" | "dismiss-error"
  /** True when the last lifecycle attempt was cancelled by the user rather than
   * failing. Rendered as a neutral, retryable state instead of an error. */
  lifecycleFailureCancelled?: boolean
  lifecycleAction?: "start" | "stop" | "restart" | "dismiss-error"
  /** Where a pending start or restart is, once the backend reports it. */
  lifecycleStep?: LifecycleStep
  attention?: {
    level: "warning" | "error"
    message: string
  }
  freshness: "fresh" | "stale"
  /** The native read overlapped an operation; runtime fields retain their last settled values. */
  settling?: boolean
  repositories: ApplicationRepository[]
  files: ApplicationFileEntry[]
  ports: ApplicationPort[]
  logs: ApplicationLog[]
  githubRepositories: string[]
  secretNames: string[]
  /** Removed secrets that this computer may still hold until revocation or restart. */
  pendingSecretRevocations?: string[]
  checkpoints?: ComputerCheckpoint[]
  checkpointOperation?: ComputerCheckpointOperation | null
  pendingCheckpointRestore?: PendingCheckpointRestore | null
  unfinishedRestore?: UnfinishedRestore | null
  /** What a fork was made quiet with. */
  quiet?: QuietState | null
}

/** A secret reaches a computer as a placeholder for its allowed hosts (`generic`), or only as
 * the computer's GitHub access (`github-token`). */
export type SecretKind = "generic" | "github-token"

export interface ApplicationSecret {
  id: string
  name: string
  /** Absent means `generic`. */
  kind?: SecretKind
  /** For a GitHub token, the computers whose GitHub access uses it. */
  computers: string[]
  allowedDomains: string[]
  /** The GitHub account a token belongs to, once GitHub confirmed it. */
  account?: string
  /** Whether GitHub accepts a token; a disconnected token gives its computers no access. */
  connection?: { state: "connected" | "disconnected"; message?: string }
  state: "active" | "applying" | "restart-required"
  pendingComputers?: string[]
  error?: string
  removing?: boolean
}

// Values travel only with a save request, never in the published secret metadata.
export type SecretConfigurationRequest = {
  name: string
  kind?: SecretKind
  computers: string[]
  allowedDomains: string[]
} & ({ operation: "add"; value: string } | { operation: "edit"; id: string; value?: string })

export interface ApplicationGitIdentity {
  name: string
  email: string
}

export interface ApplicationComputerGitIdentity extends ApplicationGitIdentity {
  apply: boolean
}

export interface ApplicationGitHubRepositoryPolicy {
  repository: string
  allowPushes: boolean
}

export interface ApplicationGitHubComputerPolicy {
  authenticationMethod?: "oauth" | "token"
  /** With token access, the `github-token` secret this computer uses. */
  tokenSecretId?: string
  /** Set by the engine when the chosen token secret no longer exists. */
  tokenMissing?: boolean
  repositoryMode?: "selected" | "all"
  allRepositoriesAllowChanges?: boolean
  computer: string
  identity: ApplicationComputerGitIdentity
  repositories: readonly ApplicationGitHubRepositoryPolicy[]
}

/**
 * A save of computer GitHub choices. `computers` lists only the computers being changed;
 * other computers keep their saved choices. `baseRevision` is the `policyRevision` the
 * edit was based on, so a change made meanwhile (such as a fork's copied assignment) is
 * not overwritten. Access on/off is changed only through `setGitHubAccessEnabled`.
 */
export interface ApplicationGitHubConfiguration {
  baseRevision?: number
  deviceIdentity: ApplicationGitIdentity | null
  computers: readonly ApplicationGitHubComputerPolicy[]
}

export type GitHubComputerOperation = {
  computer: string
  message: string
} & (
  | { status: "applying" }
  | { status: "succeeded" }
  | { status: "failed"; canRetry: true; diagnosticDetails?: string }
)

export type GitHubRepositoryCatalogStatus =
  | { status: "available" }
  | { status: "unavailable"; message: string; canRetry: boolean }

export interface ApplicationSource {
  devices?: Device[]
  connections?: ConnectionsStatus
  connectionsError?: string
  /** Silo could not read its list of connected devices; the listed ones are the last known. */
  devicesError?: string
  sshAccess?: SshAccessState
  sshAccessError?: string | null
  network?: NetworkState
  networkError?: string | null
  runtimeRepair: RuntimeRepairPresentation | null
  /** Ordered admission queue for computer-changing operations on this device. */
  operationQueue?: OperationQueue
  computers: ApplicationComputer[]
  activities: ApplicationActivity[]
  computerConfigurationOperation: ComputerConfigurationOperation | null
  repositoryPushOperations: RepositoryPushOperation[]
  github: {
    policyRevision?: number
    state: "disconnected" | "connecting" | "connected"
    account?: string
    /** Optional until every native source publishes the richer management snapshot. */
    accessEnabled?: boolean
    repositoryCatalog?: readonly string[]
    repositoryCatalogStatus?: GitHubRepositoryCatalogStatus
    deviceIdentity?: ApplicationGitIdentity | null
    computers?: readonly ApplicationGitHubComputerPolicy[]
    computerOperations?: readonly GitHubComputerOperation[]
  }
  secrets: ApplicationSecret[]
  backup: {
    lastArchive: string
    completedLabel: string
    compressedSize: string
    destination: string
  }
  /** Operation-owned capacity evidence. Fixtures set this only through an explicit scenario. */
  resourceNotice?:
    | { kind: "create-storage"; computer: string; requiredGB: number; availableGB: number; volume: string }
    | { kind: "start-memory"; computer: string; memoryGiB: number }
  computerOperationsUnavailable?: string
  /** This device's limits for computer CPU and memory ceilings (D-46); absent when unmeasured. */
  deviceCapacity?: { logicalCpus: number; physicalMemoryBytes: number; maxMemoryGib: number }
  preferences: ApplicationPreferenceSelection & {
    launchAtLogin: boolean
    startComputersAtLaunch: boolean
    startupComputerIds?: string[]
    reduceMotion: boolean
  }
}

export interface ApplicationActions {
  /** What the review screen runs; the desktop's when omitted. */
  reviewRequestActions?: import("@/features/application/components/review-request").ReviewRequestActions
  /** The review requests of this device and, with `allDevices`, every connected device. */
  listReviewRequests?: (options?: { status?: "open" | "kept" | "thrown-away" | "all"; allDevices?: boolean }) => Promise<RequestList>
  /** What the agents inside computers ask the person who runs Silo for. */
  listApprovals?: (options?: { state?: ApprovalState }) => Promise<ApprovalList>
  /** Allow or deny a waiting approval; the note is shown to the agent with the answer. */
  decideApproval?: (answer: { id: string; decision: "allow" | "deny"; note?: string }) => Promise<Approval>
  /** How the job an allowed approval started is going. */
  readApprovalJob?: (jobId: string) => Promise<ApprovalJob>
  /** Calls the handler whenever an approval changes. Returns the function that stops it. */
  watchApprovals?: (onChange: () => void) => () => void
  /** Who did what, newest first; `beforeId` continues from the previous page. */
  listAudit?: (options?: { computerId?: string; callerKind?: AuditCallerKind; beforeId?: number; limit?: number }) => Promise<AuditPage>
  /** What the device has left in memory and disk. */
  readDeviceResources?: (deviceId?: string) => Promise<DeviceResources>
  /** The agents Silo can install in a computer, each with its state; `unknown` while the computer is stopped. */
  listAgents?: (computer: string) => Promise<AgentInfo[]>
  /** Installs an agent in a running computer. `onProgress` hears each step; it resolves with the computer's agents. */
  installAgent?: (computer: string, agent: AgentId, onProgress?: (progress: AgentInstallProgress) => void) => Promise<AgentInfo[]>
  /** Opens the person's terminal on a running computer with the agent started in its workspace folder. */
  openAgent?: (computer: string, agent: AgentId) => void
  /** The SSH host name this device's desktop apps use to reach the computer. Adds it to the SSH configuration. */
  ensureSshName?: (computer: string) => Promise<SshName>
  /** A computer's network policy: its mode and hosts, what Silo adds, and whether a change applies now or at the next start. Computers of this device only. */
  readNetworkPolicy?: (computer: string) => Promise<NetworkPolicyState>
  /** Saves a policy. `expected` is the one the person was looking at; the engine saves nothing if it has changed since. */
  saveNetworkPolicy?: (computer: string, policy: NetworkPolicy, expected?: NetworkPolicy) => Promise<NetworkPolicyState>
  /** Whether the agent inside a computer can use Silo's tools, and when a change takes effect. Computers of this device only. */
  readComputerTools?: (computer: string) => Promise<ComputerTools>
  /** Turns Silo's tools on or off for a computer. */
  setComputerTools?: (computer: string, enabled: boolean) => Promise<ComputerTools>
  createCheckpoint?: (computer: string, name: string) => Promise<void>
  forkCheckpoint?: (computer: string, checkpointId: string | null, newName: string) => Promise<void>
  restoreCheckpoint?: (computer: string, checkpointId: string) => Promise<void>
  /** Give up an unfinished Restore of a computer on this device, keeping its current state. */
  abandonRestore?: (computer: string) => Promise<void>
  /** Delete one checkpoint of a computer on this device. */
  deleteCheckpoint?: (computer: string, checkpointId: string) => Promise<void>
  /** Checkpoint sizes and Delete availability for a computer on this device, by its ID. */
  readCheckpointUsage?: (computerId: string) => Promise<CheckpointUsage>
  /** The recorded steps of a computer (oldest first); `deviceId` for a computer on another device. */
  listComputerSteps?: (computerId: string, deviceId?: string) => Promise<ComputerStep[]>
  /** Every computer on a device with what it was forked from and its pending changes. */
  readComputerTree?: (deviceId?: string) => Promise<ComputerTreeNode[]>
  readWorkspaceStorage?: (computerId: string) => Promise<WorkspaceStorageState>
  reclaimWorkspaceStorage?: (computerId: string) => Promise<WorkspaceStorageState>
  refreshRepositories?: () => Promise<void>
  openDesktop?: (computer: string) => void | Promise<void>
  cancelLogExport?: () => Promise<void>
  queryLogs?: LogLoader
  exportLogs?: (requests: LogQuery[]) => Promise<boolean>
  setConnectionsEnabled?: (enabled: boolean) => Promise<void>
  setupDeviceKey?: (address: string) => Promise<void>
  authorizeDevice?: (address: string) => Promise<void>
  connectDevice?: (address: string, options?: { replaceAddress?: boolean }) => Promise<void>
  removeDevice?: (deviceId: string) => Promise<void>
  saveRemoteComputer?: (deviceId: string, configuration: SetupComputerConfiguration, expected?: SetupComputerConfiguration) => Promise<void>
  deleteRemoteComputer?: (deviceId: string, configuration: SetupComputerConfiguration) => Promise<void>
  sshConnection?: (computer: string, download: boolean, network?: boolean) => Promise<string | null>
  refreshSshAccess?: (options?: { background?: boolean }) => Promise<void>
  saveSshAccess?: (request: SshAccessRequest) => Promise<void>
  refreshNetwork?: (options?: { background?: boolean }) => Promise<void>
  saveNetworkPort?: (request: NetworkPortRequest) => Promise<void>
  removeNetworkPort?: (computer: string, port: number) => Promise<void>
  openNetworkPort?: (computer: string, port: number) => Promise<void>
  listComputerDirectory?: DirectoryLoader
  /** Upload to and download from a running computer. Absent where the native boundary is unavailable. */
  fileTransfers?: FileTransferActions
  saveSecret: (request: SecretConfigurationRequest) => Promise<void> | void
  removeSecret: (id: string) => Promise<void> | void
  retrySecret?: (id: string) => Promise<void> | void
  retryRuntimeChecks: () => void
  saveComputerConfiguration: (request: SetupComputerConfigurationRequest, baseline?: SetupComputerConfiguration[]) => Promise<void> | void
  dismissComputerConfigurationError: () => void
  retryComputerConfiguration: (computer: string) => void
  dismissRepositoryPush?: (computer: string, repositoryPath: string) => void
  /** Push exactly the confirmed `target`; the host aborts if the computer no longer matches it. */
  pushRepository: (computer: string, repositoryPath: string, target: RepositoryPushTarget) => void
  startComputer: (computer: string) => void
  stopComputer: (computer: string) => void
  restartComputer: (computer: string) => void
  /** Cancel a queued or cancellable running operation by its operation-queue id. */
  cancelOperation?: (id: number) => void
  dismissComputerError: (computer: string) => void
  openTerminal: (computer: string) => void
  openEditor: (computer: string, path?: string) => void
  cancelGitHubConnection?: () => void
  reopenGitHubAuthorization?: () => void
  manageGitHubRepositories?: () => void
  connectGitHub?: () => void
  disconnectGitHub?: () => void
  setGitHubAccessEnabled?: (enabled: boolean) => void
  saveGitHubConfiguration?: (configuration: ApplicationGitHubConfiguration) => void | Promise<void>
  retryGitHubConfiguration?: (computer?: string) => void
  retryGitHubRepositoryCatalog?: () => void
}
