import { useEffect, useRef, useState } from "react"
import { Check, KeyRound } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { dismissOperationToast, showActionFailure } from "@/lib/operation-toast"
import { restoreFocus } from "@/lib/focus"
import type { ApplicationSource } from "@/features/application/model/application-source"

const removalFailureToastId = "action-failure:Could not remove token"

export function PersonalTokenConnection({ status, onSave, onRemove }: {
  status?: ApplicationSource["github"]["personalToken"]
  onSave?: (token: string) => Promise<void>
  onRemove?: () => Promise<void>
}) {
  const [editing, setEditing] = useState(false)
  const [token, setToken] = useState("")
  const [busy, setBusy] = useState(false)
  const tokenButton = useRef<HTMLButtonElement>(null)
  const shouldRestoreFocus = useRef(false)
  const operation = useRef({ pending: false, generation: 0, mounted: false, removalFailureShown: false })
  useEffect(() => {
    const state = operation.current
    state.mounted = true
    return () => {
      state.mounted = false
      state.generation++
      if (state.removalFailureShown) dismissOperationToast(removalFailureToastId)
    }
  }, [])
  useEffect(() => {
    if (!editing && !busy && shouldRestoreFocus.current) {
      shouldRestoreFocus.current = false
      restoreFocus(tokenButton.current)
    }
  }, [editing, busy])
  function closeEditor() {
    shouldRestoreFocus.current = editing
    setEditing(false)
    setToken("")
  }
  function dismissRemovalFailure() {
    if (!operation.current.removalFailureShown) return
    operation.current.removalFailureShown = false
    dismissOperationToast(removalFailureToastId)
  }
  const connected = status?.state === "connected"
  async function save() {
    if (!onSave || !token.trim() || operation.current.pending || !operation.current.mounted) return
    operation.current.pending = true
    operation.current.generation++
    dismissRemovalFailure()
    const value = token.trim()
    setToken("")
    setBusy(true)
    try { await onSave(value); if (operation.current.mounted) closeEditor() }
    catch { if (operation.current.mounted) showActionFailure("Could not connect token", "Check its validity, your connection, and credential-store access.", undefined, { native: false }) }
    finally { operation.current.pending = false; if (operation.current.mounted) setBusy(false) }
  }
  async function remove(generation = operation.current.generation) {
    if (!onRemove || operation.current.pending || !operation.current.mounted || generation !== operation.current.generation) return
    operation.current.pending = true
    const removalGeneration = ++operation.current.generation
    dismissRemovalFailure()
    setBusy(true)
    try { await onRemove(); if (operation.current.mounted) closeEditor() }
    catch {
      if (operation.current.mounted) {
        operation.current.removalFailureShown = true
        showActionFailure("Could not remove token", "Check credential-store access and try again.", () => void remove(removalGeneration), { native: false })
      }
    }
    finally { operation.current.pending = false; if (operation.current.mounted) setBusy(false) }
  }
  return <ListCard className="shrink-0">
    <ListRow icon={<ListRowIcon>{busy ? <Spinner /> : connected ? <Check className="size-3.5 text-success" /> : <KeyRound className="size-3.5" />}</ListRowIcon>}
      title={<h3 className="text-sm">{connected ? `Token connected as @${status.account}` : "Personal access token"}</h3>}
      detail={status?.message ?? (connected ? "Available to computers that select Use token." : "Connect a token with the GitHub permissions you choose.")}
      actions={<div className="flex gap-1">
        <Button ref={tokenButton} size="xs" variant="outline" disabled={busy || !onSave} onClick={() => { setEditing(true) }}>{status?.saved ? "Replace token" : "Add token"}</Button>
        {status?.saved && <Button size="xs" variant="ghost" disabled={busy || !onRemove} onClick={() => void remove()}>Remove token</Button>}
      </div>} />
    {editing && <form className="flex flex-wrap gap-2 border-t p-3" onSubmit={event => { event.preventDefault(); void save() }}>
      <Input technical autoFocus className="min-w-40 flex-1" type="password" aria-label="GitHub personal access token" autoComplete="off" spellCheck={false}
        value={token} disabled={busy} onChange={event => setToken(event.target.value)} placeholder="Paste your personal access token" />
      <Button size="sm" type="submit" disabled={busy || !token.trim()}>Connect token</Button>
      <Button size="sm" variant="ghost" type="button" disabled={busy} onClick={closeEditor}>Cancel</Button>
    </form>}
  </ListCard>
}
