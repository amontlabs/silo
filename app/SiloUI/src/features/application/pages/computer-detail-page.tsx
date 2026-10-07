import { ChevronRight, Code, Cpu, GitBranch, Globe, KeyRound, Monitor, Play, Plus, RotateCw, Square, Terminal, TriangleAlert } from "lucide-react"
import { useId, type MouseEvent, type ReactNode } from "react"

import { ActionsMenu, type MenuAction, type MenuPopovers } from "@/components/actions-menu"
import { ListHeader, listHeadingClassName } from "@/components/list-header"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import { ComputerEditor } from "@/features/computers/components/computer-editor"
import { useComputerEditing } from "@/features/computers/model/use-computer-editing"
import { computerBusyReason } from "@/features/computers/model/computer-presentation"
import { deviceCapacityFrom } from "@/features/computers/model/computer-limits"
import type { ApplicationInitialRoute } from "@/features/application/model/use-application-navigation"
import { DeleteComputerBody, type DeleteComputerDetails } from "@/features/computers/components/delete-computer-confirmation"
import { computerEditMenu } from "@/features/computers/model/computer-edit-menu"
import { CheckpointPanel } from "@/features/application/components/checkpoint-panel"
import { StatusSeparator, ComputerStatus } from "@/features/application/components/computer-status"
import { DisabledReason } from "@/features/application/components/disabled-reason"
import { LifecycleControl } from "@/features/application/components/lifecycle-control"
import type { LifecycleGuard } from "@/features/application/model/lifecycle-guard"
import type { NetworkPort, ApplicationActions, ApplicationSource, ApplicationComputer, ComputerDetailTab, SshAccessComputer } from "@/features/application/model/application-source"
import { computerNamesOnDevice, type ComputerCheckpoint } from "@/features/application/model/checkpoint-source"
import { computerTarget } from "@/features/application/model/connections"
import { computerAvailability } from "@/features/application/model/computer-availability"
import { SshAccessBadges, SshAccessRow } from "@/features/application/pages/ssh-access-panel"
import { WorkspaceStoragePanel } from "@/features/application/pages/workspace-storage-panel"
import { SecretChangesLabel } from "@/features/computers/components/secret-changes-label"
import { AddSecretEditor, SecretRow } from "@/features/application/components/secrets-management"
import { useSecretsManager } from "@/features/application/components/secrets-manager"
import { NetworkPortForm, NetworkPortRowActions } from "@/features/application/components/network-ports"
import { networkAddress, networkPortState, useNetworkPorts } from "@/features/application/components/network-ports-state"
import { ComputerUseSection } from "@/desktop/computer-use-panel"
import { PortStateLabel } from "@/features/application/components/application-ui"
import { cn } from "@/lib/utils"

/** Everything the detail page needs to edit or delete this computer in place, sharing the
 * list's `useComputerEditing` behaviour (validation, stale-baseline conflict review, saving). */
export interface ComputerDetailEditing {
  configurations: readonly SetupComputerConfiguration[]
  devices?: readonly { id: string; name: string; connected: boolean }[]
  getDeviceId?: (configuration: SetupComputerConfiguration) => string | undefined
  onCommitComputer?: (configuration: SetupComputerConfiguration, original: SetupComputerConfiguration | undefined, deviceId: string, baseline?: SetupComputerConfiguration[]) => Promise<void>
  onDeleteComputer?: (configuration: SetupComputerConfiguration, baseline?: SetupComputerConfiguration[]) => Promise<void>
  onConfigurationsChange: (configurations: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[]) => Promise<void> | void
  validateOperation?: (configuration: SetupComputerConfiguration, isNew: boolean, deviceId?: string) => string | undefined
  isComputerCreated?: (configuration: SetupComputerConfiguration) => boolean
  isComputerRunning?: (configuration: SetupComputerConfiguration) => boolean
}

