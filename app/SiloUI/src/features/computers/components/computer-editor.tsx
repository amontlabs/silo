import { parseRemoteComputerTarget } from "@/features/application/model/connections"
import { useEffect, useId, useRef, useState, type ReactNode } from "react"
import { Monitor, Square } from "lucide-react"

import { InlineConfirmation } from "@/components/inline-confirmation"
import { InlineAlert } from "@/components/inline-alert"
import { restoreFocus } from "@/lib/focus"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { Checkbox } from "@/components/ui/checkbox"
import { Switch } from "@/components/ui/switch"
import { Input } from "@/components/ui/input"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import {
  computerCapacityError,
  supportedCPUs,
  supportedMemoryGiB,
  supportedStorageGiB,
  validateComputer,
  type ComputerValidationErrors,
} from "@/features/onboarding/model/computer-configuration"
import { divergentComputerFields, sameComputerConfiguration } from "@/features/application/model/computer-change"
import { useChatGptApp, useComputerUseBridge } from "@/desktop/computer-use-bridge"
import type { ComputerEditorDraft } from "@/features/onboarding/model/onboarding-draft"
import { computerFieldLabel, type ComputerReview } from "@/features/computers/model/computer-review"
import { parseWholeNumber, presetsWithin, resourceFields, resourceMaximums, runtimeLimits, validateComputerResources, type DeviceCapacity } from "@/features/computers/model/computer-limits"

function SelectField({ label, value, values, suffix, max, error, readOnly = false, custom = false, onChange }: {
  label: string
  value: number
  values: readonly number[]
  suffix: string
  /** The largest custom value the runtime accepts for this field. */
  max: number
  readOnly?: boolean
  custom?: boolean
  error?: string
  onChange: (value: number) => void
}) {
  const [customSelected, setCustomSelected] = useState(!values.includes(value))
  const isCustom = custom && (customSelected || !values.includes(value))
  // The custom input keeps the user's text ("1.5", "1e3", "") so it can be corrected;
  // the draft only receives whole numbers, and anything else fails validation.
  const [customText, setCustomText] = useState(value ? String(value) : "")
  const errorId = useId()
  const describedBy = error ? errorId : undefined
  const field = (
    <div className="grid min-w-0 gap-1 text-caption font-medium text-muted-foreground">
      {label}
      <select
        disabled={readOnly}
        aria-label={label}
        // With a custom value, the number input holds it and takes focus on failed validation.
        aria-invalid={Boolean(error) && !isCustom}
        aria-describedby={describedBy}
        className="h-8 min-w-0 rounded-lg border border-input bg-background px-2 text-xs text-foreground disabled:cursor-default disabled:opacity-60 outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 aria-invalid:border-destructive"
        value={isCustom ? "custom" : value}
        onChange={(event) => {
          const selected = event.target.value
          setCustomSelected(selected === "custom")
          if (selected === "custom") setCustomText(value ? String(value) : "")
          else { setCustomText(selected); onChange(Number(selected)) }
        }}
      >
        {values.map((option) => <option key={option} value={option}>{option} {suffix}</option>)}
        {custom && <option value="custom">Custom…</option>}
      </select>
      {isCustom && <Input technical
        type="number" inputMode="numeric" disabled={readOnly} min={1} max={max} step={1}
        aria-label={`${label} custom (${suffix === "CPUs" ? "CPUs" : "GiB"})`}
        aria-invalid={Boolean(error)}
        aria-describedby={describedBy}
        value={customText}
        onChange={(event) => {
          setCustomText(event.target.value)
          onChange(parseWholeNumber(event.target.value))
        }}
      />}
      {error && <span id={errorId} className="text-destructive">{error}</span>}
    </div>
  )
  return readOnly ? (
    <TooltipProvider><Tooltip><TooltipTrigger asChild><div role="group" tabIndex={0} aria-label={`${label}: ${value} ${suffix}, read-only`} className="rounded-md focus-ring">{field}</div></TooltipTrigger>
      <TooltipContent>Disk size is read-only.</TooltipContent>
    </Tooltip></TooltipProvider>
  ) : field
}

