import { errorMessage } from "@/lib/error-message"
import { useEffect, useRef, useState } from "react"

import type { ApplicationSecret, ApplicationSource, SecretConfigurationRequest } from "@/features/application/model/application-source"
import { restoreFocus } from "@/lib/focus"

const actionableFailures = new Set([
  "Cannot access secrets in the system credential store. Unlock it and retry.",
  "Secret settings are too large. Reduce assignments or allowed domains and retry. No settings were overwritten.",
  "A selected computer was removed while saving this secret. Select computers again and retry.",
])

function operationFailure(error: unknown, fallback: string) {
  const message = errorMessage(error)
  return actionableFailures.has(message) ? message : fallback
}

interface EditorState {
  secret?: ApplicationSecret
  /** Preselected computers when adding a new secret scoped to one computer. */
  initialComputers?: string[]
}

/** Shared state and operations for viewing, adding, editing, removing, and retrying secrets.
 * Both the full Secrets page and a computer's Secrets section drive identical row states from it. */
export function useSecretsManager({ source, onSaveSecret, onRemoveSecret, onRetrySecret }: {
  source: ApplicationSource
  onSaveSecret: (request: SecretConfigurationRequest) => Promise<void> | void
  onRemoveSecret: (id: string) => Promise<void> | void
  onRetrySecret?: (id: string) => Promise<void> | void
}) {
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string>()
  const [busy, setBusy] = useState<string | null>(null)
  const [operationError, setOperationError] = useState<{ id: string; message: string; action: (id: string) => Promise<void> | void } | null>(null)
  const shouldRestoreFocus = useRef(false)
  const editorTrigger = useRef<HTMLElement | null>(null)

  useEffect(() => {
    if (!editor && !saving && shouldRestoreFocus.current) {
      shouldRestoreFocus.current = false
      restoreFocus(editorTrigger.current)
    }
  }, [editor, saving])

  function openEditor(trigger: HTMLElement | null, options: EditorState = {}) {
    editorTrigger.current = trigger
    setSaveError(undefined)
    setEditor(options)
  }

  function closeEditor() {
    shouldRestoreFocus.current = true
    setEditor(null)
  }

  async function saveSecret(request: SecretConfigurationRequest) {
    setSaving(true)
    setSaveError(undefined)
    try {
      await onSaveSecret(request)
      closeEditor()
    } catch (error) {
      setSaveError(operationFailure(error, "Could not save this secret. Your changes are still here. Retry."))
    } finally {
      setSaving(false)
    }
  }

  async function runOperation(id: string, action: (id: string) => Promise<void> | void) {
    setBusy(id)
    setOperationError(null)
    try {
      await action(id)
    } catch (error) {
      setOperationError({ id, message: operationFailure(error, "Could not update this secret. Retry."), action })
    } finally {
      setBusy(null)
    }
  }

  function removeSecret(id: string) {
    void runOperation(id, onRemoveSecret)
  }

  return {
    source, onRetrySecret,
    editor, saving, saveError, busy, operationError,
    openEditor, closeEditor, saveSecret, runOperation, removeSecret,
  }
}

export type SecretsManager = ReturnType<typeof useSecretsManager>