export interface ComputerDetailControls {
  /** Whether the page holding this computer is visible; background refresh stops while it is not. Default true. */
  pageActive?: boolean
  onBack: () => void
  activeTab: ComputerDetailTab
  onSelectTab: (tab: ComputerDetailTab) => void
  readOnly: boolean
  configurationLocked: boolean
  computerOperationBusy: boolean
  /** Edit, Duplicate, Add Linux desktop and Delete wait while work runs or the device is offline. */
  changesBlocked?: boolean
  canOpen: boolean
  canStart: boolean
  canStop: boolean
  /** Why Open, Start or Stop is unavailable, shown on the disabled control. */
  disabledReasons?: { open?: string; start?: string; stop?: string }
  menuActions: MenuAction[]
  /** Popovers opened by `menuActions` entries with a matching `popover` key (e.g. Fork), anchored to the ⋯ button. */
  popovers?: MenuPopovers
  onTerminal: () => void
  onEditor: () => void
  /** Opens the Linux desktop; absent when the computer has none. */
  desktop?: { disabled: boolean; onClick: () => void }
  /** Start and Stop go through the shared lifecycle guard; its prompts open next to the button. */
  lifecycleGuard: LifecycleGuard
  /** In-place Edit/Delete of this computer. Absent in read-only or standalone renders. */
  editing?: ComputerDetailEditing
  /** Opens a ⋯ popover (Fork, Delete) without the menu, for a command palette request. */
  menuRequest?: { token: number; panel: string }
  /** What the Delete dialog states (checkpoints, size) and offers, as from the list row. */
  deleteDetails?: DeleteComputerDetails
  /** Duplicate opens the list editor for the new computer (it leaves the detail page). */
  onDuplicate?: () => void
  /** Jump to another section (Files/Network filtered to this computer, or the Secrets tab). */
  onNavigate?: (route: ApplicationInitialRoute) => void
  // Export a checkpoint's disks; progress is shown as a background toast.
  onCheckpointExport?: (checkpoint: ComputerCheckpoint) => void
  checkpointExportDisabled: boolean
  // Toast a created fork (with Open) and a restored checkpoint (with Start).
  onCheckpointForkedAction?: (name: string) => { label: string; onClick: () => void }
  onCheckpointRestoredAction?: (checkpoint: ComputerCheckpoint) => { label: string; onClick: () => void }
}

const Sep = StatusSeparator

function DetailSubtitle({ computer, source, readOnly, pendingSecrets, sshAccess, sshStale, onCancel, onOpenSsh }: {
  computer: ApplicationComputer
  source: ApplicationSource
  readOnly: boolean
  pendingSecrets: string[]
  sshAccess?: SshAccessComputer
  sshStale: boolean
  onCancel?: ApplicationActions["cancelOperation"]
  onOpenSsh?: () => void
}) {
  const { configuration } = computer
  const location = computer.device ? computer.device.name : "This device"
  return <span>
    <ComputerStatus computer={computer} source={source} readOnly={readOnly} onCancel={onCancel} />
    <Sep />{location}
    {pendingSecrets.length > 0 && <><Sep /><SecretChangesLabel inline computer={configuration.name} state={computer.state} secrets={pendingSecrets} /></>}
    {sshAccess?.enabled && <><Sep /><SshAccessBadges access={sshAccess} stale={sshStale} onOpen={onOpenSsh} /></>}
  </span>
}

function Section({ label, action, children }: { label: string; action?: ReactNode; children: ReactNode }) {
  return <section className="grid gap-1.5">
    <div className="flex min-h-6 items-center justify-between gap-2">
      <h3 className="text-xs font-medium">{label}</h3>
      {action}
    </div>
    {children}
  </section>
}

function repositoryName(path: string) {
  return path.split("/").filter(Boolean).pop() ?? path
}

/** A right-aligned link that jumps to the section this data is managed in, scoped to the
 * computer where applicable. Shown even when the section is empty — it is still the place
 * to manage it. */
function ViewAllAction({ label, onClick }: { label: string; onClick: () => void }) {
  return <button
    type="button"
    aria-label={label}
    onClick={onClick}
    className="inline-flex shrink-0 items-center gap-0.5 rounded-sm text-caption text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
  >
    View all<ChevronRight className="size-3" aria-hidden="true" />
  </button>
}

