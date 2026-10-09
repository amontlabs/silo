import { useLifecycleToasts } from "../model/use-lifecycle-toasts"
import { ForkBody } from "../components/fork-popover"
import { runCheckpointOperation, syncCheckpointProgress } from "../model/checkpoint-operation-toast"
import { useSshAccessRefresh } from "./use-ssh-access-refresh"
import { SshAccessBadges } from "./ssh-access-panel"
import { StatusFolderPicker } from "@/features/status-bar/status-folder-picker"
import { computerAvailability, type ComputerAvailability } from "../model/computer-availability"
import { DisabledReason } from "../components/disabled-reason"
import type { ComputerCommandRequest } from "../components/application-commands"
import { LifecycleControl } from "../components/lifecycle-control"
import { lifecycleGuard, type LifecycleAction, type LifecycleGuard } from "../model/lifecycle-guard"
import { DeviceBadge } from "@/features/computers/components/device-badge"
import { parseRemoteComputerTarget, computerTarget } from "../model/connections"
import { ConnectDeviceForm } from "../components/connections-settings"
import { ComputerDetailPage, type ComputerDetailControls, type ComputerDetailEditing } from "./computer-detail-page"
import type { ApplicationInitialRoute } from "@/features/application/model/use-application-navigation"
import { CircleAlert, Code, Download, GitFork, HardDrive, History, KeyRound, Loader2, Monitor, Play, RotateCw, Square, Terminal } from "lucide-react"
import { useEffect, useEffectEvent, useRef, useState, type ReactNode } from "react"
import { dismissOperationToast, dismissComputerToasts, dismissComputerToastsById, showActionFailure } from "@/lib/operation-toast"

import type { MenuAction, MenuPopovers } from "@/components/actions-menu"
import { ConfirmBody } from "@/components/confirm-popover"
import type { BackupController, VerifiedExport } from "../model/backup-source"
import { computerNamesOnDevice, type ComputerCheckpoint } from "../model/checkpoint-source"