function TextField({ label, value, error, hint, firstField = false, inputRef, ...props }: {
  label: string
  value: string
  error?: string
  /** Guidance shown under the field while it has no error. */
  hint?: string
  firstField?: boolean
  inputRef?: React.RefObject<HTMLInputElement | null>
} & Omit<React.ComponentProps<typeof Input>, "value" | "aria-label">) {
  const errorId = useId()
  const hintId = useId()
  return (
    <label className="grid min-w-0 gap-1 text-caption font-medium text-muted-foreground">
      {label}
      <Input technical ref={firstField ? inputRef : undefined} aria-label={label} aria-invalid={Boolean(error)} aria-describedby={error ? errorId : hint ? hintId : undefined} value={value} {...props} />
      {error ? <span id={errorId} className="text-destructive">{error}</span> : hint && <span id={hintId} className="font-normal">{hint}</span>}
    </label>
  )
}

export function ComputerEditor({ saving, blockedReason, editorHeader, editor, focusRequest, configurations, baselineComputer, conflict = false, review, onCancel, onSave, onDraftChange, onReview, onDiscard, created, running, capacity, deviceName, deviceId }: {
  saving?: boolean
  /** Why Save is unavailable right now (another change locks editing); the draft is kept. */
  blockedReason?: string
  editorHeader?: ReactNode
  editor: ComputerEditorDraft
  focusRequest: number
  created: boolean
  running: boolean
  /** The CPUs and memory of the device the computer runs on, when known. */
  capacity?: DeviceCapacity
  /** That device's name for messages; defaults to "This device". */
  deviceName?: string
  /** The device id of the other device a new computer will run on; empty or omitted is this one. */
  deviceId?: string
  /** The device the computer will run on; empty or omitted is this one. */
  configurations: readonly SetupComputerConfiguration[]
  /** The computer's saved configuration when this editor opened, for divergence detection. */
  baselineComputer?: SetupComputerConfiguration
  /** A save was rejected because the computer changed while the edit waited. */
  conflict?: boolean
  /** After "Review changes": the draft was rebased onto the latest settings, with these differences. */
  review?: ComputerReview | null
  onCancel: () => void
  onSave: (configuration: SetupComputerConfiguration) => void
  onDraftChange: (draft: SetupComputerConfiguration) => void
  onReview?: () => void
  onDiscard?: () => void
}) {
  const [draft, setDraft] = useState(editor.draft)
  const [errors, setErrors] = useState<ComputerValidationErrors>({})
  const firstField = useRef<HTMLInputElement>(null)
  const container = useRef<HTMLFormElement>(null)
  const blockedReasonId = useId()
  // Bumped by each failed Save so focus moves to the first invalid field once it renders.
  const [failedValidation, setFailedValidation] = useState(0)
  const original = configurations.find(configuration => configuration.id === editor.originalID)
  // Detect that the committed computer changed under the open editor. `baselineComputer` is only
  // supplied for edits backed by a live source (not onboarding drafts), so these notices
  // stay quiet there. A missing live configuration for an edit means it was deleted elsewhere.
  const deletedElsewhere = Boolean(editor.originalID) && baselineComputer !== undefined && !original
  const divergent = Boolean(baselineComputer && original && !sameComputerConfiguration(baselineComputer, original))
  const changedFields = divergent && baselineComputer && original ? divergentComputerFields(baselineComputer, original) : []
  // A computer whose desktop is built into its image always starts it; only older computers are configured here.
  const builtInDesktop = created && original?.desktop?.builtIn === true
  const computerUse = useComputerUseBridge()
  // A new computer gets the built-in desktop only if the device it runs on can provide it.
  // This device follows the build; another device is asked, because it may run an
  // older Silo (and guest image) without computer use.
  const remoteOwner = !created && deviceId ? deviceId : undefined
  const ownerApp = useChatGptApp(remoteOwner && computerUse ? computerUse.chatGptFor(remoteOwner) : undefined)
  const ownerName = deviceName ?? "that device"
  const newVmSupport: "yes" | "no" | "checking" = !computerUse ? "no"
    : !remoteOwner ? "yes"
    : ownerApp.status ? (ownerApp.status.state === "unknown" ? "no" : "yes")
    : ownerApp.loadError ? "no" : "checking"
  const builtInNewVm = !created && newVmSupport === "yes"
  const startsWithComputer = draft.desktop?.startWithComputer === false
  // Computer use needs the session running: a new built-in computer always starts it, so a
  // duplicate or saved draft that chose to start it by hand is corrected.
  useEffect(() => {
    if (builtInNewVm && startsWithComputer) {
      const next = { ...draft, desktop: { startWithComputer: true } } as SetupComputerConfiguration
      setDraft(next)
      onDraftChange(next)
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [builtInNewVm, startsWithComputer])
  const desktopInstalled = created && Boolean(original?.desktop)
  const desktopOnlyChange = Boolean(original)
    && JSON.stringify(original?.desktop) !== JSON.stringify(draft.desktop)
    && JSON.stringify({ ...original, desktop: undefined }) === JSON.stringify({ ...draft, desktop: undefined })
  const requiresStop = running && !desktopOnlyChange
  const [confirmingStop, setConfirmingStop] = useState(false)
  // The confirmation disappears by itself if the computer stops elsewhere, a save starts, or
  // Save becomes blocked while it is shown.
  const stopPending = confirmingStop && requiresStop && !saving && !blockedReason && !deletedElsewhere
  if (confirmingStop && !stopPending) setConfirmingStop(false)
  const stopTarget = `${draft.name}${deviceName ? ` on ${deviceName}` : ""}`
  const cancelStop = useRef<HTMLButtonElement>(null)
  const saveButton = useRef<HTMLButtonElement>(null)
  const returnFocusToSave = useRef(false)
  useEffect(() => {
    if (stopPending) cancelStop.current?.focus()
    else if (returnFocusToSave.current) { returnFocusToSave.current = false; restoreFocus(saveButton.current) }
  }, [stopPending])
  function dismissStop() {
    returnFocusToSave.current = true
    setConfirmingStop(false)
  }
  // Offer only what the device can run; the runtime rejects ceilings above it.
  const maximums = resourceMaximums(capacity)
  const cpuPresets = presetsWithin(supportedCPUs, capacity ? maximums.cpus : undefined)
  const memoryPresets = presetsWithin(supportedMemoryGiB, capacity ? maximums.memoryGiB : undefined)

  useEffect(() => {
    function focusFirstField() {
      firstField.current?.focus()
      firstField.current?.scrollIntoView?.({ block: "nearest" })
    }
    focusFirstField()
    // A closing menu can restore focus after the editor mounts.
    const frame = requestAnimationFrame(() => {
      const active = document.activeElement
      if (active === document.body || active?.getAttribute("aria-haspopup") === "menu") focusFirstField()
    })
    return () => cancelAnimationFrame(frame)
  }, [focusRequest])

  useEffect(() => {
    if (!failedValidation) return
    container.current?.querySelector<HTMLElement>("[aria-invalid='true']:not(:disabled)")?.focus()
  }, [failedValidation])

  function update(changes: Partial<SetupComputerConfiguration>) {
    const next = { ...draft, ...changes } as SetupComputerConfiguration
    setDraft(next)
    onDraftChange(next)
    setErrors({})
    setConfirmingStop(false)
  }

  function save(stopConfirmed = false) {
    const nativeId = (id: string) => parseRemoteComputerTarget(id)?.computerId ?? id
    const nextErrors = validateComputer({ ...draft, id: nativeId(draft.id) }, configurations.map(configuration => ({ ...configuration, id: nativeId(configuration.id) })), editor.originalID ? nativeId(editor.originalID) : undefined)
    // Resource fields get readable range messages instead of the contract schema's.
    for (const field of resourceFields) delete nextErrors[field]
    Object.assign(nextErrors, validateComputerResources(draft, capacity, deviceName))
    const capacityError = computerCapacityError(configurations.length, editor.originalID)
    if (capacityError) nextErrors.form = capacityError
    setErrors(nextErrors)
    if (Object.keys(nextErrors).length > 0) {
      setConfirmingStop(false)
      setFailedValidation(count => count + 1)
    }
    // Stopping a running computer is always confirmed first (decision 8).
    else if (requiresStop && !stopConfirmed) setConfirmingStop(true)
    else onSave(builtInNewVm && startsWithComputer ? { ...draft, desktop: { startWithComputer: true } } : draft)
  }

  const initialDraft = useRef(editor.draft)
  // Escape cancels only an untouched editor, so a keystroke never discards typed changes.
  function cancelOnEscape(event: React.KeyboardEvent) {
    if (event.key !== "Escape" || event.defaultPrevented || event.nativeEvent.isComposing || saving || stopPending) return
    if (!sameComputerConfiguration(initialDraft.current, draft)) return
    event.preventDefault()
    onCancel()
  }

  return (
    <form ref={container} noValidate className="grid min-w-0 gap-3 p-3" data-testid={`computer-editor-${draft.id}`} onSubmit={(event) => { event.preventDefault(); if (!saving && !stopPending && !deletedElsewhere && !blockedReason) save() }} onKeyDown={cancelOnEscape}>
      <div className="flex min-w-0 items-center gap-2">
        <Monitor className="size-4 shrink-0" aria-hidden="true" />
        <span className="min-w-0 flex-1 text-xs font-semibold">Computer details</span>
      </div>

      {editorHeader}
      {deletedElsewhere ? (
        <InlineAlert>This computer no longer exists.</InlineAlert>
      ) : conflict ? (
        <InlineAlert>
          <p>This computer changed since you opened it.</p>
          <div className="flex justify-end gap-2">
            <Button type="button" size="xs" variant="outline" disabled={saving} onClick={onDiscard}>Discard my edits</Button>
            <Button type="button" size="xs" disabled={saving} onClick={onReview}>Review changes</Button>
          </div>
        </InlineAlert>
      ) : divergent ? (
        <InlineAlert tone="warning" role="status">
          This computer was changed elsewhere.{changedFields.length > 0 ? ` Updated: ${changedFields.map(computerFieldLabel).join(", ")}.` : ""}
        </InlineAlert>
      ) : review ? (
        <InlineAlert tone="warning" role="status" aria-label="Review changes" className="gap-1">
          <p>Your edits are kept on top of the latest settings.</p>
          {review.conflicts.length > 0 && <>
            <p>Also changed elsewhere:</p>
            <ul className="grid gap-0.5 pl-3">
              {review.conflicts.map(conflict => <li key={conflict.field} className="list-disc">{conflict.label}: yours {conflict.mine}, elsewhere {conflict.theirs}</li>)}
            </ul>
          </>}
          {review.adopted.length > 0 && <p>Updated from elsewhere: {review.adopted.join(", ")}.</p>}
          <p>Save to apply your edits, or Cancel to keep the latest settings.</p>
        </InlineAlert>
      ) : null}
      {/* Lock every field while saving so edits typed after Save aren't silently discarded. */}
      <fieldset disabled={saving} className="m-0 grid min-w-0 gap-3 border-0 p-0">
      <TextField
        firstField
        inputRef={firstField}
        label="Computer name"
        value={draft.name}
        readOnly={created}
        className={created ? "opacity-60" : undefined}
        error={errors.name}
        hint={created ? undefined : "1–32 lowercase letters, numbers, or hyphens, starting with a letter."}
        autoComplete="off"
        maxLength={32}
        onChange={(event) => update({ name: event.target.value })}
      />

      {created && <p className="text-caption text-muted-foreground">Existing computers cannot be renamed or have their disks resized. To use a different disk size, create a new computer and transfer your data.</p>}
      {editor.displayAfterID && <p className="text-caption text-muted-foreground">Creates a new empty computer with the same settings. Files are not included.</p>}

      <div className="grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-2">
        <p className="col-span-full text-caption text-muted-foreground">"At start" values are what the computer begins with; the maximums are the most it can use. The Workspace disk holds /workspace; the Runtime disk holds the operating system and installed applications.</p>
        <SelectField custom label="CPUs at start" value={draft.cpus} values={cpuPresets} max={maximums.cpus} suffix="CPUs" error={errors.cpus} onChange={(cpus) => update({ cpus } as Partial<SetupComputerConfiguration>)} />
        <SelectField custom label="Maximum CPUs" value={draft.maxCPUs} values={cpuPresets} max={maximums.cpus} suffix="CPUs" error={errors.maxCPUs} onChange={(maxCPUs) => update({ maxCPUs } as Partial<SetupComputerConfiguration>)} />
        <SelectField custom label="Memory at start" value={draft.memoryGiB} values={memoryPresets} max={maximums.memoryGiB} suffix="GiB" error={errors.memoryGiB} onChange={(memoryGiB) => update({ memoryGiB } as Partial<SetupComputerConfiguration>)} />
        <SelectField custom label="Maximum memory" value={draft.maxMemoryGiB} values={memoryPresets} max={maximums.memoryGiB} suffix="GiB" error={errors.maxMemoryGiB} onChange={(maxMemoryGiB) => update({ maxMemoryGiB } as Partial<SetupComputerConfiguration>)} />
        <SelectField custom readOnly={created} label="Workspace disk" value={draft.workspaceStorageGiB} values={supportedStorageGiB} max={runtimeLimits.storageGiB} suffix="GiB" error={errors.workspaceStorageGiB} onChange={(workspaceStorageGiB) => update({ workspaceStorageGiB } as Partial<SetupComputerConfiguration>)} />
        <SelectField custom readOnly={created} label="Runtime disk" value={draft.runtimeStorageGiB} values={supportedStorageGiB} max={runtimeLimits.storageGiB} suffix="GiB" error={errors.runtimeStorageGiB} onChange={(runtimeStorageGiB) => update({ runtimeStorageGiB } as Partial<SetupComputerConfiguration>)} />
      </div>

      {!builtInDesktop && !builtInNewVm && <section aria-label="Linux desktop" className="grid gap-2 border-t border-border pt-3">
        {!created && computerUse && remoteOwner && (newVmSupport === "no"
          ? <p className="text-caption text-muted-foreground">Update Silo on {ownerName} for built-in computer use. Until then, the optional Linux desktop is available.</p>
          : newVmSupport === "checking" ? <p role="status" className="text-caption text-muted-foreground">Checking {ownerName}…</p> : null)}
        {desktopInstalled ? <label className="flex items-center justify-between gap-3 text-xs">
          <span>Start desktop with computer<span className="mt-1 block text-caption text-muted-foreground">When off, start the desktop from its viewer.</span></span>
          <Switch aria-label="Start desktop with computer" checked={draft.desktop?.startWithComputer ?? true} disabled={saving} onCheckedChange={startWithComputer => update({ desktop: { startWithComputer } })} />
        </label> : created ? <div className="flex items-center justify-between gap-3">
          <div className="text-xs">Linux desktop<p className="mt-1 text-caption text-muted-foreground">Use graphical applications in this computer.</p></div>
          {draft.desktop ? <span className="text-xs text-muted-foreground">Installs when you save</span> : <Button type="button" size="sm" variant="outline" disabled={saving} onClick={() => update({ desktop: { startWithComputer: true } })}>Add Linux desktop</Button>}
        </div> : <label className="flex items-start gap-2 text-xs">
          <Checkbox aria-label="Linux desktop" checked={Boolean(draft.desktop)} disabled={saving} onCheckedChange={checked => update({ desktop: checked === true ? { startWithComputer: true } : undefined })} />
          <span>Linux desktop<span className="mt-1 block text-caption text-muted-foreground">Run graphical applications. Starts with the computer.</span></span>
        </label>}
      </section>}
      </fieldset>

      <p role={saving ? "status" : undefined} aria-live="polite" aria-atomic="true" className="sr-only">{saving ? `Saving ${draft.name}…` : ""}</p>
      {blockedReason && !saving && <p id={blockedReasonId} role="status" className="text-right text-caption text-muted-foreground">{blockedReason}</p>}
      {stopPending ? <InlineConfirmation active onDismiss={dismissStop}>
        <div role="group" aria-label={`Stop ${stopTarget} and save?`} className="grid gap-2 rounded-md border border-border px-3 py-2">
          <p className="text-caption text-muted-foreground">Stop {stopTarget} and save? Running processes will be interrupted. The new settings apply when you start it again.</p>
          <div className="flex justify-end gap-1.5">
            <Button ref={cancelStop} type="button" variant="ghost" size="xs" onClick={dismissStop}>Cancel</Button>
            <Button type="button" variant="destructive" size="xs" onClick={() => { setConfirmingStop(false); save(true) }}><Square />Stop and save</Button>
          </div>
        </div>
      </InlineConfirmation> : <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" size="sm" disabled={saving} onClick={onCancel}>Cancel</Button>
        <Button ref={saveButton} type="submit" size="sm" disabled={saving || deletedElsewhere || Boolean(blockedReason)} aria-describedby={blockedReason && !saving ? blockedReasonId : undefined}>{!editor.originalID ? (saving ? "Creating…" : "Create") : saving ? "Saving…" : requiresStop ? "Stop and save…" : "Save"}</Button>
      </div>}
      {errors.form && <p className="text-xs text-destructive" role="alert">{errors.form}</p>}
    </form>
  )
}