/** A small ghost xs action that fits the section header row, paired with the View all link. */
function AddAction({ label, disabled, onClick }: { label: string; disabled?: boolean; onClick: (event: MouseEvent<HTMLButtonElement>) => void }) {
  return <Button type="button" variant="ghost" size="xs" aria-label={label} disabled={disabled} onClick={onClick}>
    <Plus aria-hidden="true" data-icon="inline-start" />Add
  </Button>
}

/** The Secrets section, scoped to this computer: assigned secrets with the same row states,
 * inline editor, and remove confirmation as the Secrets page. Add preselects this computer.
 * Only local computers support secrets, so remote computers stay read-only. */
function SecretsSection({ computer, source, actions, onNavigate }: { computer: ApplicationComputer; source: ApplicationSource; actions: ApplicationActions; onNavigate?: (route: ApplicationInitialRoute) => void }) {
  const { configuration } = computer
  const canManage = !computer.device
  const manager = useSecretsManager({ source, onSaveSecret: actions.saveSecret, onRemoveSecret: actions.removeSecret, onRetrySecret: actions.retrySecret })
  // Secret assignments name local computers, so a remote computer never lists them.
  const computerSecrets = canManage ? source.secrets.filter(secret => secret.computers.includes(configuration.name)) : []
  const adding = Boolean(manager.editor && !manager.editor.secret)

  if (!canManage) return <Section label="Secrets">
    <p className="text-xs text-muted-foreground">Secrets are available only for computers on this device.</p>
  </Section>

  const action = <div className="flex items-center gap-2">
    <AddAction label="Add secret" disabled={manager.saving || manager.busy !== null} onClick={(event) => manager.openEditor(event.currentTarget, { initialComputers: [configuration.name] })} />
    {onNavigate && <ViewAllAction label="View all secrets" onClick={() => onNavigate({ tab: "secrets" })} />}
  </div>

  return <Section label="Secrets" action={action}>
    <ListCard>
      {adding && <div className="border-b border-border"><AddSecretEditor manager={manager} /></div>}
      {computerSecrets.length > 0
        ? <ul className="divide-y divide-border" aria-label={`Secrets for ${configuration.name}`}>{computerSecrets.map(secret => <SecretRow key={secret.id} secret={secret} manager={manager} />)}</ul>
        : !adding && <ListRow
              icon={<ListRowIcon aria-hidden="true"><KeyRound className="size-3.5" /></ListRowIcon>}
              title={<span className="font-normal text-muted-foreground">No secrets assigned.</span>}
              detail=""
            />}
    </ListCard>
  </Section>
}

const inlinePortFormClassName = "grid grid-cols-2 items-end gap-2 p-3 sm:grid-cols-[6rem_minmax(0,1fr)_8rem_auto] sm:gap-3"

/** The Ports section, scoped to this computer and backed by the same live network data as the
 * Network page. Add/edit/remove/open reuse the Network page's form, confirmation, and actions.
 * Falls back to the computer's cached ports only when live network data is absent. */
