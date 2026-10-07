import { useEffect, useRef, useState, type ReactNode } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import {  } from "lucide-react"
import { Dialog } from "radix-ui"
import { useSettingsSelector } from "@/features/preferences/settings-store"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { restoreFocus } from "@/lib/focus"
import { cn } from "@/lib/utils"
import {
  cancellableRunning,
  emptyOperationQueue,
  operationQueueSchema,
  shutdownWaitingLabel,
  type OperationQueue,
} from "@/features/application/model/operation-queue"

/**
 * Keep the existing screen visible while the native owner finishes Quit.
 * `pendingWork` names frontend setup work Quit waits for (it is not in the queue).
 */
export function ShutdownBoundary({ children, compact = false, pendingWork }: { children: ReactNode; compact?: boolean; pendingWork?: string }) {
  const [quitting, setQuitting] = useState(false)
  const [queue, setQueue] = useState<OperationQueue>(emptyOperationQueue)
  const previousFocus = useRef<HTMLElement | null>(null)
  const reduceMotion = useSettingsSelector((view) => view.settings.reduceMotion)
  useEffect(() => {
    let disposed = false
    let receivedEvent = false
    let receivedSnapshot = false
    let unsubscribe: (() => void) | undefined
    let connecting = false
    const read = async () => {
      const active = await invoke<boolean>("read_shutdown_state")
      receivedSnapshot = typeof active === "boolean"
      if (!disposed && !receivedEvent && receivedSnapshot) setQuitting(active)
    }
    const connect = () => {
      if (disposed || connecting || (unsubscribe && (receivedSnapshot || receivedEvent))) return
      connecting = true
      const connection = unsubscribe ? read() : listen<boolean>("silo://shutdown-state-changed", ({ payload }) => {
        receivedEvent = true
        if (!disposed && typeof payload === "boolean") setQuitting(payload)
      }).then(async stop => {
        if (disposed) { stop(); return }
        unsubscribe = stop
        await read()
      })
      void connection.catch(error => console.error("Silo shutdown status:", error))
        .finally(() => { connecting = false })
    }
    window.addEventListener("focus", connect)
    connect()
    return () => { disposed = true; unsubscribe?.(); window.removeEventListener("focus", connect) }
  }, [])
  // While quitting, follow the operation queue so the overlay can name the work
  // Quit is waiting for and offer to cancel it. The read command keeps working
  // during shutdown; queue changes (including cancelled waiters) re-fetch it.
  useEffect(() => {
    if (!quitting) return
    let disposed = false
    let unsubscribe: (() => void) | undefined
    // Reads are not ordered: only the latest request's answer may replace the queue.
    let sequence = 0
    const read = () => {
      const request = ++sequence
      return invoke<unknown>("read_operation_queue")
        .then(value => { if (!disposed && request === sequence) setQueue(operationQueueSchema.parse(value)) })
        .catch(error => console.error("Silo shutdown queue:", error))
    }
    void listen("silo://operation-queue-changed", () => { void read() }).then(stop => {
      if (disposed) { stop(); return }
      unsubscribe = stop
      void read()
    }).catch(error => {
      console.error("Silo shutdown queue:", error)
      if (!disposed) void read()
    })
    return () => { disposed = true; unsubscribe?.(); setQueue(emptyOperationQueue) }
  }, [quitting])
  const waitingLabel = shutdownWaitingLabel(queue)
  // The native shutdown entry names the computer it is stopping ("Stopping dev (1 of 2)").
  const stopping = queue.running.find(entry => entry.kind === "shutdown")?.label
  const cancellable = cancellableRunning(queue)
  const cancelAndQuit = () => { for (const entry of cancellable) void invoke("cancel_operation", { id: entry.id }).catch(error => console.error("Silo cancel operation:", error)) }
  // The status window measures this wrapper before resizing. Preserve its
  // natural height even while the window still has the previous page's size.
  return <div className={compact ? "flex shrink-0 flex-col" : "flex h-full min-h-0 flex-col"}>
    <div className="flex min-h-0 flex-1 flex-col" inert={quitting} aria-busy={quitting}>{children}</div>
    <Dialog.Root open={quitting}>
      <Dialog.Portal>
        <Dialog.Content
          className={cn("fixed inset-0 z-[100] flex flex-col items-center justify-center bg-background/70 text-foreground backdrop-blur-sm outline-none", compact && "rounded-xl")}
          onEscapeKeyDown={event => event.preventDefault()}
          onInteractOutside={event => event.preventDefault()}
          onOpenAutoFocus={() => { previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null }}
          onCloseAutoFocus={event => { event.preventDefault(); restoreFocus(previousFocus.current) }}
        >
          <Dialog.Title className="sr-only">Quitting Silo</Dialog.Title>
          <div role="status" className="flex max-w-sm flex-col items-center gap-3 px-6 text-center">
            <Spinner reduceMotion={reduceMotion} strokeWidth={1.5} className="size-6 text-muted-foreground" />
            <Dialog.Description className="text-[13px] font-medium text-foreground">{waitingLabel ?? pendingWork ?? (stopping ? `${stopping}…` : "Stopping local computers…")}</Dialog.Description>
            {waitingLabel && (cancellable.length > 0
              ? <Button type="button" size="sm" variant="outline" onClick={cancelAndQuit}>Cancel and quit</Button>
              : <p className="text-xs text-muted-foreground">Quit will finish once this work completes.</p>)}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  </div>
}
