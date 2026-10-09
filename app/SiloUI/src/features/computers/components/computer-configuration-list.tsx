import { ActionsMenu, type MenuAction, type MenuPopovers } from "@/components/actions-menu"
import { useEffect, useEffectEvent, useId, useMemo, useRef, useState, type DragEvent, type KeyboardEvent, type ReactNode } from "react"
import { CopyPlus, GripVertical, Pencil, Plus, Trash2 } from "lucide-react"
import { DropdownMenu } from "radix-ui"

import { ConfirmPopover } from "@/components/confirm-popover"
import { ListHeader, listHeadingClassName } from "@/components/list-header"
import { Button } from "@/components/ui/button"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import { configurationRequest } from "@/features/onboarding/model/computer-configuration"
import { ComputerEditor } from "@/features/computers/components/computer-editor"
import { useComputerEditing } from "@/features/computers/model/use-computer-editing"
import { ComputerAction, ComputerList, ComputerListItem, ComputerListRow, type ComputerIconState, type ComputerRowTone } from "@/features/computers/components/computer-list"
import { computerSummary } from "@/features/computers/model/computer-summary"
import { deleteComputerDescription, deleteComputerTitle } from "@/features/computers/model/delete-computer-copy"
import { DeleteComputerBody, type DeleteComputerDetails } from "@/features/computers/components/delete-computer-confirmation"
import { computerEditMenu } from "@/features/computers/model/computer-edit-menu"
import { OperatingSystemField, type ComputerOs } from "@/features/computers/components/os-badge"
import { showActionFailure } from "@/lib/operation-toast"
import { useComputerEditorDrafts, type MacosEditorState } from "@/features/computers/model/editor-drafts-context"
import { defaultMacosFields, type MacosComputerRequest } from "@/features/macos-computers/model/macos-computers"
import { MacosComputerForm } from "@/features/macos-computers/components/macos-computer-form"
import { MacosComputerRow } from "@/features/macos-computers/components/macos-computer-row"
import { useMacosComputers } from "@/features/macos-computers/model/macos-computers"
import { restoreFocus } from "@/lib/focus"
import type { DeviceCapacity } from "@/features/computers/model/computer-limits"
import type { ComputerEditorDraft } from "@/features/onboarding/model/onboarding-draft"

export interface ComputerRowPresentation {
  expandedContent?: ReactNode
  menuActions?: MenuAction[]
  /** Popovers opened by `menuActions` entries with a matching `popover` key, anchored to the ⋯ button. */
  popovers?: MenuPopovers
  kindBadge?: ReactNode
  badge?: ReactNode
  detail?: ReactNode
  detailClassName?: string
  icon?: ReactNode
  iconState?: ComputerIconState
  actions?: ReactNode
  actionsClassName?: string
  tone?: ComputerRowTone
  busy?: boolean
  /** Disables the row's mutating controls (reorder, ⋯ menu) while work runs. Opening the
   * computer's page stays available so its progress and errors remain reachable. */
  suppressInteractions?: boolean
  /** False when the row has no page to open yet (a computer that is still being created). */
  openable?: boolean
  /** What the Delete dialog states (checkpoints, size) and offers (Export, then delete). */
  deleteDetails?: DeleteComputerDetails
}