function PortsSection({ computer, source, actions, browser, active, onNavigate }: { computer: ApplicationComputer; source: ApplicationSource; actions: ApplicationActions; browser: string; active: boolean; onNavigate?: (route: ApplicationInitialRoute) => void }) {
  const { configuration } = computer
  const target = computerTarget(computer)
  const fieldID = useId()
  const useLive = source.network?.computers.some(item => item.computer === target) ?? false
  const controller = useNetworkPorts({ computers: [computer], network: source.network, error: source.networkError, actions, active })
  const { draft, rows } = controller
  const fallbackPorts = computer.ports ?? []
  const hasDiscoveryError = Boolean(controller.error) || controller.errors.length > 0
  const hasPorts = useLive ? rows.length > 0 : fallbackPorts.length > 0
  const loading = controller.loading && !hasDiscoveryError
  const canAdd = Boolean(actions.saveNetworkPort) && controller.localComputers.length > 0
  const inlineForm = <NetworkPortForm controller={controller} fieldID={fieldID} hideComputer className={inlinePortFormClassName} />

  const action = <div className="flex items-center gap-2">
    {canAdd && (controller.addDisabledReason
      ? <Tooltip><TooltipTrigger asChild><span tabIndex={0}><AddAction label="Add port" disabled onClick={() => undefined} /></span></TooltipTrigger><TooltipContent>{controller.addDisabledReason}</TooltipContent></Tooltip>
      : <AddAction label="Add port" disabled={controller.busy} onClick={() => controller.add(target)} />)}
    {onNavigate && <ViewAllAction label="View all network for this computer" onClick={() => onNavigate({ computerSection: "network", computer: configuration.id })} />}
  </div>

  return <Section label="Ports" action={action}>
    {(controller.error || controller.errors.length > 0) && <div role="alert" className="mb-2 flex items-center justify-between gap-3 rounded-md border border-destructive/20 px-3 py-2 text-xs text-destructive">
      <span>{controller.error || controller.errors.join(" · ")}</span>
      {actions.refreshNetwork && <Button size="sm" variant="ghost" onClick={() => void actions.refreshNetwork?.()}>Retry</Button>}
    </div>}
    {loading && <p role="status" aria-label="Loading ports" className="text-xs text-muted-foreground">Checking network services…</p>}
    {(hasPorts || draft || (!hasDiscoveryError && !loading)) && <ListCard>
      {draft && !draft.editing && <div className="border-b border-border">{inlineForm}</div>}
      {useLive
        ? rows.length > 0
          ? <div className="divide-y divide-border">{rows.map(({ computer: portComputer, port, host, error: rowError }) => {
              const key = `${target}:${port.port}`
              if (draft?.editing && draft.port === String(port.port)) return <div key={key} className="last:*:border-b-0">{inlineForm}</div>
              const address = networkAddress(port, host)
              const stateText = networkPortState(portComputer, port, rowError)
              return <ListRow
                key={key}
                icon={<ListRowIcon aria-hidden="true"><Globe className="size-3.5" /></ListRowIcon>}
                title={<span className="truncate font-mono" title={address ? `${port.port} → ${address}` : `Port ${port.port}`}>{address ? `${port.port} → ${address}` : `Port ${port.port}`}</span>}
                detailClassName="whitespace-normal"
                detail={<span className="inline-flex flex-wrap items-center gap-1.5">
                  <PortStateLabel state={stateText} />
                  {port.message && <span className={port.state === "unknown" ? "text-destructive" : "text-muted-foreground"}>· {port.message}</span>}
                </span>}
                actions={<div className="flex shrink-0 items-center gap-0.5 text-muted-foreground">
                  <NetworkPortRowActions controller={controller} computer={portComputer} port={port} state={stateText} browser={browser} host={host} />
                </div>}
              />
            })}</div>
          : !draft && <ListRow
              icon={<ListRowIcon aria-hidden="true"><Globe className="size-3.5" /></ListRowIcon>}
              title={<span className="font-normal text-muted-foreground">No ports</span>}
              detail=""
            />
        : fallbackPorts.length > 0
          ? <div className="divide-y divide-border">{fallbackPorts.map(port => {
              const cachedPort: NetworkPort = {
                ...port, hostPort: port.hostPort ?? null, scheme: port.scheme ?? null,
                configured: port.configured ?? false,
                state: port.hostPort == null || port.configured === false ? "unpublished" : port.listening === true ? "reachable" : port.listening === false ? "waiting" : "unknown",
              }
              const address = networkAddress(cachedPort)
              const stateText = networkPortState(computer, cachedPort)
              const title = address ?? `Port ${port.port}`
              return <ListRow
                key={port.port}
                icon={<ListRowIcon aria-hidden="true"><Globe className="size-3.5" /></ListRowIcon>}
                title={<span className="truncate" title={title}>{title}</span>}
                detail={<PortStateLabel state={stateText} />}
              />
            })}</div>
          : <ListRow
              icon={<ListRowIcon aria-hidden="true"><Globe className="size-3.5" /></ListRowIcon>}
              title={<span className="font-normal text-muted-foreground">No ports</span>}
              detail=""
            />}
    </ListCard>}
  </Section>
}

