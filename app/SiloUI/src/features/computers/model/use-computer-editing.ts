import { useEffect, useEffectEvent, useRef, useState } from "react"

import { showActionFailure, showOperationNotice } from "@/lib/operation-toast"

import type { SetupComputerConfiguration } from "@/contracts/silo"
import {
  configurationRequest,
  duplicateComputer,
  newVirtualComputer,
} from "@/features/onboarding/model/computer-configuration"
import { isStaleConfigurationError } from "@/features/application/model/computer-change"
import type { ComputerEditorDraft } from "@/features/onboarding/model/onboarding-draft"
import { fitComputerToCapacity, type DeviceCapacity } from "@/features/computers/model/computer-limits"
import { rebaseComputerDraft, type ComputerReview } from "@/features/computers/model/computer-review"
import { useComputerEditorDrafts } from "@/features/computers/model/editor-drafts-context"

export const defaultSaveBlockedReason = "Saving is paused while another computer change is in progress or needs review."

export interface ComputerEditingOptions {
  configurations: readonly SetupComputerConfiguration[]
  getDeviceId?: (configuration: SetupComputerConfiguration) => string | undefined
  onCommitComputer?: (configuration: SetupComputerConfiguration, original: SetupComputerConfiguration | undefined, deviceId: string, baseline?: SetupComputerConfiguration[]) => Promise<void>
  onDeleteComputer?: (configuration: SetupComputerConfiguration, baseline?: SetupComputerConfiguration[]) => Promise<void>
  onConfigurationsChange: (configurations: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[]) => Promise<void> | void
  validateOperation?: (configuration: SetupComputerConfiguration, isNew: boolean, deviceId?: string) => string | undefined
  isComputerRunning?: (configuration: SetupComputerConfiguration) => boolean
  onEditorDraftChange?: (editor: ComputerEditorDraft | null) => void
  initialEditorDraft?: ComputerEditorDraft | null
  interactionDisabled?: boolean
  /** Why `interactionDisabled` blocks saving an open editor; a generic reason by default. */
  interactionDisabledReason?: string
  /** The capacity of a device ("" is this one), when known, so new computers fit it. */
  getDeviceCapacity?: (deviceId: string) => DeviceCapacity | undefined
  /** Why a computer cannot be edited or deleted now (it is starting or stopping), if so. */
  getConfigurationBusyReason?: (configuration: SetupComputerConfiguration) => string | undefined
  /**
   * Keeps an open editor under this key in the nearest `ComputerEditorDraftsProvider`, so
   * leaving the surface and coming back restores the unsaved edit. Ignored when
   * `initialEditorDraft` is given.
   */
  draftKey?: string
}

/**
 * Owns every piece of the computer editing flow — draft state, validation, stale-baseline
 * conflict detection and review, committing/deleting, and the Run-on device selection —
 * so the computer list and the computer detail page share exactly the same behaviour. The
 * caller renders `ComputerEditor` with the returned state and wires its handlers.
 */