interface ComputerConfigurationListProps {
  configurations: readonly SetupComputerConfiguration[]
  devices?: readonly { id: string; name: string; connected: boolean }[]
  getDeviceId?: (configuration: SetupComputerConfiguration) => string | undefined
  onCommitComputer?: (configuration: SetupComputerConfiguration, original: SetupComputerConfiguration | undefined, deviceId: string, baseline?: SetupComputerConfiguration[]) => Promise<void>
  onDeleteComputer?: (configuration: SetupComputerConfiguration, baseline?: SetupComputerConfiguration[]) => Promise<void>
  onConnectDevice?: () => void
  onImportComputer?: () => void
  /** Wraps the Add button so an import review popover can anchor to it. */
  importPopover?: (addButton: ReactNode) => ReactNode
  onConfigurationsChange: (configurations: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[]) => Promise<void> | void
  getRowPresentation?: (configuration: SetupComputerConfiguration) => ComputerRowPresentation
  sortPriority?: (configuration: SetupComputerConfiguration) => number
  /** This device's saved position of a row, lower first; rows without one follow in `configurations` order. */
  orderRank?: (configuration: SetupComputerConfiguration) => number | undefined
  /** Saves a reordered list (every row, local and remote, by id) instead of changing the
   * computer configuration. Without it, only local rows reorder, through `onConfigurationsChange`. */
  onReorder?: (computerIds: string[]) => void
  newComputerRequest?: number
  onNewComputerRequestHandled?: (id: number) => void
  /** Opens the computer's detail page when its row body is activated. */
  onOpenComputer?: (configuration: SetupComputerConfiguration) => void
  /** Runs Duplicate against a configuration on behalf of its detail page (which opens the list editor). */
  configurationActionRequest?: { token: number; computerId: string; action: "duplicate" }
  onConfigurationActionHandled?: (token: number) => void
  interactionDisabled?: boolean
  summary?: ReactNode
  footer?: ReactNode
  initialEditorDraft?: ComputerEditorDraft | null
  onEditorDraftChange?: (editor: ComputerEditorDraft | null) => void
  isComputerCreated?: (configuration: SetupComputerConfiguration) => boolean
  isComputerRunning?: (configuration: SetupComputerConfiguration) => boolean
  validateOperation?: (configuration: SetupComputerConfiguration, isNew: boolean, deviceId?: string) => string | undefined
  /** The CPUs and memory of a device ("" is this one), when known: fits new-computer defaults and presets, and rejects ceilings above it. */
  getDeviceCapacity?: (deviceId: string) => DeviceCapacity | undefined
  /** Why a computer cannot be edited or deleted now (it is starting or stopping), if so. */
  getConfigurationBusyReason?: (configuration: SetupComputerConfiguration) => string | undefined
  /** Keeps an open editor across navigation within a `ComputerEditorDraftsProvider`. */
  editorDraftKey?: string
  /** Lists this device's macOS computers with the others and offers macOS when creating one. */
  includeMacosComputers?: boolean
}