function OverviewTab({ computer, source, actions, active, onEdit, onNavigate, computerUse }: { computerUse?: ReactNode; computer: ApplicationComputer; source: ApplicationSource; actions: ApplicationActions; active: boolean; onEdit?: () => void; onNavigate?: (route: ApplicationInitialRoute) => void }) {
  const { configuration } = computer
  const repositories = computer.repositories ?? []
  const extraGithub = (computer.githubRepositories ?? []).filter(name => !repositories.some(repo => repo.path.endsWith(name)))
  const hasRepositories = repositories.length > 0 || extraGithub.length > 0

  const resourceTitle = `CPUs at start: ${configuration.cpus} (maximum ${configuration.maxCPUs}) · Memory at start: ${configuration.memoryGiB} GiB (maximum ${configuration.maxMemoryGiB} GiB)`
  const diskDetail = `Workspace disk: ${configuration.workspaceStorageGiB} GiB · Runtime disk: ${configuration.runtimeStorageGiB} GiB`

  return <div className="grid gap-5">
    <Section label="Resources">
      <ListCard>
        <ListRow
          icon={<ListRowIcon aria-hidden="true"><Cpu className="size-3.5" /></ListRowIcon>}
          title={resourceTitle}
          detail={diskDetail}
          actions={onEdit ? <Button type="button" variant="outline" size="xs" onClick={onEdit}>Edit</Button> : undefined}
        />
      </ListCard>
    </Section>

    {computerUse}

    <Section label="Repositories" action={onNavigate ? <ViewAllAction label="View all files for this computer" onClick={() => onNavigate({ computerSection: "files", computer: configuration.id })} /> : undefined}>
      <ListCard divided={repositories.length + extraGithub.length > 1}>
        {hasRepositories ? <>
          {repositories.map(repo => <ListRow
            key={repo.path}
            icon={<ListRowIcon aria-hidden="true"><GitBranch className="size-3.5" /></ListRowIcon>}
            title={<span className="truncate" title={repo.path}>{repositoryName(repo.path)}</span>}
            detail={repo.branch}
          />)}
          {extraGithub.map(name => <ListRow
            key={name}
            icon={<ListRowIcon aria-hidden="true"><GitBranch className="size-3.5" /></ListRowIcon>}
            title={<span className="truncate" title={name}>{name}</span>}
            detail="Clones on next start"
          />)}
        </> : <ListRow
          icon={<ListRowIcon aria-hidden="true"><GitBranch className="size-3.5" /></ListRowIcon>}
          title={<span className="font-normal text-muted-foreground">No repositories cloned yet.</span>}
          detail=""
        />}
      </ListCard>
    </Section>

    <SecretsSection computer={computer} source={source} actions={actions} onNavigate={onNavigate} />

    <PortsSection computer={computer} source={source} actions={actions} browser={source.preferences.browser} active={active} onNavigate={onNavigate} />
  </div>
}