export function useComputerEditing({
  configurations,
  getDeviceId,
  onCommitComputer,
  onDeleteComputer,
  onConfigurationsChange,
  validateOperation,
  isComputerRunning,
  onEditorDraftChange,
  initialEditorDraft = null,
  interactionDisabled = false,
  interactionDisabledReason = defaultSaveBlockedReason,
  getDeviceCapacity,
  getConfigurationBusyReason,
  draftKey,
}: ComputerEditingOptions) {
  // Restore an editor left open on this surface earlier (see editor-drafts-context.ts).
  const drafts = useComputerEditorDrafts()
  const [stored] = useState(() => !initialEditorDraft && draftKey ? drafts?.get(draftKey) : undefined)
  const [deviceId, setDeviceId] = useState(stored?.deviceId ?? "")
  const [committing, setCommitting] = useState(Boolean(stored?.pendingSave))
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])
  const disabled = interactionDisabled || committing
  const [editorFocusRequest, setEditorFocusRequest] = useState(0)
  const [editor, setEditorState] = useState<ComputerEditorDraft | null>(initialEditorDraft ?? stored?.editor ?? null)
  const busyReason = (configuration: SetupComputerConfiguration | undefined) => configuration ? getConfigurationBusyReason?.(configuration) : undefined
  // An editor can stay open while another change starts (or fails and awaits review), or
  // while its computer starts or stops: Save is then disabled with this reason instead of
  // silently doing nothing or being rejected.
  const saveBlockedReason = interactionDisabled
    ? interactionDisabledReason
    : editor?.originalID ? busyReason(configurations.find(({ id }) => id === editor.originalID)) : undefined
  // The saved configuration captured when the current operation began. Every local
  // save/delete/reorder carries it as the change's `expected` baseline, so a queued edit
  // applies to fresh state — or is rejected — instead of overwriting concurrent work.
  const baselineRef = useRef<SetupComputerConfiguration[] | null>(stored?.baseline ?? null)
  // The edited computer's baseline, plus editor conflict state, drive the in-editor notices.
  const [editorBaseline, setEditorBaseline] = useState<SetupComputerConfiguration | null>(stored?.editorBaseline ?? null)
  const [editorConflict, setEditorConflict] = useState(stored?.editorConflict ?? false)
  const [editorReview, setEditorReview] = useState<ComputerReview | null>(stored?.editorReview ?? null)
  useEffect(() => {
    if (!draftKey || !drafts) return
    if (editor) drafts.set(draftKey, { editor, editorBaseline, editorConflict, editorReview, baseline: baselineRef.current, deviceId, pendingSave: drafts.get(draftKey)?.pendingSave, macosForm: drafts.get(draftKey)?.macosForm, pendingMacosCreate: drafts.get(draftKey)?.pendingMacosCreate })
    else drafts.delete(draftKey)
  }, [drafts, draftKey, editor, editorBaseline, editorConflict, editorReview, deviceId])
  const [editorResetToken, setEditorResetToken] = useState(0)

  const completeRestoredSave = useEffectEvent(() => { setEditor(null); setCommitting(false) })
  // A restored editor observes the same save instead of starting another one.
  useEffect(() => {
    if (!stored?.pendingSave) return
    let current = true
    void stored.pendingSave.then(() => {
      if (current) completeRestoredSave()
    }, cause => {
      if (current) {
        if (isStaleConfigurationError(cause)) setEditorConflict(true)
        setCommitting(false)
      }
    })
    return () => { current = false }
  }, [stored])

  function captureBaseline() {
    baselineRef.current = structuredClone(configurations as SetupComputerConfiguration[])
  }

  // The editor as of the latest change, for rejections that settle after it closed or moved on.
  const editorRef = useRef(editor)
  function setEditor(next: ComputerEditorDraft | null) {
    editorRef.current = next
    setEditorState(next)
    onEditorDraftChange?.(next)
    if (!next) { setEditorConflict(false); setEditorBaseline(null); setEditorReview(null) }
  }

  /**
   * A stale-baseline rejection is shown in the open editor for that computer. With no such
   * editor (Add Linux desktop from the ⋯ menu, or a local save that closed its editor before
   * the rejection arrived), report it as a failure instead of dropping it.
   */
  function reportSaveFailure(cause: unknown, configuration?: Pick<SetupComputerConfiguration, "id" | "name">) {
    if (configuration && isStaleConfigurationError(cause) && draftKey && drafts) {
      const cached = drafts.get(draftKey)
      if (cached?.editor.originalID === configuration.id) drafts.set(draftKey, { ...cached, editorConflict: true })
    }
    if (mounted.current && configuration && isStaleConfigurationError(cause) && editorRef.current?.originalID === configuration.id) setEditorConflict(true)
    else showActionFailure(configuration ? `Could not save ${configuration.name}` : "Could not save changes", cause, undefined, { native: false })
  }

  function beginOperation() {
    setEditor(null)
  }

  function startEdit(configuration: SetupComputerConfiguration) {
    if (disabled) return
    const busy = busyReason(configuration)
    if (busy) { showActionFailure(`Could not edit ${configuration.name}`, busy, undefined, { native: false }); return }
    beginOperation()
    captureBaseline()
    setEditorBaseline(structuredClone(configuration))
    setDeviceId(getDeviceId?.(configuration) ?? "")
    setEditor({
      draft: structuredClone(configuration),
      originalID: configuration.id,
      insertAt: configurations.findIndex(({ id }) => id === configuration.id),
    })
  }

  function startAdd() {
    if (disabled) return
    beginOperation()
    captureBaseline()
    setDeviceId("")
    setEditor({
      // New computers start on this device, so fit the defaults to it.
      draft: fitComputerToCapacity(newVirtualComputer(configurations), getDeviceCapacity?.("")),
      insertAt: configurations.length,
    })
  }

  function startDuplicate(configuration: SetupComputerConfiguration) {
    if (disabled) return
    beginOperation()
    captureBaseline()
    const sourceIndex = configurations.findIndex(({ id }) => id === configuration.id)
    setDeviceId(getDeviceId?.(configuration) ?? "")
    setEditor({ draft: duplicateComputer(configuration, configurations), insertAt: sourceIndex + 1, displayAfterID: configuration.id })
  }

  // Restrict a captured baseline to the configurations this list actually commits (local vs a
  // single remote device), matching the list the save is derived against.
  function scopedBaseline(baseline = baselineRef.current, targetDeviceId = deviceId): SetupComputerConfiguration[] | undefined {
    if (!baseline) return undefined
    return getDeviceId ? baseline.filter(configuration => (getDeviceId(configuration) ?? "") === targetDeviceId) : baseline
  }

  /** Applies a whole-list change; returns its settlement (failures already reported) when asynchronous. */
  function dispatchChange(next: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[], saved?: Pick<SetupComputerConfiguration, "id" | "name">): Promise<void> | undefined {
    // Only pass a baseline when one was captured, keeping the no-baseline call shape
    // (onboarding drafts) exactly one argument.
    const outcome = baseline ? onConfigurationsChange(next, baseline) : onConfigurationsChange(next)
    if (outcome && typeof (outcome as Promise<void>).then === "function") {
      return (outcome as Promise<void>).catch((cause) => reportSaveFailure(cause, saved))
    }
    return undefined
  }

  async function save(configuration: SetupComputerConfiguration, originalID = editor?.originalID, targetDeviceId = deviceId) {
    if (committing) return
    // Also covers menu saves with no editor open (Add Linux desktop).
    const blockedReason = saveBlockedReason ?? (originalID ? busyReason(configurations.find(({ id }) => id === originalID)) : undefined)
    if (blockedReason) { showActionFailure(`Could not save ${configuration.name}`, blockedReason, undefined, { native: false }); return }
    const blocked = validateOperation?.(configuration, !originalID, targetDeviceId)
    if (blocked) { showActionFailure(`Could not save ${configuration.name}`, blocked, undefined, { native: false }); return }
    const baseline = baselineRef.current ?? undefined
    if (onCommitComputer) {
      setCommitting(true)
      let pendingSave: Promise<void> | undefined
      try {
        // The expected state is what the editor opened with, so a concurrent change is
        // rejected as stale instead of silently overwritten.
        const original = baseline?.find(item => item.id === originalID) ?? configurations.find(item => item.id === originalID)
        pendingSave = onCommitComputer(configuration, original, targetDeviceId, baseline)
        const cached = draftKey ? drafts?.get(draftKey) : undefined
        if (cached && draftKey && cached.editor === editorRef.current) drafts?.set(draftKey, { ...cached, pendingSave })
        await pendingSave
        // Cache settlement cannot depend on an effect in an editor that has left the page.
        if (draftKey && drafts?.get(draftKey)?.pendingSave === pendingSave) drafts.delete(draftKey)
        setEditor(null)
      } catch (cause) {
        // A stale-baseline rejection keeps the editor open with the user's edits so they
        // can review the latest values or discard; other failures surface as before.
        reportSaveFailure(cause, originalID ? { id: originalID, name: configuration.name } : configuration)
      }
      finally {
        const cached = draftKey ? drafts?.get(draftKey) : undefined
        if (cached && draftKey && cached.pendingSave === pendingSave) drafts?.set(draftKey, { ...cached, pendingSave: undefined })
        setCommitting(false)
      }
      return
    }
    const base = baseline ?? [...configurations]
    const updated = [...base]
    if (originalID) {
      const index = updated.findIndex(({ id }) => id === originalID)
      if (index < 0) return
      updated[index] = configuration
    } else {
      updated.splice(editor?.insertAt ?? updated.length, 0, configuration)
    }
    dispatchChange(configurationRequest(getDeviceId ? updated.filter(configuration => !getDeviceId(configuration)) : updated).computers, baseline ? scopedBaseline() : undefined, configuration)
    setEditor(null)
  }

  // "Review changes" after a stale rejection: rebase the user's edits onto the latest saved
  // configuration (instead of replacing them), re-baseline so the next Save applies to it,
  // and list what changed on both sides.
  function reviewConflict() {
    if (!editor?.originalID) return
    const latest = configurations.find(({ id }) => id === editor.originalID)
    if (!latest) { setEditor(null); return }
    const { draft, review } = rebaseComputerDraft(editorBaseline ?? latest, latest, editor.draft)
    captureBaseline()
    setEditorBaseline(structuredClone(latest))
    const next = { ...editor, draft }
    editorRef.current = next
    setEditorState(next)
    onEditorDraftChange?.(next)
    setEditorConflict(false)
    setEditorReview(review)
    setEditorResetToken(token => token + 1)
  }

  async function remove(configuration: SetupComputerConfiguration) {
    if (disabled || (isComputerRunning?.(configuration))) return
    const busy = busyReason(configuration)
    if (busy) { showActionFailure(`Could not delete ${configuration.name}`, busy, undefined, { native: false }); return }
    beginOperation()
    captureBaseline()
    const baseline = baselineRef.current ?? undefined
    if (onDeleteComputer) {
      setCommitting(true)
      try { await onDeleteComputer(baseline?.find(item => item.id === configuration.id) ?? configuration, baseline) }
      catch (cause) { showActionFailure(`Could not delete ${configuration.name}`, cause, undefined, { native: false }) }
      finally { setCommitting(false) }
      return
    }
    const base = baseline ?? configurations
    dispatchChange(configurationRequest(base.filter(({ id }) => id !== configuration.id)).computers, baseline ? scopedBaseline() : undefined)
  }

  // Delete a configuration without the list's confirmation — the detail page confirms
  // in its own dialog, so it captures a fresh baseline and awaits the deletion here, letting
  // failures propagate to the dialog instead of the inline notice.
  async function deleteComputerNow(configuration: SetupComputerConfiguration) {
    if (disabled) throw new Error(interactionDisabledReason)
    const blocked = validateOperation?.(configuration, false, getDeviceId?.(configuration) ?? "")
    if (blocked) throw new Error(blocked)
    const baseline = structuredClone(configurations as SetupComputerConfiguration[])
    if (onDeleteComputer) { await onDeleteComputer(configuration, baseline); return }
    const next = configurationRequest(baseline.filter(({ id }) => id !== configuration.id)).computers
    const outcome = onConfigurationsChange(next, scopedBaseline(baseline, getDeviceId?.(configuration) ?? ""))
    if (outcome) await outcome
  }

  /** Confirmed deletion (from the shared delete popover): deletes, then reports the outcome in a notification. */
  async function deleteWithNotice(configuration: SetupComputerConfiguration): Promise<boolean> {
    // The popover may have been opened while the computer was stopped; never delete a running computer.
    if (isComputerRunning?.(configuration)) {
      showActionFailure(`Could not delete ${configuration.name}`, "Stop the computer before deleting it.", undefined, { native: false })
      return false
    }
    const busy = busyReason(configuration)
    if (busy) {
      showActionFailure(`Could not delete ${configuration.name}`, busy, undefined, { native: false })
      return false
    }
    try {
      await deleteComputerNow(configuration)
      showOperationNotice(`computer-deleted:${configuration.id}`, `Deleted ${configuration.name}`)
      return true
    } catch (cause) {
      showActionFailure(`Could not delete ${configuration.name}`, cause, undefined, { native: false })
      return false
    }
  }

  return {
    deviceId, setDeviceId,
    committing,
    interactionDisabled: disabled,
    saveBlockedReason,
    editor, setEditor,
    editorBaseline, editorConflict, editorReview, editorResetToken,
    editorFocusRequest, setEditorFocusRequest,
    baselineRef,
    captureBaseline, beginOperation, scopedBaseline, dispatchChange,
    startEdit, startAdd, startDuplicate, save, remove, reviewConflict, deleteComputerNow, deleteWithNotice,
  }
}