import { configurationFailureDiagnostic } from "../model/configuration-failure"
import { ErrorDetails } from "@/components/error-details"
import { ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { setupComputerConfigurationSchema, type SetupComputerConfiguration, type SiloProgressEvent } from "@/contracts/silo"
import { ComputerStateLabel } from "@/features/application/components/application-ui"
import { ComputerStatus } from "@/features/application/components/computer-status"
import type {
  ApplicationActions,
  ApplicationSource,
  ApplicationComputer,
  ComputerConfigurationOperation,
  ComputerDetailTab,
} from "@/features/application/model/application-source"
import { ComputerConfigurationList } from "@/features/computers/components/computer-configuration-list"
import { MacosComputersSection } from "@/features/macos-computers/components/macos-computers-section"
import type { DeleteComputerDetails } from "@/features/computers/components/delete-computer-confirmation"
import { ComputerAction, type ComputerIconState } from "@/features/computers/components/computer-list"

import { SecretChangesLabel } from "@/features/computers/components/secret-changes-label"
import { computerBusyReason, computerIconState, computerRowTone } from "@/features/computers/model/computer-presentation"
import { deviceCapacityFrom } from "@/features/computers/model/computer-limits"
import { nextComputerOrder, computerOrderKey, computerOrderRanks } from "@/features/computers/model/computer-order"
import { useSettings } from "@/features/preferences/settings-store"

/** A command palette request carried out on a computer's page. */
export interface ComputerPageRequest {
  token: number
  computerId: string
  request: ComputerCommandRequest
}

const attentionPriority: Record<ComputerIconState, number> = {
  error: 0,
  warning: 1,
  normal: 2,
}

interface ConfigurationRowView {
  status: "running" | "failed"
  message: string
  diagnostic?: string
  completedSteps?: number
  recovery?: string
  retryable: boolean
}

const configurationSteps = new Set([
  "computer-configuration",
  "computer-networking",
  "computer-verification",
])

function emptyComputer(configuration: SetupComputerConfiguration): ApplicationComputer {
  return {
    configuration,
    purpose: "New computer",
    state: "stopped",
    stateDetail: "Not configured",
    freshness: "fresh",
    repositories: [],
    files: [],
    ports: [],
    logs: [],
    githubRepositories: [],
    secretNames: [],
  }
}

function displayComputers(source: ApplicationSource): ApplicationComputer[] {
  const operation = source.computerConfigurationOperation
  if (!operation) return source.computers
  const committedIDs = new Set(source.computers.map(({ configuration }) => configuration.id))
  const candidatesByID = new Map(operation.candidate.computers.map((configuration) => [configuration.id, configuration]))
  return [
    ...source.computers.map((computer) => ({
      ...computer,
      configuration: candidatesByID.get(computer.configuration.id) ?? computer.configuration,
    })),
    ...operation.candidate.computers
      .filter(({ id }) => !committedIDs.has(id))
      .map(emptyComputer),
  ]
}

function latestSafeEvent(operation: ComputerConfigurationOperation, computer: string): SiloProgressEvent | undefined {
  const activeRevision = operation.progressEvents.findLast(({ revision }) => revision)?.revision
  return operation.progressEvents.findLast((event) => (
    event.safeForDisplay
    && event.computer === computer
    && (!activeRevision || !event.revision || event.revision === activeRevision)
  ))
}

function configurationRowView(
  computer: ApplicationComputer,
  committedComputer: ApplicationComputer | undefined,
  operation: ComputerConfigurationOperation,
): ConfigurationRowView | undefined {
  const candidate = operation.candidate.computers.find(({ id }) => id === computer.configuration.id)
  const candidateName = candidate?.name ?? computer.configuration.name
  const removed = Boolean(committedComputer && !candidate)
  const addedOrChanged = !committedComputer || JSON.stringify(setupComputerConfigurationSchema.parse(committedComputer.configuration)) !== JSON.stringify(candidate && setupComputerConfigurationSchema.parse(candidate))
  const errorTargetsComputer = operation.status === "failed"
    && (operation.error.computer === candidateName || (!operation.error.computer && (removed || addedOrChanged)))

  if (errorTargetsComputer) {
    return {
      status: "failed",
      message: operation.error.message,
      diagnostic: configurationFailureDiagnostic(operation, candidateName),
      recovery: operation.error.recovery ?? undefined,
      retryable: operation.error.retryable,
    }
  }
  if (operation.status === "failed") return undefined
  if (removed) {
    return {
      status: "running",
      message: "Deleting the computer’s files and checkpoints.",
      retryable: false,
    }
  }

  const latest = latestSafeEvent(operation, candidateName)
  if (!latest && !addedOrChanged) return undefined
  if (!latest) {
    return {
      status: "running",
      message: "Preparing computer configuration.",
      completedSteps: 0,
      retryable: false,
    }
  }

  const completedSteps = new Set(operation.progressEvents
    .filter((event) => event.computer === candidateName && event.step && event.fraction === 1 && configurationSteps.has(event.step))
    .map(({ step }) => step)).size
  return {
    status: "running",
    message: latest.message,
    completedSteps,
    retryable: false,
  }
}

function ConfigurationIcon({ failed }: { failed: boolean }) {
  return (
    <ListRowIcon aria-hidden="true" className={failed
      ? "mt-1 self-start bg-destructive/10 text-destructive"
      : undefined}
    >
      {failed
        ? <CircleAlert className="size-3.5" aria-hidden="true" />
        : <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />}
    </ListRowIcon>
  )
}

function ConfigurationDetail({ view }: { view: ConfigurationRowView }) {
  const failed = view.status === "failed"
  const progressLabel = view.completedSteps === undefined ? undefined : `${view.completedSteps} of 3 steps complete`
  if (!failed) {
    return (
      <div role="status" aria-live="polite" aria-atomic="true" className="relative h-4 min-w-0">
        <div className="flex min-w-0 items-center gap-3">
          <span className="min-w-0 flex-1 truncate" title={view.message}>{view.message}</span>
          {progressLabel && <span className="shrink-0 text-[10px] text-muted-foreground">{progressLabel}</span>}
        </div>
        {view.completedSteps !== undefined && (
          <Progress aria-label={progressLabel} value={(view.completedSteps / 3) * 100} className="absolute inset-x-0 -bottom-1 h-0.5" />
        )}
      </div>
    )
  }
  return (
    <div
      role={failed ? "alert" : "status"}
      aria-live={failed ? "assertive" : "polite"}
      aria-atomic="true"
      className="grid gap-1.5 py-0.5"
    >
      <div className="flex min-w-0 items-start justify-between gap-3">
        <ErrorDetails className="flex-1 text-destructive" message={view.message} diagnostic={view.diagnostic} fallbackSummary="Computer changes failed." />
        {progressLabel && <span className="shrink-0 text-[10px] text-muted-foreground">{progressLabel}</span>}
      </div>
      {view.completedSteps !== undefined && (
        <Progress aria-label={progressLabel} value={(view.completedSteps / 3) * 100} className="mt-0.5" />
      )}
      {view.recovery && <p className="text-[10px] text-muted-foreground">{view.recovery}</p>}
    </div>
  )
}

/** The row's Start or Stop control, through the shared lifecycle guard. */
function ComputerActions({ computer, availability, readOnly, guard }: { computer: ApplicationComputer; availability: ComputerAvailability; readOnly: boolean; guard: LifecycleGuard }) {
  const { configuration } = computer
  const action = computer.state === "running" || computer.state === "starting" ? "stop" : "start"
  const enabled = !readOnly && (action === "stop" ? availability.canStop : availability.canStart)
  const asks = enabled && guard.check(computer, action).kind === "confirm"
  return <>
    {Boolean(computer.pendingSecretRevocations?.length) && <LifecycleControl guard={guard} computer={computer} action="restart" disabled={readOnly || !availability.canRestart} reason={readOnly ? undefined : availability.reasons.restart}>
      {({ onClick, disabled }) => <ComputerAction label={`Restart ${configuration.name}`} disabled={disabled} onClick={onClick}><RotateCw /></ComputerAction>}
    </LifecycleControl>}
    <LifecycleControl guard={guard} computer={computer} action={action} disabled={!enabled} reason={readOnly ? undefined : availability.reasons[action]}>
      {({ onClick, disabled }) => action === "stop"
        ? <ComputerAction label={`Stop ${configuration.name}`} tooltip={asks ? `Stop ${configuration.name}…` : undefined} disabled={disabled} onClick={onClick}><Square /></ComputerAction>
        : <ComputerAction label={`Start ${configuration.name}`} tooltip={asks ? `Start ${configuration.name}…` : undefined} disabled={disabled} onClick={onClick}><Play /></ComputerAction>}
    </LifecycleControl>
  </>
}

export function OverviewPage({ active = true, readOnly = false, notifyOperations = true,
  source,
  actions,
  backup,
  onConfigurationsChange,
  newComputerRequest,
  onNewComputerRequestHandled,
  computerRequest,
  onComputerRequestHandled,
  onExportComputer,
  onImportComputer,
  importPopover,
  selectedComputerId,
  computerTab,
  onOpenComputer,
  onCloseComputer,
  onSelectComputerTab,
  onNavigate,
}: {
  active?: boolean
  readOnly?: boolean
  /** Standalone pages own notifications; ApplicationApp owns them across navigation. */
  notifyOperations?: boolean
  newComputerRequest?: number
  onNewComputerRequestHandled?: (id: number) => void
  /** A palette command for a computer's page: its folder picker, Fork or Delete popover. */
  computerRequest?: ComputerPageRequest
  onComputerRequestHandled?: (token: number) => void
  /** Pick a folder and export a computer (or one of its checkpoints) as a background toast. */
  onExportComputer?: (computerName: string, checkpoint?: { id: string; name: string }) => void | Promise<VerifiedExport | null>
  /** Open the import review dialog after picking an export file. */
  onImportComputer?: () => void
  /** Wraps the computer list's Add button so the import review popover anchors to it. */
  importPopover?: (addButton: ReactNode) => ReactNode
  source: ApplicationSource
  actions: ApplicationActions
  backup?: BackupController
  onConfigurationsChange: (configurations: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[]) => Promise<void> | void
  /** The open computer detail page, its tab, and navigation callbacks. When omitted, the
   * page keeps its own selection state so it works standalone (fixtures and unit tests). */
  selectedComputerId?: string | null
  computerTab?: ComputerDetailTab
  onOpenComputer?: (computer: string, tab?: ComputerDetailTab) => void
  onCloseComputer?: () => void
  onSelectComputerTab?: (tab: ComputerDetailTab) => void
  /** Navigate to another section (Files/Network filtered to a computer, or the Secrets tab). */
  onNavigate?: (route: ApplicationInitialRoute) => void
}) {
  useLifecycleToasts(source, actions, { enabled: notifyOperations, readOnly })
  useSshAccessRefresh(readOnly ? undefined : actions.refreshSshAccess, active)
  // The editor folder picker replaces the page for the route it was opened from. It closes
  // for good when that route changes (palette, status panel, Back/Forward, another section)
  // or when its computer can no longer be opened, so it never takes the screen over later.
  const [folderPicker, setFolderPicker] = useState<{ computerId: string; route: string } | null>(null)
  const [connecting, setConnecting] = useState(false)
  // Computer detail selection: controlled by the app's navigation when the callbacks are
  // supplied, otherwise kept locally so the page still opens details on its own.
  const controlledNav = onOpenComputer !== undefined
  const [internalComputer, setInternalComputer] = useState<{ id: string; tab: ComputerDetailTab } | null>(null)
  const selectedId = controlledNav ? selectedComputerId ?? null : internalComputer?.id ?? null
  const activeComputerTab: ComputerDetailTab = controlledNav ? computerTab ?? "overview" : internalComputer?.tab ?? "overview"
  const openComputer = (id: string, tab: ComputerDetailTab = "overview") => { if (controlledNav) onOpenComputer!(id, tab); else setInternalComputer({ id, tab }) }
  const closeComputer = () => { if (controlledNav) onCloseComputer?.(); else setInternalComputer(null) }
  const selectComputerTab = (tab: ComputerDetailTab) => { if (controlledNav) onSelectComputerTab?.(tab); else setInternalComputer((current) => current ? { ...current, tab } : current) }
  // Duplicate from a detail page opens the list editor for the new computer, so it hands the
  // request to the list (which owns that flow) and returns to the list. Edit and Delete are
  // now handled in place on the detail page.
  const [configurationAction, setConfigurationAction] = useState<{ token: number; computerId: string; action: "duplicate" }>()
  const configurationActionToken = useRef(0)
  function requestDuplicate(computerId: string) {
    closeComputer()
    setConfigurationAction({ token: ++configurationActionToken.current, computerId, action: "duplicate" })
  }
  const backupOperation = backup?.state.operation
  const transferBusy = backupOperation?.kind === "running"
  const exportComputer = !readOnly && backup && onExportComputer ? onExportComputer : undefined
  const importComputer = !readOnly && backup && onImportComputer ? onImportComputer : undefined
  const visibleComputers = displayComputers(source)
  // Resolved lazily when a "Fork created" toast's Open button is clicked, so it finds the
  // newly forked computer once the backend snapshot includes it rather than at toast time.
  const computersRef = useRef(visibleComputers)
  useEffect(() => { computersRef.current = visibleComputers })
  const computers = new Map(visibleComputers.map((computer) => [computer.configuration.id, computer]))
  const committedComputers = new Map(source.computers.map((computer) => [computer.configuration.id, computer]))
  const configurations = visibleComputers.map(({ configuration }) => configuration)
  /** Whether "Fork…" is currently unavailable for a computer. */
  function forkDisabled(computer: ApplicationComputer) {
    return configurationLocked || computerAvailability(computer, source).busy || computer.freshness === "stale"
  }
  /** The Fork popover body for a computer's ⋯ menu (row or detail page); state lives with that menu. */
  function forkPopovers(computer: ApplicationComputer | undefined): MenuPopovers | undefined {
    if (!computer || !actions.forkCheckpoint) return undefined
    return { fork: close => <ForkBody computerName={computer.configuration.name} disabled={forkDisabled(computer)} takenNames={computerNamesOnDevice(visibleComputers, computer.device?.id)} onFork={name => forkCurrentState(computer, name)} onClose={close} /> }
  }
  const configurationOperation = source.computerConfigurationOperation
  const configurationLocked = readOnly || configurationOperation !== null
  const getComputerDeviceId = (configuration: SetupComputerConfiguration) => parseRemoteComputerTarget(configuration.id)?.deviceId ?? computers.get(configuration.id)?.device?.id
  const localOnly = (list: readonly SetupComputerConfiguration[]) => list.filter(configuration => !getComputerDeviceId(configuration))
  const localConfigurations = localOnly(configurations)

  // Return to the list if the open computer disappeared (deleted, or removed by a refresh).
  // Controlled navigation replaces its history entries in place (the app forgets missing
  // computers), so only the standalone page closes its own selection here: pushing a new
  // entry would leave Back pointing at the missing computer.
  const detailComputer = selectedId ? computers.get(selectedId) : undefined
  const detailMissing = Boolean(selectedId) && !detailComputer
  const returnToList = useEffectEvent(() => { if (!controlledNav) closeComputer() })
  // oxlint-disable-next-line react/set-state-in-effect
  useEffect(() => { if (detailMissing) returnToList() }, [detailMissing])

  // Build the save from the baseline the editor started from (falling back to the live
  // local list) so the change carries the right `expected` state and does not drag other
  // computers' concurrent edits into this one.
  function updateLocal(configuration: SetupComputerConfiguration, original?: SetupComputerConfiguration, baseline?: SetupComputerConfiguration[]) {
    const base = baseline ? localOnly(baseline) : localConfigurations
    const next = original ? base.map(item => item.id === original.id ? configuration : item) : [...base, configuration]
    return onConfigurationsChange(next, baseline ? base : undefined)
  }

  // The computer editing callbacks, shared by the list and the detail page's in-place editor
  // and delete dialog so both commit, delete, and validate through exactly the same paths.
  // This device's own order of the list, local and remote computers alike.
  const { settings: { computerOrder }, updateSettings } = useSettings()
  const orderRanks = computerOrderRanks(computerOrder)
  const orderRank = (configuration: SetupComputerConfiguration) => {
    const computer = computers.get(configuration.id)
    return computer ? orderRanks.get(computerOrderKey(computer)) : undefined
  }
  const reorderComputers = (ids: string[]) => {
    const shown = ids.flatMap(id => { const computer = computers.get(id); return computer ? [computerOrderKey(computer)] : [] })
    void updateSettings({ computerOrder: nextComputerOrder(computerOrder, shown) })
  }
  const commitComputer = actions.saveRemoteComputer ? async (configuration: SetupComputerConfiguration, original: SetupComputerConfiguration | undefined, deviceId: string, baseline?: SetupComputerConfiguration[]) => {
    if (deviceId) await actions.saveRemoteComputer!(deviceId, configuration, original)
    else await updateLocal(configuration, original, baseline)
  } : undefined
  const deleteComputer = actions.deleteRemoteComputer ? async (configuration: SetupComputerConfiguration, baseline?: SetupComputerConfiguration[]) => {
    const device = computers.get(configuration.id)?.device
    if (device) {
      if (!device.connected) throw new Error(`${device.name} is offline. Reconnect to it before deleting ${configuration.name}.`)
      await actions.deleteRemoteComputer!(device.id, configuration)
    } else {
      const base = baseline ? localOnly(baseline) : localConfigurations
      await onConfigurationsChange(base.filter(item => item.id !== configuration.id), baseline ? base : undefined)
    }
  } : undefined
  const changeConfigurations = (next: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[]) => {
    if (source.computerOperationsUnavailable) { notifyOperationUnavailable(); return }
    return onConfigurationsChange(localOnly(next), baseline ? localOnly(baseline) : undefined)
  }
  const validateComputerOperation = (configuration: SetupComputerConfiguration, isNew: boolean, deviceId?: string) => {
    const device = computers.get(configuration.id)?.device ?? source.devices?.find(device => device.id === deviceId)
    if (deviceId && !device) return "The selected device was removed. Choose another device before saving."
    if (device) return device.busy ? `${device.name} is updating. Wait before changing ${configuration.name}.` : device.connected ? undefined : `${device.name} is offline. Reconnect to it before changing ${configuration.name}.`
    if (source.computerOperationsUnavailable) return source.computerOperationsUnavailable
    const notice = source.resourceNotice
    if (!isNew || notice?.kind !== "create-storage" || configuration.name !== notice.computer) return undefined
    return `Not enough storage to create ${configuration.name}. About ${notice.requiredGB} GiB is needed on ${notice.volume}; ${notice.availableGB} GiB is available. No computer was created.`
  }
  const isComputerCreated = (configuration: SetupComputerConfiguration) => committedComputers.has(configuration.id)
  const isComputerRunning = (configuration: SetupComputerConfiguration) => computers.get(configuration.id)?.state === "running"
  const latestDeleteState = useRef({ source, readOnly })
  useEffect(() => { latestDeleteState.current = { source, readOnly } }, [source, readOnly])
  const configurationBusyReason = (configuration: SetupComputerConfiguration) => computerBusyReason(computers.get(configuration.id))

  function notifyOperationUnavailable() {
    showActionFailure("Computer operation unavailable", source.computerOperationsUnavailable ?? "Computer operations are unavailable.", undefined, { native: false })
  }

  // Every lifecycle request from this page (row, computer page, menus, toasts) goes through the
  // shared guard: unavailable Computer operations are reported and memory pressure asks first.
  const guard = lifecycleGuard(source, actions)
  // Notification actions run later: resolve the computer and its guard when clicked, so a toast
  // shown before the snapshot refreshed never acts on outdated state.
  const latestLifecycle = useRef({ guard, computers })
  useEffect(() => { latestLifecycle.current = { guard, computers } })
  function lifecycleLater(computer: ApplicationComputer, action: LifecycleAction, confirmed: boolean) {
    return () => {
      const { guard: current, computers: now } = latestLifecycle.current
      const fresh = now.get(computer.configuration.id) ?? computer
      if (confirmed) current.confirm(fresh, action)
      else current.request(fresh, action)
    }
  }

  /** Work runs on the computer or its device is unreachable, so its settings can't change now. */
  function changesBlocked(computer: ApplicationComputer) {
    return computerAvailability(computer, source).busy || Boolean(computer.device && !computer.device.connected)
  }

  /** What a computer's Delete dialog states and offers, the same from its row and its page. */
  function deleteDetails(computer: ApplicationComputer): DeleteComputerDetails {
    const { configuration } = computer
    const readStorage = actions.readWorkspaceStorage
    return {
      checkpoints: computer.checkpoints?.length,
      readSize: !computer.device && readStorage
        ? async () => {
            const storage = await readStorage(configuration.id)
            return storage.workspaceHostBytes === null || storage.runtimeHostBytes === null
              ? null : storage.workspaceHostBytes + storage.runtimeHostBytes
          }
        : undefined,
      exportFirst: !computer.device && exportComputer
        ? async () => {
            try {
              if (!await exportComputer(configuration.name)) return false
            } catch (error) {
              showActionFailure(`Could not export ${configuration.name}`, error, undefined, { native: false })
              return false
            }
            // Export can take minutes. A verified file does not authorize deleting a computer
            // that started, disappeared, or became busy while that file was being written.
            const current = latestDeleteState.current
            const fresh = current.source.computers.find(item => item.configuration.id === configuration.id && !item.device)
            if (!fresh || current.readOnly || current.source.computerOperationsUnavailable || current.source.computerConfigurationOperation || computerAvailability(fresh, current.source).busy || fresh.state === "running" || fresh.freshness === "stale") {
              showActionFailure(`Could not delete ${configuration.name}`, "The computer changed while exporting. Review its current state before deleting it. Your export is saved.", undefined, { native: false })
              return false
            }
            return true
          }
        : undefined,
    }
  }

  /** The Open Linux desktop icon of a computer's list row and page, absent without a desktop. */
  function desktopAction(computer: ApplicationComputer): { disabled: boolean; onClick: () => void } | undefined {
    const { configuration } = computer
    if (!configuration.desktop || !actions.openDesktop) return undefined
    const target = computerTarget(computer)
    const availability = computerAvailability(computer, source)
    return { disabled: configurationLocked || availability.busy || Boolean(computer.device && computer.freshness === "stale"), onClick: () => { void actions.openDesktop!(target) } }
  }

  /** A computer's ⋯ menu actions and popovers, built once for its list row and its page. The
   * page and the list append their own Edit, Duplicate, Add Linux desktop and Delete items. */
  function computerMenu(computer: ApplicationComputer): { items: MenuAction[]; popovers?: MenuPopovers } {
    const { configuration } = computer
    const availability = computerAvailability(computer, source)
    const stale = computer.freshness === "stale"
    const local = !computer.device
    const restartCheck = guard.check(computer, "restart")
    const restartPrompt = restartCheck.kind === "confirm" && availability.canRestart && !readOnly ? restartCheck.prompt : undefined
    const items: MenuAction[] = [
      restartPrompt
        ? { label: "Restart…", icon: RotateCw, accessibleLabel: `Restart ${configuration.name}`, popover: "restart" }
        : { label: "Restart", icon: RotateCw, accessibleLabel: `Restart ${configuration.name}`, disabled: readOnly || !availability.canRestart, tooltip: readOnly || availability.canRestart ? undefined : availability.reasons.restart, onSelect: () => guard.request(computer, "restart") },
      // Checkpoints, Storage and SSH open the page's tabs, which disable their own actions as needed.
      { label: "Checkpoints", icon: History, accessibleLabel: `Checkpoints for ${configuration.name}`, onSelect: () => openComputer(configuration.id, "checkpoints") },
      ...(actions.forkCheckpoint ? [{ label: "Fork…", icon: GitFork, accessibleLabel: `Fork ${configuration.name}`, disabled: forkDisabled(computer), popover: "fork" }] : []),
      ...(local && actions.readWorkspaceStorage ? [{ label: "Storage", icon: HardDrive, accessibleLabel: `Storage for ${configuration.name}`, onSelect: () => openComputer(configuration.id, "storage") }] : []),
      // Shown whenever the page shows its SSH tab.
      ...((source.sshAccess || actions.refreshSshAccess) ? [{ label: "SSH", icon: KeyRound, accessibleLabel: `SSH for ${configuration.name}`, onSelect: () => openComputer(configuration.id, "access") }] : []),
      ...(local && exportComputer ? [{ label: "Export…", icon: Download, accessibleLabel: `Export ${configuration.name}`, disabled: configurationLocked || availability.busy || transferBusy || stale, onSelect: () => exportComputer(configuration.name) }] : []),
    ]
    const popovers: MenuPopovers = { ...forkPopovers(computer) }
    if (restartPrompt) popovers.restart = close => <ConfirmBody tone={restartPrompt.tone} title={restartPrompt.title} description={restartPrompt.description} confirmLabel={restartPrompt.confirmLabel} onClose={close} onConfirm={lifecycleLater(computer, "restart", true)} />
    return { items, popovers }
  }

  // Checkpoint operations report through one progress notification each (see
  // model/checkpoint-operation-toast). Both the list and the detail page render from here, so
  // the backend stage refinement lives in this single effect. A finished fork is easy to miss
  // (the new computer is stopped), so Open jumps to it, resolved fresh at click time.
  useEffect(() => {
    syncCheckpointProgress(source.computers, { queue: source.operationQueue, cancel: readOnly ? undefined : actions.cancelOperation })
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [source.computers, source.operationQueue])

  // A deleted computer takes its notifications with it: their actions (Open, Retry…) would
  // otherwise point at something that no longer exists.
  const knownComputers = useRef(new Map<string, { id: string; name: string; deviceId: string; target: string }>())
  useEffect(() => {
    const current = new Map(source.computers.map(computer => [`${computer.device?.id ?? ""}:${computer.configuration.id}`, { id: computer.configuration.id, name: computer.configuration.name, deviceId: computer.device?.id ?? "", target: computerTarget(computer) }]))
    for (const [key, known] of knownComputers.current) {
      if (current.has(key)) continue
      dismissComputerToastsById(known.id)
      if (known.deviceId) dismissComputerToasts(known.target)
      dismissOperationToast(`lifecycle:${key}`)
      // A name shared with a computer that still exists (e.g. on another device) keeps its notifications.
      if ([...current.values()].some(other => other.name === known.name)) continue
      dismissComputerToasts(known.name)
    }
    knownComputers.current = current
  }, [source.computers])

  /** Open the fork `name`, created on the same device as its source computer. */
  function forkOpenAction(name: string, deviceId = "") {
    return {
      label: "Open",
      onClick: () => {
        const match = computersRef.current.find(({ configuration, device }) => (device?.id ?? "") === deviceId && configuration.name === name)
        if (match) openComputer(match.configuration.id)
      },
    }
  }

  function forkCurrentState(computer: ApplicationComputer, name: string) {
    const target = computerTarget(computer)
    void runCheckpointOperation({
      id: `checkpoint:${target}:fork`,
      kind: "fork",
      target,
      computer: [computer.configuration.name, name],
      noticeComputer: { id: computer.configuration.id, name: computer.configuration.name },
      title: `Creating fork ${name}`,
      run: () => actions.forkCheckpoint!(target, null, name),
      success: { title: "Fork created", description: `${name} is stopped. Start it when you’re ready.`, action: forkOpenAction(name, computer.device?.id) },
      failureTitle: `Could not create fork ${name}`,
    })
  }

  const pickerRoute = `${active}:${selectedId ?? ""}:${activeComputerTab}`
  const openFolderPicker = (computerId: string) => setFolderPicker({ computerId, route: pickerRoute })
  // Palette requests open the computer's page (the app navigates there first) and then the
  // same folder picker or ⋯ popover its own controls open.
  const [menuRequest, setMenuRequest] = useState<{ token: number; computerId: string; panel: string }>()
  const handledComputerRequest = useRef(0)
  const runComputerRequest = useEffectEvent((request: ComputerPageRequest) => {
    openComputer(request.computerId)
    if (request.request === "editor") setFolderPicker({ computerId: request.computerId, route: `${active}:${request.computerId}:${activeComputerTab}` })
    else setMenuRequest({ token: request.token, computerId: request.computerId, panel: request.request })
    onComputerRequestHandled?.(request.token)
  })
  useEffect(() => {
    if (!computerRequest || handledComputerRequest.current === computerRequest.token) return
    handledComputerRequest.current = computerRequest.token
    runComputerRequest(computerRequest)
  }, [computerRequest])
  // The request belongs to the page it was made for: leaving that page drops it, so the
  // popover never reopens when the page is shown again later.
  if (menuRequest && selectedId !== null && selectedId !== menuRequest.computerId) setMenuRequest(undefined)
  if (menuRequest && selectedId === null && !computerRequest) setMenuRequest(undefined)
  const folderComputer = folderPicker && folderPicker.route === pickerRoute ? computers.get(folderPicker.computerId) : undefined
  const showFolderPicker = Boolean(folderComputer && computerAvailability(folderComputer, source).canOpen)
  // Adjusting state while rendering: the picker is dropped before it could reappear.
  if (folderPicker && !showFolderPicker) setFolderPicker(null)
  if (folderComputer && showFolderPicker) {
    return <div className="mx-auto flex h-full min-h-0 w-full max-w-4xl flex-col px-4 py-5 sm:px-6 sm:py-6">
      <StatusFolderPicker key={folderComputer.configuration.id} computer={folderComputer} editor={source.preferences.editor} listDirectory={actions.listComputerDirectory} onBack={() => setFolderPicker(null)} onOpen={(path) => actions.openEditor(computerTarget(folderComputer), path)} />
    </div>
  }

  function detailControls(computer: ApplicationComputer): ComputerDetailControls {
    const configuration = computer.configuration
    const target = computerTarget(computer)
    const availability = computerAvailability(computer, source)
    const menu = computerMenu(computer)
    // Edit and Delete are appended and handled in place by the detail page; Duplicate hands
    // off to the list editor for the new computer. Editing is offered only when not read-only.
    const editing: ComputerDetailEditing | undefined = readOnly ? undefined : {
      configurations,
      devices: source.devices,
      getDeviceId: getComputerDeviceId,
      onCommitComputer: commitComputer,
      onDeleteComputer: deleteComputer,
      onConfigurationsChange: changeConfigurations,
      validateOperation: validateComputerOperation,
      isComputerCreated,
      isComputerRunning,
    }
    return {
      pageActive: active,
      editing,
      onDuplicate: readOnly ? undefined : () => requestDuplicate(configuration.id),
      onNavigate,
      onBack: closeComputer,
      activeTab: activeComputerTab,
      onSelectTab: selectComputerTab,
      readOnly,
      configurationLocked,
      computerOperationBusy: availability.busy,
      changesBlocked: changesBlocked(computer),
      menuRequest: menuRequest?.computerId === computer.configuration.id ? menuRequest : undefined,
      deleteDetails: deleteDetails(computer),
      canOpen: availability.canOpen && !readOnly,
      canStart: availability.canStart && !readOnly,
      canStop: availability.canStop && !readOnly,
      disabledReasons: readOnly ? {} : availability.reasons,
      menuActions: menu.items,
      popovers: menu.popovers,
      onTerminal: () => actions.openTerminal(target),
      onEditor: () => openFolderPicker(configuration.id),
      desktop: desktopAction(computer),
      lifecycleGuard: guard,
      onCheckpointExport: exportComputer ? (checkpoint: ComputerCheckpoint) => exportComputer(configuration.name, { id: checkpoint.id, name: checkpoint.name }) : undefined,
      checkpointExportDisabled: transferBusy || backup?.state.availability === "unavailable",
      onCheckpointForkedAction: (name: string) => forkOpenAction(name, computer.device?.id),
      onCheckpointRestoredAction: () => ({ label: "Start", onClick: lifecycleLater(computer, "start", false) }),
    }
  }

  return (
    <div className="mx-auto flex h-full min-h-0 w-full max-w-4xl flex-col px-4 py-5 sm:px-6 sm:py-6">
      <div className="min-h-0 flex-1">
        {detailComputer ? (
          // Keyed per computer so edit drafts, delete confirmations and panel state never carry over.
          <ComputerDetailPage key={computerTarget(detailComputer)} computer={detailComputer} source={source} actions={actions} controls={detailControls(detailComputer)} />
        ) : (
          <>
            {connecting && actions.connectDevice && <div className="mb-3"><ConnectDeviceForm connect={actions.connectDevice} authorize={actions.authorizeDevice} setupKey={actions.setupDeviceKey} onClose={() => setConnecting(false)} /></div>}
            {configurationOperation?.status === "failed" && <div className="mb-3 rounded-md border border-destructive/30 p-3">
              <div role="alert" className="text-sm text-destructive"><ErrorDetails message={configurationOperation.error.message} diagnostic={configurationFailureDiagnostic(configurationOperation)} fallbackSummary="Computer changes failed." /></div>
              <Button variant="outline" size="sm" className="mt-2" disabled={readOnly} onClick={() => actions.dismissComputerConfigurationError()}>Dismiss configuration error</Button>
            </div>}
            <div className="flex h-full min-h-0 flex-col gap-4">
            <div className="min-h-0 flex-1">
            <ComputerConfigurationList
              newComputerRequest={readOnly ? undefined : newComputerRequest}
              onNewComputerRequestHandled={onNewComputerRequestHandled}
              onOpenComputer={(configuration) => openComputer(configuration.id)}
              configurationActionRequest={configurationAction}
              onConfigurationActionHandled={(token) => setConfigurationAction((current) => current?.token === token ? undefined : current)}
              configurations={configurations}
              devices={source.devices}
              getDeviceId={getComputerDeviceId}
              onConnectDevice={actions.connectDevice ? () => setConnecting(true) : undefined}
              onImportComputer={importComputer}
              importPopover={importComputer ? importPopover : undefined}
              onCommitComputer={commitComputer}
              onDeleteComputer={deleteComputer}
              isComputerCreated={isComputerCreated}
              isComputerRunning={isComputerRunning}
              getConfigurationBusyReason={configurationBusyReason}
              editorDraftKey="computer-list"
              // New computers fit this device; remote devices do not report capacity yet.
              getDeviceCapacity={(deviceId) => deviceId ? undefined : deviceCapacityFrom(source.deviceCapacity)}
              onConfigurationsChange={changeConfigurations}
              orderRank={orderRank}
              onReorder={reorderComputers}
              interactionDisabled={configurationLocked}
              validateOperation={validateComputerOperation}
              summary={configurationOperation ? <>{source.computers.length} configured · {configurationOperation.status === "failed" ? "Computer changes failed" : "Applying computer changes"}</> : undefined}
              sortPriority={(configuration) => {
                const computer = computers.get(configuration.id)
                const rowView = computer && !computer.device && configurationOperation
                  ? configurationRowView(computer, committedComputers.get(configuration.id), configurationOperation)
                  : undefined
                return attentionPriority[rowView?.status === "failed" ? "error" : computerIconState(computer)]
              }}
              getRowPresentation={(configuration) => {
                const computer = computers.get(configuration.id)
                const state = computer?.state ?? "stopped"
                const pendingSecrets = !computer?.device
                  ? source.secrets.filter((secret) => secret.state === "restart-required" && secret.computers.includes(configuration.name)).map((secret) => secret.name)
                  : []
                const badge = pendingSecrets.length > 0
                  ? <SecretChangesLabel computer={configuration.name} state={state} secrets={pendingSecrets} />
                  : undefined
                const visualState = computerIconState(computer)
                const rowView = computer && !computer.device && configurationOperation
                  ? configurationRowView(computer, committedComputers.get(configuration.id), configurationOperation)
                  : undefined
                if (rowView) {
                  const failed = rowView.status === "failed"
                  return {
                    badge,
                    busy: !failed,
                    suppressInteractions: true,
                    openable: committedComputers.has(configuration.id),
                    icon: <ConfigurationIcon failed={failed} />,
                    iconState: failed ? "error" as const : "normal" as const,
                    tone: failed ? "error" as const : "starting" as const,
                    detailClassName: failed ? "overflow-visible whitespace-normal text-xs" : "overflow-visible",
                    detail: <ConfigurationDetail view={rowView} />,
                    actions: failed && rowView.retryable
                      ? <ComputerAction label={`Retry ${configuration.name} configuration`} disabled={readOnly} onClick={() => actions.retryComputerConfiguration(configuration.name)}><RotateCw /></ComputerAction>
                      : undefined,
                    actionsClassName: failed ? "mt-1 self-start" : undefined,
                  }
                }
                const access = computer && source.sshAccess?.computers.find(row => row.computer === computerTarget(computer))
                const sshStale = Boolean((source.sshAccessError && !computer?.device) || computer?.device?.connected === false || computer?.freshness === "stale")
                const lifecycle = computer?.lifecycleAction
                const checkpointOperation = computer?.checkpointOperation?.status === "running" ? computer.checkpointOperation : undefined
                const computerOperationBusy = Boolean(lifecycle) || Boolean(checkpointOperation)
                const menu = computer ? computerMenu(computer) : { items: [] }
                const availability = computer ? computerAvailability(computer, source) : undefined
                const openReason = readOnly || availability?.canOpen ? undefined : availability?.reasons.open
                return {
                  kindBadge: computer?.device ? <DeviceBadge device={computer.device} /> : undefined,
                  badge: <>{badge}<SshAccessBadges access={access} stale={sshStale} onOpen={() => openComputer(configuration.id, "access")} /></>,
                  popovers: menu.popovers,
                  menuActions: menu.items,
                  deleteDetails: computer ? deleteDetails(computer) : undefined,
                  busy: computerOperationBusy || Boolean(computer?.device?.busy),
                  suppressInteractions: computer ? changesBlocked(computer) : false,
                  icon: computerOperationBusy ? <ListRowIcon aria-hidden="true"><Loader2 className="size-3.5 animate-spin" /></ListRowIcon> : undefined,
                  iconState: computer?.lifecycleFailure && !computer.lifecycleFailureCancelled ? "error" as const : visualState,
                  tone: computerOperationBusy ? "starting" as const : computer?.lifecycleFailure && !computer.lifecycleFailureCancelled ? "error" as const : computerRowTone(computer),
                  detail: checkpointOperation ? <div role="status" aria-live="polite" aria-atomic="true" className="grid gap-1.5 py-0.5">
                    <p className="truncate text-xs" title={checkpointOperation.stage}>{checkpointOperation.stage}</p>
                    <Progress value={null} aria-label="Checkpoint operation progress" />
                  </div> : (
                    <span className="inline-flex max-w-full items-baseline gap-1 align-baseline">
                      <span className="truncate" title={computer?.attention?.message}>
                        {computer ? <ComputerStatus computer={computer} source={source} readOnly={readOnly} onCancel={actions.cancelOperation} /> : <ComputerStateLabel state={state} />}
                        {computer?.attention && <> · {computer.attention.message}</>}
                      </span>
                      {computer?.canDismissError && state === "failed" && <Button size="xs" variant="ghost" className="h-4 rounded px-1 text-[10px] font-normal" aria-label={`Dismiss ${configuration.name} error`} disabled={configurationLocked || computerOperationBusy || computer.freshness === "stale"} onClick={() => actions.dismissComputerError(computerTarget(computer))}>Dismiss</Button>}
                    </span>
                  ),
                  actions: <>
                    <DisabledReason reason={openReason}><ComputerAction label={`Open ${configuration.name} in ${source.preferences.terminal}`} disabled={readOnly || !availability?.canOpen} onClick={() => computer && actions.openTerminal(computerTarget(computer))}><Terminal /></ComputerAction></DisabledReason>
                    <DisabledReason reason={openReason}><ComputerAction label={`Open ${configuration.name} in ${source.preferences.editor}`} disabled={readOnly || !availability?.canOpen} onClick={() => openFolderPicker(configuration.id)}><Code /></ComputerAction></DisabledReason>
                    {computer && desktopAction(computer) && <ComputerAction label={`Open ${configuration.name} desktop`} disabled={desktopAction(computer)!.disabled} onClick={desktopAction(computer)!.onClick}><Monitor /></ComputerAction>}
                    {computer && availability && <ComputerActions computer={computer} availability={availability} readOnly={readOnly} guard={guard} />}
                  </>,
                }
              }}
            />
            </div>
            <MacosComputersSection capacity={deviceCapacityFrom(source.deviceCapacity)} />
            </div>
          </>
        )}
      </div>
    </div>
  )
}