export function ComputerDetailPage({ computer, source, actions, controls }: {
  computer: ApplicationComputer
  source: ApplicationSource
  actions: ApplicationActions
  controls: ComputerDetailControls
}) {
  const { configuration } = computer
  const target = computerTarget(computer)
  const state = computer.state
  const canStop = state === "running" || state === "starting"
  // A starting or stopping computer can be neither edited nor deleted until it settles.
  const busyReason = computerBusyReason(computer)

  // The detail page edits and deletes this computer in place using the same flow as the list.
  const editingContext = controls.editing
  const editing = useComputerEditing({
    configurations: editingContext?.configurations ?? [configuration],
    getDeviceId: editingContext?.getDeviceId,
    onCommitComputer: editingContext?.onCommitComputer,
    onDeleteComputer: editingContext?.onDeleteComputer,
    onConfigurationsChange: editingContext?.onConfigurationsChange ?? (() => {}),
    validateOperation: editingContext?.validateOperation,
    isComputerRunning: editingContext?.isComputerRunning,
    interactionDisabled: controls.configurationLocked,
    getConfigurationBusyReason: (item) => item.id === configuration.id ? busyReason : undefined,
    // Leaving the page (⌘1–7, ⌘[, the breadcrumb) and returning restores an unsaved edit.
    draftKey: `computer-detail:${configuration.id}`,
  })
  const canEdit = Boolean(editingContext) && !controls.configurationLocked && !busyReason
  const isEditing = Boolean(editing.editor)
  const editDeviceConfigurations = editingContext?.getDeviceId
    ? (editingContext.configurations).filter(item => (editingContext.getDeviceId!(item) ?? "") === editing.deviceId)
    : (editingContext?.configurations ?? [configuration])

  const displayName = computer.device ? `${configuration.name} on ${computer.device.name}` : configuration.name
  const editMenuActions: MenuAction[] = editingContext ? computerEditMenu({
    configuration,
    displayName,
    disabled: controls.configurationLocked || editing.interactionDisabled || Boolean(controls.changesBlocked),
    busyReason,
    created: Boolean(editingContext.isComputerCreated?.(configuration)),
    running: state === "running",
    separatorBefore: controls.menuActions.length > 0,
    onEdit: () => editing.startEdit(configuration),
    onDuplicate: controls.onDuplicate,
    onAddDesktop: (vm) => {
      editing.beginOperation()
      editing.captureBaseline()
      void editing.save({ ...vm, desktop: { startWithComputer: true } }, configuration.id, editingContext.getDeviceId?.(configuration) ?? "")
    },
  }) : []
  const menuActions = [...controls.menuActions, ...editMenuActions]

  const access = source.sshAccess?.computers.find(row => row.computer === target)
  const sshAvailable = Boolean(source.sshAccess || actions.refreshSshAccess)
  const sshStale = Boolean((source.sshAccessError && !computer.device) || computer.device?.connected === false || computer.freshness === "stale")
  const pendingSecrets = !computer.device
    ? source.secrets.filter(secret => secret.state === "restart-required" && secret.computers.includes(configuration.name)).map(secret => secret.name)
    : []

  const showStorage = !computer.device && Boolean(actions.readWorkspaceStorage)
  const showAccess = sshAvailable
  const tabs: { value: ComputerDetailTab; label: string; visible: boolean }[] = [
    { value: "overview", label: "Overview", visible: true },
    { value: "checkpoints", label: "Checkpoints", visible: true },
    { value: "storage", label: "Storage", visible: showStorage },
    { value: "access", label: "SSH", visible: showAccess },
  ]
  const visibleTabs = tabs.filter(tab => tab.visible)
  const menuPopovers: MenuPopovers = {
    ...controls.popovers,
    delete: close => <DeleteComputerBody
      displayName={displayName}
      details={controls.deleteDetails}
      onClose={close}
      onDelete={async () => {
        if (await editing.deleteWithNotice(configuration)) controls.onBack()
      }}
    />,
  }
  const menu = <ActionsMenu label={`More actions for ${configuration.name}`} items={menuActions} popovers={menuPopovers} openPanel={controls.menuRequest} />
  const activeTab = visibleTabs.some(tab => tab.value === controls.activeTab) ? controls.activeTab : "overview"

  const reasons = controls.disabledReasons ?? {}
  const restartAvailability = computerAvailability(computer, source)

  return <TooltipProvider delayDuration={150}>
    <div className="flex h-full min-h-0 flex-col">
      <ListHeader
        heading={<nav aria-label="Breadcrumb" className="flex min-w-0 items-center gap-1">
          <button type="button" className={cn(listHeadingClassName, "shrink-0 rounded-sm hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none")} onClick={controls.onBack}>Computers</button>
          <ChevronRight className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span className={cn(listHeadingClassName, "truncate")} title={configuration.name}>{configuration.name}</span>
        </nav>}
        subtitle={<span data-slot="computer-detail-status"><DetailSubtitle computer={computer} source={source} readOnly={controls.readOnly} pendingSecrets={pendingSecrets} sshAccess={access} sshStale={sshStale} onCancel={actions.cancelOperation} onOpenSsh={showAccess ? () => controls.onSelectTab("access") : undefined} /></span>}
        actions={<div className="flex shrink-0 items-center gap-1">
          <DisabledReason reason={controls.canOpen ? undefined : reasons.open}><Button type="button" variant="outline" size="xs" aria-label={`Open ${configuration.name} in ${source.preferences.terminal}`} disabled={!controls.canOpen} onClick={controls.onTerminal}><Terminal aria-hidden="true" data-icon="inline-start" />Terminal</Button></DisabledReason>
          <DisabledReason reason={controls.canOpen ? undefined : reasons.open}><Button type="button" variant="outline" size="xs" aria-label={`Open ${configuration.name} in ${source.preferences.editor}`} disabled={!controls.canOpen} onClick={controls.onEditor}><Code aria-hidden="true" data-icon="inline-start" />Editor</Button></DisabledReason>
          {controls.desktop && <Button type="button" variant="outline" size="xs" aria-label={`Open ${configuration.name} desktop`} disabled={controls.desktop.disabled} onClick={controls.desktop.onClick}><Monitor aria-hidden="true" data-icon="inline-start" />Desktop</Button>}
          {canStop
            ? <LifecycleControl guard={controls.lifecycleGuard} computer={computer} action="stop" disabled={!controls.canStop} reason={reasons.stop}>
              {({ onClick, disabled }) => <Button type="button" variant="outline" size="xs" aria-label={`Stop ${configuration.name}`} disabled={disabled} onClick={onClick}><Square aria-hidden="true" data-icon="inline-start" />{!disabled && controls.lifecycleGuard.check(computer, "stop").kind === "confirm" ? "Stop…" : "Stop"}</Button>}
            </LifecycleControl>
            : <LifecycleControl guard={controls.lifecycleGuard} computer={computer} action="start" disabled={!controls.canStart} reason={reasons.start}>
              {({ onClick, disabled }) => <Button type="button" variant="outline" size="xs" aria-label={`Start ${configuration.name}`} disabled={disabled} onClick={onClick}><Play aria-hidden="true" data-icon="inline-start" />{!disabled && controls.lifecycleGuard.check(computer, "start").kind === "confirm" ? "Start…" : "Start"}</Button>}
            </LifecycleControl>}
          {menuActions.length > 0 && menu}
        </div>}
      />

      {Boolean(computer.pendingSecretRevocations?.length) && <div role="note" aria-label="Pending secret revocation" className="flex items-center gap-2 border-b border-warning/20 bg-warning/5 px-3 py-2 text-xs text-warning">
        <TriangleAlert className="size-3.5 shrink-0" aria-hidden="true" />
        <p className="min-w-0 flex-1 break-words">May still have access to {computer.pendingSecretRevocations!.join(", ")} until it restarts.</p>
        <LifecycleControl guard={controls.lifecycleGuard} computer={computer} action="restart" disabled={controls.readOnly || !restartAvailability.canRestart} reason={controls.readOnly ? undefined : restartAvailability.reasons.restart}>
          {({ onClick, disabled }) => <Button variant="outline" size="xs" aria-label={`Restart ${configuration.name}`} disabled={disabled} onClick={onClick}><RotateCw aria-hidden="true" />Restart</Button>}
        </LifecycleControl>
      </div>}

      {isEditing && editing.editor ? (
        <ScrollArea className="min-h-0 flex-1">
          <Section label={`Edit ${configuration.name}`}>
            <div className="rounded-lg border border-border bg-background">
              <ComputerEditor
                key={`${editing.editor.draft.id}:${editing.editorResetToken}`}
                saving={editing.committing}
                blockedReason={editing.saveBlockedReason}
                capacity={editing.deviceId ? undefined : deviceCapacityFrom(source.deviceCapacity)}
                deviceName={computer.device?.name}
                deviceId={editing.deviceId}
                editorHeader={editingContext?.devices ? <label className="grid gap-1 text-caption text-muted-foreground">Run on<select aria-label="Run on" className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground" value={editing.deviceId} disabled={Boolean(editing.editor.originalID) || editing.committing} onChange={event => editing.setDeviceId(event.target.value)}><option value="">This device</option>{editingContext.devices.map(device => <option key={device.id} value={device.id} disabled={!device.connected}>{device.name}{!device.connected ? " (offline)" : ""}</option>)}</select></label> : undefined}
                focusRequest={editing.editorFocusRequest}
                created={Boolean(editing.editor.originalID && editingContext?.isComputerCreated?.(configuration))}
                running={Boolean(editing.editor.originalID && editingContext?.isComputerRunning?.(configuration))}
                editor={editing.editor}
                baselineComputer={editing.editorBaseline ?? undefined}
                conflict={editing.editorConflict}
                review={editing.editorReview}
                configurations={editDeviceConfigurations}
                onCancel={() => editing.setEditor(null)}
                onSave={editing.save}
                onDraftChange={(draft) => editing.setEditor({ ...editing.editor!, draft })}
                onReview={editing.reviewConflict}
                onDiscard={() => editing.setEditor(null)}
              />
            </div>
          </Section>
        </ScrollArea>
      ) : (
      <Tabs value={activeTab} onValueChange={value => controls.onSelectTab(value as ComputerDetailTab)} className="flex min-h-0 flex-1 flex-col gap-0">
        <div className="relative z-10 border-b border-border">
          <TabsList variant="line" className="-ml-1.5 w-fit">
            {visibleTabs.map(tab => <TabsTrigger key={tab.value} value={tab.value} className="text-xs group-data-[orientation=horizontal]/tabs:after:bottom-[-4px]">{tab.label}</TabsTrigger>)}
          </TabsList>
        </div>
        <ScrollArea className="min-h-0 flex-1">
          <div className="pt-4">
            <TabsContent value="overview"><OverviewTab computer={computer} source={source} actions={actions} active={activeTab === "overview" && controls.pageActive !== false} onEdit={canEdit ? () => editing.startEdit(configuration) : undefined} onNavigate={controls.onNavigate} computerUse={configuration.desktop ? <ComputerUseSection key={target} computer={target} active={activeTab === "overview" && controls.pageActive !== false} /> : undefined} /></TabsContent>
            <TabsContent value="checkpoints">
              <CheckpointPanel computer={computer} target={target} actions={actions} takenNames={computerNamesOnDevice(source.computers, computer.device?.id)} disabled={controls.configurationLocked || Boolean(computer.lifecycleAction) || Boolean(computer.device?.busy) || computer.freshness === "stale"} onExport={controls.onCheckpointExport} exportDisabled={controls.checkpointExportDisabled} forkedAction={controls.onCheckpointForkedAction} restoredAction={controls.onCheckpointRestoredAction} />
            </TabsContent>
            {showStorage && actions.readWorkspaceStorage && <TabsContent value="storage">
              <WorkspaceStoragePanel key={configuration.id} computerId={configuration.id} computerName={configuration.name} deviceName={computer.device?.name} running={state === "running"} disabled={controls.configurationLocked || controls.computerOperationBusy} read={actions.readWorkspaceStorage} reclaim={actions.reclaimWorkspaceStorage} />
            </TabsContent>}
            {showAccess && <TabsContent value="access">
              <SshAccessRow embedded readOnly={controls.readOnly || controls.computerOperationBusy} computer={computer} access={access} save={actions.saveSshAccess} connection={actions.sshConnection} stale={sshStale} />
            </TabsContent>}
          </div>
        </ScrollArea>
      </Tabs>
      )}
    </div>
  </TooltipProvider>
}