export function ComputerConfigurationList({ devices, getDeviceId, onCommitComputer, onDeleteComputer, onConnectDevice, onImportComputer, importPopover, configurations, onConfigurationsChange, getRowPresentation, sortPriority, orderRank, onReorder, interactionDisabled: interactionDisabledProp = false, newComputerRequest, onNewComputerRequestHandled, onOpenComputer, configurationActionRequest, onConfigurationActionHandled, summary, footer, initialEditorDraft = null, onEditorDraftChange, validateOperation, isComputerCreated, isComputerRunning, getDeviceCapacity, getConfigurationBusyReason, editorDraftKey, includeMacosComputers = false }: ComputerConfigurationListProps) {
  const {
    deviceId, setDeviceId,
    committing,
    interactionDisabled,
    saveBlockedReason,
    editor, setEditor,
    editorBaseline, editorConflict, editorReview, editorResetToken,
    editorFocusRequest, setEditorFocusRequest,
    baselineRef,
    captureBaseline, beginOperation, dispatchChange,
    startEdit, startAdd, startDuplicate, save, remove, reviewConflict, deleteWithNotice,
  } = useComputerEditing({ configurations, getDeviceId, onCommitComputer, onDeleteComputer, onConfigurationsChange, validateOperation, isComputerRunning, onEditorDraftChange, initialEditorDraft, interactionDisabled: interactionDisabledProp, getDeviceCapacity, getConfigurationBusyReason, draftKey: editorDraftKey })

  // macOS computers share this list; their store is separate and absent where macOS is unsupported.
  const macos = useMacosComputers(includeMacosComputers)
  const macosState = macos?.snapshot.state?.supported ? macos.snapshot.state : null
  const macosComputers = macosState?.computers ?? []
  // The operating system choice and the macOS fields belong to the editor they were made in, and
  // are kept with its stored draft so navigating away and back restores them.
  const drafts = useComputerEditorDrafts()
  const [storedMacosForm, setStoredMacosForm] = useState<MacosEditorState | null>(() => {
    const stored = editorDraftKey ? drafts?.get(editorDraftKey)?.macosForm : undefined
    // A creation started before leaving is not tracked by this instance; it is no longer pending here.
    return stored ? { ...stored, creating: false } : null
  })
  const macosFormRef = useRef(storedMacosForm)
  const editorRef = useRef(editor)
  const mountedRef = useRef(true)
  const listRoot = useRef<HTMLDivElement>(null)
  const focusOsSelect = useRef(false)
  useEffect(() => {
    editorRef.current = editor
    mountedRef.current = true
    return () => { mountedRef.current = false }
  }, [editor])
  function persistMacosForm(next: MacosEditorState | null) {
    macosFormRef.current = next
    const cached = editorDraftKey ? drafts?.get(editorDraftKey) : undefined
    if (editorDraftKey && cached) drafts?.set(editorDraftKey, { ...cached, macosForm: next ?? undefined })
  }
  function setMacosForm(next: MacosEditorState | null) {
    persistMacosForm(next)
    setStoredMacosForm(next)
  }
  const macosForm = storedMacosForm && storedMacosForm.editorId === editor?.draft.id ? storedMacosForm : null
  const newOs: ComputerOs = macosForm?.os ?? "linux"
  // Focus follows the operating system choice into the fields that replace the old ones.
  useEffect(() => {
    if (!focusOsSelect.current) return
    focusOsSelect.current = false
    listRoot.current?.querySelector<HTMLSelectElement>("select[aria-label='Operating system']")?.focus()
  }, [newOs])
  const [addOpen, setAddOpen] = useState(false)
  const addSelected = useRef<"editor" | "external" | null>(null)
  const [draggedID, setDraggedID] = useState<string | null>(null)
  const [announcement, setAnnouncement] = useState("")
  const reorderHelpId = useId()
  const headingId = useId()
  const addButton = useRef<HTMLButtonElement>(null)
  const editorTriggers = useRef(new Map<string, HTMLButtonElement>())
  const previousEditor = useRef(editor)
  useEffect(() => {
    const closed = previousEditor.current
    previousEditor.current = editor
    if (!closed || editor || document.activeElement !== document.body) return
    // The row is replaced while editing, so return to its newly mounted control.
    const sourceId = closed.originalID ?? closed.displayAfterID
    restoreFocus((sourceId ? editorTriggers.current.get(sourceId) : undefined) ?? addButton.current)
  }, [editor])

  // Rows in this device's saved order; rows it has not placed yet keep their place after them.
  const orderedConfigurations = useMemo(() => {
    if (!orderRank) return configurations
    return [...configurations]
      .map((configuration, index) => ({ configuration, index, rank: orderRank(configuration) ?? Number.POSITIVE_INFINITY }))
      .sort((a, b) => a.rank - b.rank || a.index - b.index)
      .map(({ configuration }) => configuration)
  }, [configurations, orderRank])

  const displayConfigurations = useMemo(() => {
    // Inject the open editor's draft whenever no live configuration carries its id: a new/
    // duplicated configuration, or one deleted elsewhere while its editor stayed open (so the
    // conflict notice remains visible instead of the row vanishing).
    const detached = editor ? !configurations.some(({ id }) => id === editor.draft.id) : false
    if (!sortPriority) {
      const next = [...orderedConfigurations]
      if (editor && detached) next.splice(editor.insertAt, 0, editor.draft)
      return next
    }

    const next = [...orderedConfigurations]
      .map((configuration, index) => ({ configuration, index }))
      .sort((a, b) => sortPriority(a.configuration) - sortPriority(b.configuration) || a.index - b.index)
      .map(({ configuration }) => configuration)
    if (editor && detached) {
      const sourceIndex = editor.displayAfterID ? next.findIndex(({ id }) => id === editor.displayAfterID) : -1
      next.splice(sourceIndex >= 0 ? sourceIndex + 1 : next.length, 0, editor.draft)
    }
    return next
  }, [editor, configurations, orderedConfigurations, sortPriority])

  const consumedNewRequest = useRef(0)
  const openRequestedVM = useEffectEvent((id: number) => {
    if (!interactionDisabled) {
      if (editor) setEditorFocusRequest(id)
      else startAdd()
    }
    onNewComputerRequestHandled?.(id)
  })
  useEffect(() => {
    if (!newComputerRequest || consumedNewRequest.current === newComputerRequest) return
    consumedNewRequest.current = newComputerRequest
    openRequestedVM(newComputerRequest)
  }, [newComputerRequest])

  const consumedConfigurationAction = useRef(0)
  const runConfigurationAction = useEffectEvent((request: NonNullable<ComputerConfigurationListProps["configurationActionRequest"]>) => {
    const configuration = configurations.find(({ id }) => id === request.computerId)
    if (configuration && !interactionDisabled) startDuplicate(configuration)
    onConfigurationActionHandled?.(request.token)
  })
  useEffect(() => {
    if (!configurationActionRequest || consumedConfigurationAction.current === configurationActionRequest.token) return
    consumedConfigurationAction.current = configurationActionRequest.token
    runConfigurationAction(configurationActionRequest)
  }, [configurationActionRequest])

  const isRemote = (configuration: SetupComputerConfiguration) => Boolean(getDeviceId?.(configuration))
  // With `onReorder`, this device saves its own order of every row. Otherwise only local rows
  // reorder (their order is part of this device's configuration) and positions count them only.
  const reorderable = (configuration: SetupComputerConfiguration) => Boolean(onReorder) || !isRemote(configuration)
  const orderable = displayConfigurations.filter((configuration) => reorderable(configuration) && configurations.some(({ id }) => id === configuration.id))
  // A keyboard move waits for the source to publish the previous one, so rapid presses
  // never recompute from the stale `configurations` the first move started from.
  const reorderPending = useRef(false)
  useEffect(() => { reorderPending.current = false }, [configurations])

  function reorder(id: string, targetIndex: number) {
    if (interactionDisabled || editor || reorderPending.current) return
    // Reorder against the order captured when the drag/keyboard move began, so the change
    // carries that order as `expectedOrder` and does not fold in concurrent edits. A saved
    // display order has no configuration to conflict with, so it reorders the current rows.
    const base = onReorder ? [...orderedConfigurations] : (baselineRef.current ?? [...configurations]).filter((configuration) => !isRemote(configuration))
    const reorderBaseline = baselineRef.current ? base : undefined
    const from = orderable.findIndex((configuration) => configuration.id === id)
    const boundedTarget = Math.max(0, Math.min(targetIndex, orderable.length - 1))
    if (from < 0 || from === boundedTarget) return
    beginOperation()
    const moved = orderable[from]
    const target = orderable[boundedTarget]
    let updated: SetupComputerConfiguration[]

    if (sortPriority) {
      const priority = sortPriority(moved)
      if (sortPriority(target) !== priority) {
        setAnnouncement(`${moved.name} can only be reordered within its status group.`)
        return
      }
      const bucket = base.filter((configuration) => sortPriority(configuration) === priority)
      const bucketFrom = bucket.findIndex((configuration) => configuration.id === moved.id)
      const bucketTarget = bucket.findIndex((configuration) => configuration.id === target.id)
      if (bucketFrom < 0 || bucketTarget < 0) return
      const [bucketMoved] = bucket.splice(bucketFrom, 1)
      bucket.splice(bucketTarget, 0, bucketMoved)
      let bucketIndex = 0
      updated = base.map((configuration) => sortPriority(configuration) === priority ? bucket[bucketIndex++] : configuration)
    } else {
      updated = [...base]
      const configuredFrom = updated.findIndex((configuration) => configuration.id === id)
      const configuredTarget = updated.findIndex((configuration) => configuration.id === target.id)
      if (configuredFrom < 0 || configuredTarget < 0) return
      const [configuredMoved] = updated.splice(configuredFrom, 1)
      updated.splice(configuredTarget, 0, configuredMoved)
    }

    if (onReorder) {
      onReorder(updated.map(({ id }) => id))
      setAnnouncement(`${moved.name} moved to position ${boundedTarget + 1} of ${orderable.length}.`)
      return
    }
    const pending = dispatchChange(configurationRequest(updated).computers, reorderBaseline)
    if (pending) {
      reorderPending.current = true
      void pending.finally(() => { reorderPending.current = false })
    }
    setAnnouncement(`${moved.name} moved to position ${boundedTarget + 1} of ${orderable.length}.`)
  }

  function handleReorderKey(event: KeyboardEvent<HTMLElement>, configuration: SetupComputerConfiguration) {
    if (interactionDisabled || editor) return
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return
    event.preventDefault()
    if (reorderPending.current) return
    captureBaseline()
    const from = orderable.findIndex(({ id }) => id === configuration.id)
    reorder(configuration.id, from + (event.key === "ArrowUp" ? -1 : 1))
  }

  function drop(event: DragEvent, target: SetupComputerConfiguration, rowDisabled = false) {
    event.preventDefault()
    const targetIndex = orderable.findIndex(({ id }) => id === target.id)
    const id = draggedID || event.dataTransfer.getData("text/plain")
    setDraggedID(null)
    if (interactionDisabled || rowDisabled || targetIndex < 0 || !id) return
    reorder(id, targetIndex)
  }

  const localLinuxNames = configurations.filter(configuration => !getDeviceId?.(configuration)).map(({ name }) => name)

  async function createMacos(editorId: string, request: MacosComputerRequest) {
    const current = macosFormRef.current
    if (!macos || !current || current.editorId !== editorId) return
    setMacosForm({ ...current, creating: true })
    const stillHere = () => mountedRef.current && editorRef.current?.draft.id === editorId && macosFormRef.current?.editorId === editorId && macosFormRef.current.os === "macos"
    try {
      await macos.store.create(request)
    } catch (error) {
      showActionFailure(`Could not create ${request.name}`, error, undefined, { native: false })
      if (stillHere() && macosFormRef.current) setMacosForm({ ...macosFormRef.current, creating: false })
      return
    }
    // Only the editor that started the creation closes; one that moved on is left as it is.
    if (stillHere()) {
      setMacosForm(null)
      setEditor(null)
    } else if (editorDraftKey && drafts?.get(editorDraftKey)?.editor.draft.id === editorId) drafts.delete(editorDraftKey)
  }

  const computerCount = configurations.length + macosComputers.length
  const remoteCount = configurations.filter(configuration => getDeviceId?.(configuration)).length

  return (
    <>
      <div ref={listRoot} role="group" aria-labelledby={headingId} className="flex h-full min-h-0 flex-col">
        <ListHeader
          heading={<h3 id={headingId} className={listHeadingClassName}>Computers</h3>}
          subtitle={summary ?? <>{computerCount} {computerCount === 1 ? "computer" : "computers"} · {computerCount - remoteCount} on this device · {remoteCount} on other devices</>}
          actions={(importPopover ?? ((node: ReactNode) => node))(<DropdownMenu.Root open={addOpen} onOpenChange={setAddOpen}>
            <DropdownMenu.Trigger asChild>
              <Button ref={addButton} type="button" variant="outline" size="xs" disabled={interactionDisabled}>
                <Plus aria-hidden="true" data-icon="inline-start" /> Add
              </Button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal><DropdownMenu.Content aria-label="Add computer" aria-labelledby={undefined} align="end" sideOffset={4} onCloseAutoFocus={event => {
              // Inline editors take focus; external actions retain the menu's normal return target.
              if (addSelected.current === "editor") {
                event.preventDefault()
                setEditorFocusRequest(request => request + 1)
              }
              addSelected.current = null
            }} className="silo-portal z-50 grid w-48 gap-1 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md">
              <DropdownMenu.Item className="rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent focus:bg-accent focus:outline-none" onSelect={() => { addSelected.current = "editor"; startAdd() }}>New computer</DropdownMenu.Item>
              {onConnectDevice && <DropdownMenu.Item className="rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent focus:bg-accent focus:outline-none" onSelect={() => { addSelected.current = "external"; onConnectDevice() }}>Connect device…</DropdownMenu.Item>}
              {onImportComputer && <DropdownMenu.Item className="rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent focus:bg-accent focus:outline-none" onSelect={() => { addSelected.current = "external"; onImportComputer() }}>Import computer…</DropdownMenu.Item>}
            </DropdownMenu.Content></DropdownMenu.Portal>
          </DropdownMenu.Root>)}
        />

        {macos?.snapshot.error && <div role="alert" className="mb-2 flex items-center justify-between gap-2 rounded-md border border-destructive/30 p-2 text-xs text-destructive">
          <span className="min-w-0">Could not read the macOS computers. {macos.snapshot.error}</span>
          <Button type="button" variant="outline" size="xs" onClick={() => void macos.store.refresh()}>Retry</Button>
        </div>}
        {macosState && macos?.snapshot.warning && <p role="status" className="mb-2 text-[11px] text-amber-700 dark:text-amber-400">{macos.snapshot.warning}</p>}
        <ComputerList label="Configured computers" className="max-h-full min-h-0" data-testid="computer-configuration-list">
            {displayConfigurations.map((configuration) => {
              const isEditing = editor?.draft.id === configuration.id
              const runningVM = Boolean(isComputerRunning?.(configuration))
              // Starting or stopping computers can be neither edited nor deleted until they settle.
              const busyReason = getConfigurationBusyReason?.(configuration)
              const deleteTooltip = runningVM ? "Stop the computer before deleting it." : busyReason
              const presentation = getRowPresentation?.(configuration)
              const rowInteractionsDisabled = interactionDisabled || Boolean(presentation?.suppressInteractions)
              const reorderDisabled = rowInteractionsDisabled || Boolean(editor)
              // Only a computer being created chooses its operating system; saved ones keep theirs.
              const creating = Boolean(isEditing && editor && !editor.originalID)
              const newMacosForm = creating && Boolean(macosState) && newOs === "macos"
              const macosFields = macosForm ?? { editorId: configuration.id, os: "linux" as const, creating: false, ...defaultMacosFields(getDeviceCapacity?.("")) }
              const osField = creating ? <OperatingSystemField value={newMacosForm ? "macos" : "linux"} macosSupported={Boolean(macosState)} disabled={committing || macosFields.creating} onChange={os => { focusOsSelect.current = true; setMacosForm({ ...macosFields, os }) }} /> : undefined
              const deviceName = devices?.find(device => device.id === getDeviceId?.(configuration))?.name
              const deletionName = deviceName ? `${configuration.name} on ${deviceName}` : configuration.name
              return (
                <ComputerListItem
                  key={configuration.id}
                  data-computer-id={configuration.id}
                  data-computer-name={configuration.name}
                  aria-busy={presentation?.busy || undefined}
                  className="min-w-0 bg-background"
                  onDragOver={(event) => event.preventDefault()}
                  onDrop={(event) => drop(event, configuration, Boolean(presentation?.suppressInteractions))}
                >
                  {isEditing && editor && newMacosForm ? (
                    <MacosComputerForm
                      key={editor.draft.id}
                      fields={macosFields}
                      onChange={changes => setMacosForm({ ...macosFields, ...changes })}
                      creating={macosFields.creating}
                      existingNames={macosComputers.map(({ name }) => name)}
                      otherNames={localLinuxNames}
                      capacity={getDeviceCapacity?.("")}
                      osField={osField}
                      onCancel={() => { setMacosForm(null); setEditor(null) }}
                      onCreate={request => void createMacos(editor.draft.id, request)}
                    />
                  ) : isEditing && editor ? (
                    <ComputerEditor reservedNames={deviceId === "" ? macosComputers.map(({ name }) => name) : undefined} key={`${editor.draft.id}:${editorResetToken}`} saving={committing} blockedReason={saveBlockedReason} editorHeader={<>{osField}{devices ? <label className="grid gap-1 text-[11px] text-muted-foreground">Run on<select aria-label="Run on" className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground" value={deviceId} disabled={Boolean(editor.originalID) || committing} onChange={event => setDeviceId(event.target.value)}><option value="">This device</option>{devices.map(device => <option key={device.id} value={device.id} disabled={!device.connected}>{device.name}{!device.connected ? " (offline)" : ""}</option>)}</select></label> : undefined}</>} focusRequest={editorFocusRequest} capacity={getDeviceCapacity?.(deviceId)} deviceName={devices?.find(device => device.id === deviceId)?.name} deviceId={deviceId} created={Boolean(editor.originalID && isComputerCreated?.(configuration))} running={Boolean(editor.originalID && isComputerRunning?.(configuration))} editor={editor} baselineComputer={editorBaseline ?? undefined} conflict={editorConflict} review={editorReview} configurations={getDeviceId ? configurations.filter(configuration => (getDeviceId(configuration) ?? "") === deviceId) : configurations} onCancel={() => setEditor(null)} onSave={save} onDraftChange={(draft) => setEditor({ ...editor, draft })} onReview={reviewConflict} onDiscard={() => setEditor(null)} />
                  ) : (
                    <ComputerListRow
                      name={configuration.name}
                      os="linux"
                      onOpen={onOpenComputer && presentation?.openable !== false ? () => onOpenComputer(configuration) : undefined}
                      remote={Boolean(getDeviceId?.(configuration))}
                      kindBadge={presentation?.kindBadge}
                      badge={presentation?.badge}
                      iconState={presentation?.iconState}
                      icon={presentation?.icon}
                      tone={presentation?.tone}
                      detail={presentation?.detail ?? computerSummary(configuration)}
                      detailClassName={presentation?.detailClassName}
                      leading={!reorderable(configuration) ? <span aria-hidden="true" className="size-7 shrink-0" /> : <span
                        role="button"
                        tabIndex={reorderDisabled ? -1 : 0}
                        draggable={!reorderDisabled}
                        aria-label={`Reorder ${configuration.name}`}
                        aria-describedby={reorderHelpId}
                        aria-disabled={reorderDisabled || undefined}
                        className="grid size-7 shrink-0 cursor-grab place-items-center rounded-md text-muted-foreground outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 active:cursor-grabbing aria-disabled:cursor-default aria-disabled:opacity-40"
                        onKeyDown={(event) => { if (!reorderDisabled) handleReorderKey(event, configuration) }}
                        onDragStart={(event) => {
                          if (reorderDisabled) { event.preventDefault(); return }
                          beginOperation()
                          captureBaseline()
                          setDraggedID(configuration.id)
                          event.dataTransfer.effectAllowed = "move"
                          event.dataTransfer.setData("text/plain", configuration.id)
                        }}
                        onDragEnd={() => setDraggedID(null)}
                      >
                        <GripVertical className="size-4" aria-hidden="true" />
                      </span>}
                      actions={presentation?.actions || presentation?.menuActions ? <>{presentation?.actions}{presentation?.menuActions && <ActionsMenu ref={node => { if (node) editorTriggers.current.set(configuration.id, node); else editorTriggers.current.delete(configuration.id) }} label={`More actions for ${configuration.name}`} popovers={{
                        ...presentation.popovers,
                        delete: close => <DeleteComputerBody
                          displayName={deletionName}
                          details={presentation.deleteDetails}
                          onClose={close}
                          onDelete={() => deleteWithNotice(configuration)}
                        />,
                      }} items={[
                        ...presentation.menuActions,
                        // The menu stays open to navigation while work runs; its items that
                        // change the computer follow the row's interaction lock.
                        ...computerEditMenu({
                          configuration,
                          displayName: deletionName,
                          disabled: rowInteractionsDisabled,
                          busyReason,
                          created: Boolean(isComputerCreated?.(configuration)),
                          running: runningVM,
                          separatorBefore: presentation.menuActions.length > 0,
                          onEdit: () => startEdit(configuration),
                          onDuplicate: () => startDuplicate(configuration),
                          onAddDesktop: (vm) => {
                            beginOperation()
                            captureBaseline()
                            void save({ ...vm, desktop: { startWithComputer: true } }, configuration.id, getDeviceId?.(configuration) ?? "")
                          },
                        }),
                      ]} />}</> : undefined}
                      actionsClassName={presentation?.actionsClassName}
                      hoverActions={presentation?.suppressInteractions || presentation?.menuActions ? undefined : <>
                        <ComputerAction ref={node => { if (node) editorTriggers.current.set(configuration.id, node); else editorTriggers.current.delete(configuration.id) }} label={`Edit ${configuration.name}`} tooltip={busyReason} disabled={interactionDisabled || Boolean(busyReason)} onClick={() => startEdit(configuration)}><Pencil /></ComputerAction>
                        <ComputerAction tooltip="Create a new empty computer with the same settings." label={`Duplicate settings for ${configuration.name}`} disabled={interactionDisabled} onClick={() => startDuplicate(configuration)}>
                          <CopyPlus />
                        </ComputerAction>
                        <ConfirmPopover align="end" tone="destructive" title={deleteComputerTitle(deletionName)} description={deleteComputerDescription()} confirmLabel="Delete permanently" tooltip={deleteTooltip ?? `Delete ${deletionName}`} onConfirm={() => remove(configuration)}>
                          <Button type="button" variant="ghost" size="icon-xs" aria-label={`Delete ${deletionName}`} disabled={interactionDisabled || runningVM || Boolean(busyReason)}>
                            <Trash2 />
                          </Button>
                        </ConfirmPopover>
                      </>}
                    />
                  )}
                  {!isEditing && presentation?.expandedContent}
                </ComputerListItem>
              )
            })}
            {macosComputers.map(computer => <MacosComputerRow key={computer.id} computer={computer} store={macos!.store} />)}
        </ComputerList>
        {footer && <div className="mt-3 shrink-0">{footer}</div>}
        <p id={reorderHelpId} className="sr-only">Use the Up and Down arrow keys to reorder.</p>
        <p className="sr-only" aria-live="polite">{announcement}</p>
      </div>
    </>
  )
}
