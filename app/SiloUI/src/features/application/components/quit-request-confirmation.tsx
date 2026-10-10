import { useEffect, useRef, useState } from "react"
import { AlertDialog } from "radix-ui"

import type { QuitRequest } from "@/desktop/settings"
import { Button } from "@/components/ui/button"
import { restoreFocus } from "@/lib/focus"
import { quitConfirmationDetail } from "@/features/status-bar/quit-confirmation-model"

/** `connectQuitConfirmation` from `desktop/settings`: opts the window in and routes each Quit request to `ask`. */
export type ConnectQuitConfirmation = (ask: (request: QuitRequest) => Promise<boolean>) => Promise<() => void>

interface PendingQuit {
  names: string[]
  answer: (confirmed: boolean) => void
}

/**
 * The main window's Quit confirmation (decision 7). ⌘Q, the app and Dock menus, and the
 * backend's own Quit requests arrive here while local computers run; the tray asks inline
 * with the same words. "Quit and stop" lets Silo stop them and exit; Cancel, Escape, or this
 * UI going away keeps Silo open.
 */
export function QuitRequestConfirmation({ connect }: { connect?: ConnectQuitConfirmation }) {
  const [pending, setPending] = useState<PendingQuit | null>(null)
  const quitButton = useRef<HTMLButtonElement>(null)
  const previousFocus = useRef<HTMLElement | null>(null)

  useEffect(() => {
    if (!connect) return
    let disposed = false
    let stop: (() => void) | undefined
    const unanswered = new Set<(confirmed: boolean) => void>()
    const ask = (request: QuitRequest) => new Promise<boolean>((resolve) => {
      if (disposed) { resolve(false); return }
      const answer = (confirmed: boolean) => {
        if (!unanswered.delete(answer)) return
        setPending((current) => current?.answer === answer ? null : current)
        resolve(confirmed)
      }
      unanswered.add(answer)
      setPending({ names: request.computers, answer })
    })
    connect(ask)
      .then((unlisten) => { if (disposed) unlisten(); else stop = unlisten })
      .catch((error: unknown) => console.error("Silo quit confirmation:", error))
    return () => {
      disposed = true
      stop?.()
      for (const answer of [...unanswered]) answer(false)
    }
  }, [connect])

  const detail = pending?.names.length
    ? quitConfirmationDetail(pending.names)
    : "Silo could not check which computers are running. Quitting stops any computers running on this device."

  return <AlertDialog.Root open={pending !== null} onOpenChange={(open) => { if (!open) pending?.answer(false) }}>
    <AlertDialog.Portal>
      <AlertDialog.Overlay className="fixed inset-0 z-50 bg-black/20" />
      {/* Like the tray's inline confirmation, Return confirms. */}
      <AlertDialog.Content onOpenAutoFocus={(event) => {
        previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
        event.preventDefault()
        quitButton.current?.focus()
      }} onCloseAutoFocus={(event) => {
        event.preventDefault()
        restoreFocus(previousFocus.current)
      }} className="fixed top-1/2 left-1/2 z-50 grid w-[calc(100%-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 gap-2 rounded-xl border border-border bg-popover p-4 text-xs text-popover-foreground shadow-2xl outline-none">
        <AlertDialog.Title className="text-ui font-medium">Quit Silo?</AlertDialog.Title>
        <AlertDialog.Description className="text-muted-foreground">{detail}</AlertDialog.Description>
        <div className="mt-1 flex justify-end gap-2">
          <AlertDialog.Cancel asChild><Button type="button" variant="ghost" size="sm">Cancel</Button></AlertDialog.Cancel>
          <AlertDialog.Action asChild><Button ref={quitButton} type="button" variant="destructive" size="sm" onClick={() => pending?.answer(true)}>Quit and stop</Button></AlertDialog.Action>
        </div>
      </AlertDialog.Content>
    </AlertDialog.Portal>
  </AlertDialog.Root>
}
