import { useCallback, useEffect, useRef, useState, type ComponentType, type ReactNode } from "react"
import { invoke } from "@tauri-apps/api/core"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { CircleAlert, Maximize, Monitor } from "lucide-react"
import { parseLinuxDesktopState, type LinuxDesktopState, type DesktopAction } from "./linux-desktop-state"
import { Button } from "@/components/ui/button"
import { TooltipProvider } from "@/components/ui/tooltip"
import { ViewerClipboard } from "./viewer-clipboard"
import { DesktopActionsMenu, NativeDesktopActionsMenu, type DesktopMenuProps } from "./linux-desktop-menu"
import { ViewerTransferStatus, useViewerFileDrop } from "./viewer-file-drop"

// Guest pages draw inside Silo's window, so anything inside the frame,
// including dialogs that look like Silo's, comes from the computer (G-20).
const GUEST_CONTENT_NOTICE = "Everything inside the amber frame comes from the computer. Silo's own controls are only in this bar."

export function LinuxDesktopViewer({ name, state, busy, error, onAction, onRetry, onFullscreen, screenRef, lcuUpdated = false, MenuComponent = DesktopActionsMenu, transfer, clipboard }: {
  name: string
  state: LinuxDesktopState | null
  busy: boolean
  error: string | null
  onAction: (action: DesktopAction) => void
  onRetry: () => void
  onFullscreen: () => void
  screenRef?: React.RefObject<HTMLDivElement | null>
  lcuUpdated?: boolean
  MenuComponent?: ComponentType<DesktopMenuProps>
  /** File transfer status, shown in the toolbar. */
  transfer?: React.ReactNode
  clipboard?: ReactNode
}) {
  const [menuError, setMenuError] = useState<string | null>(null)
  const [confirm, setConfirm] = useState<"stop" | "restart" | null>(null)
  const running = state?.state === "running"
  const streamNeedsRecovery = running && state?.sessionState === "running"
    && (state.streamState === "failed" || state.streamState === "stopped")
  const streamStarting = running && state?.sessionState === "running" && state.streamState === "starting"
  const updateRequired = state?.state === "stopped" && state.updateRequired === true
  const updateAvailable = state?.installed && state.state === "stopped"
    && (state.updateRequired === true || state.backend === "kasm")
  // v4 computers report computer use as a unit; older ones report the legacy LCU fields.
  const computerUse = state?.computerUse
  // A failed ChatGPT download is the device's, and setting up the computer cannot fix it.
  const downloadFailed = computerUse?.state === "failed" && computerUse.cause === "app-download"
  const lcuStatus = computerUse ? null : state?.lcuState === "needs-runtime" ? "LCU requires the official ChatGPT app in this computer"
    : state?.lcuState === "not-installed" ? "LCU is not set up"
      : state?.lcuState === "installing" ? "Setting up LCU…"
        : state?.lcuState === "repair-required" || state?.lcuState === "failed" ? "LCU setup needs attention"
          : state?.lcuState === "ready" ? `LCU ready${state.lcuAgents?.length ? ` · ${state.lcuAgents.join(", ")}` : ""}`
            : null
  const problem = error ?? menuError
  const actionLabel = updateRequired ? "Update desktop" : state?.state === "computer-stopped" ? state.autoStart ? "Start computer" : "Start computer and desktop" : state?.state === "failed" ? "Restart desktop" : "Start desktop"
  const primaryAction: DesktopAction = updateRequired ? "update-streamer" : state?.state === "failed" ? "restart" : "start"
  return <TooltipProvider><main className="flex h-dvh min-h-0 flex-col bg-background text-foreground">
    <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-3">
      <Monitor aria-hidden="true" className="size-4" /><h1 className="min-w-0 flex-1 truncate text-xs font-medium">{name}</h1>
      {running && <span className="shrink-0 rounded-sm border border-amber-500 px-1.5 text-xs text-amber-700 dark:text-amber-400" title={GUEST_CONTENT_NOTICE}>Computer content</span>}
      {confirm ? <div role="alert" className="flex min-w-0 items-center gap-2 text-xs">
        <p className="truncate" title="This closes the desktop's graphical applications.">{confirm === "stop" ? "Stopping" : "Restarting"} the desktop closes its graphical applications.</p>
        <Button size="xs" variant="ghost" onClick={() => setConfirm(null)}>Cancel</Button>
        <Button size="xs" disabled={busy} onClick={() => { onAction(confirm); setConfirm(null) }}>{confirm === "stop" ? "Stop desktop" : "Restart desktop"}</Button>
      </div> : <>
        {problem && <div role="alert" className="flex min-w-0 items-center gap-1 text-xs text-destructive">
          <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" /><span className="truncate" title={problem ?? undefined}>{problem}</span>
          {error && <Button size="xs" variant="ghost" disabled={busy} onClick={onRetry}>Reconnect</Button>}
        </div>}
        {state?.installed && state.state !== "computer-stopped" && lcuStatus && <div className="flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
          <span role={state.lcuState === "installing" ? "status" : undefined} title={state.lcuReason ?? undefined} className="truncate">{lcuStatus}</span>
          {running && state.lcuState !== "ready" && state.lcuState !== "installing" && <Button size="xs" variant="ghost" disabled={busy} aria-label="Set up LCU" onClick={() => onAction("setup-lcu")}>Set up LCU</Button>}
        </div>}
        {state?.installed && state.state !== "computer-stopped" && computerUse?.state === "failed" && <div role="alert" className="flex min-w-0 items-center gap-1 text-xs text-destructive">
          <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" />
          <span className="truncate" title={computerUse.reason ?? undefined}>{downloadFailed ? "ChatGPT download failed. Retry from the computer's page." : "Computer use setup failed"}</span>
          {running && !downloadFailed && <Button size="xs" variant="ghost" disabled={busy} onClick={() => onAction("setup-computer-use")}>Try again</Button>}
        </div>}
        {streamNeedsRecovery && <div role="alert" className="flex min-w-0 items-center gap-1 text-xs text-destructive">
          <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" /><span className="truncate">Display disconnected</span>
          <Button size="xs" variant="ghost" disabled={busy} onClick={() => onAction("restart-streamer")}>Reconnect display</Button>
        </div>}
        {streamStarting && <span role="status" className="text-xs text-muted-foreground">Connecting display…</span>}
        {lcuUpdated && (computerUse ? computerUse.state === "ready" : state?.lcuState === "ready") && <span role="status" className="text-xs text-muted-foreground">{computerUse ? "Reconnect agent sessions to load computer use." : "Reconnect agent sessions to load LCU."}</span>}
      </>}
      {running && clipboard}
      {transfer}
      <Button variant="ghost" size="icon-xs" aria-label="Toggle fullscreen" onClick={onFullscreen}><Maximize /></Button>
      {running && <MenuComponent busy={busy} onSelect={action => { setMenuError(null); setConfirm(action) }} onError={setMenuError} />}
    </header>
    {running ? <section aria-label="Computer display" aria-description={GUEST_CONTENT_NOTICE} className="flex min-h-0 flex-1 bg-amber-500 p-1">
      {/* The guest webview covers only this element; the frame around it stays Silo's. */}
      <div ref={screenRef} className="min-h-0 flex-1 bg-background" aria-label="Linux desktop display" />
    </section> : <div className="grid min-h-0 flex-1 place-items-center p-6 text-center" aria-busy={busy}>
      <div className="grid max-w-sm justify-items-center gap-3">
        <Monitor aria-hidden="true" className="size-8 text-muted-foreground" />
        <p className="text-sm">{busy || state?.state === "starting" ? "Connecting to desktop…" : !state ? "Desktop unavailable" : state.state === "uninstalled" ? computerUse ? "Desktop unavailable" : "Desktop is not installed" : state.state === "failed" ? "Desktop needs attention" : state.state === "computer-stopped" ? "Computer is stopped" : "Desktop is stopped"}</p>
        {state && state.state !== "uninstalled" && state.state !== "starting" && <Button disabled={busy} size="sm" onClick={() => onAction(primaryAction)}>{actionLabel}</Button>}
        {updateAvailable && !updateRequired && <Button disabled={busy} size="sm" variant="ghost" onClick={() => onAction("update-streamer")}>Update desktop</Button>}
        {state?.state === "uninstalled" && !computerUse && <p className="text-xs text-muted-foreground">Choose Add Linux desktop in the computer’s actions menu.</p>}
      </div>
    </div>}
  </main></TooltipProvider>
}

export function NativeLinuxDesktopViewer({ computer, name }: { computer: string; name: string }) {
  const drop = useViewerFileDrop(computer)
  const [state, setState] = useState<LinuxDesktopState | null>(null)
  const [busy, setBusy] = useState(true)
  const [lcuUpdated, setLcuUpdated] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [connectionError, setConnectionError] = useState<string | null>(null)
  const [connection, setConnection] = useState(0)
  const screenRef = useRef<HTMLDivElement>(null)
  const refreshAttachment = useRef<(() => void) | null>(null)
  const operation = useRef(false)
  const polling = useRef(false)
  const revision = useRef(0)
  const failureDelay = useRef(0)
  const transportTail = useRef<Promise<void>>(Promise.resolve())
  const transport = useCallback((work: () => Promise<void>) => {
    const next = transportTail.current.catch(() => {}).then(work)
    transportTail.current = next
    return next
  }, [])
  const refresh = useCallback(async (checkAttachment = true) => {
    if (operation.current || polling.current) return
    polling.current = true
    const currentRevision = revision.current
    try {
      const result = parseLinuxDesktopState(await invoke("read_desktop_state", { computer }))
      if (currentRevision === revision.current) {
        failureDelay.current = 0
        setState(result)
        setError(null)
        // A host disconnect can retire the transport while its guest keeps running.
        if (checkAttachment && result.state === "running" && (result.streamState == null || result.streamState === "running")) refreshAttachment.current?.()
      }
    } catch (cause) {
      if (currentRevision === revision.current) {
        failureDelay.current = Math.min(Math.max(failureDelay.current, 5000) * 2, 30000)
        setError(String(cause))
      }
    }
    finally { polling.current = false; if (currentRevision === revision.current) setBusy(false) }
  }, [computer])
  useEffect(() => {
    let disposed = false
    let timer: number | undefined
    const update = () => {
      window.clearTimeout(timer)
      if (disposed || document.visibilityState === "hidden") return
      void refresh().finally(() => {
        if (disposed || document.visibilityState === "hidden") return
        window.clearTimeout(timer)
        timer = window.setTimeout(update, Math.max(5000, failureDelay.current))
      })
    }
    timer = window.setTimeout(update, 0)
    document.addEventListener("visibilitychange", update)
    return () => { disposed = true; window.clearTimeout(timer); document.removeEventListener("visibilitychange", update) }
  }, [refresh])
  // The backend connects only once the stream runs; legacy guests omit it.
  const streamReady = state?.state === "running" && (state.streamState == null || state.streamState === "running")
  useEffect(() => {
    if (!streamReady || !screenRef.current) {
      void transport(() => invoke("desktop_viewer_detach")).catch(cause => setError(String(cause)))
      return
    }
    const screen = screenRef.current
    let disposed = false
    let pending = false
    let dirty = false
    async function attach() {
      if (disposed) return
      if (pending) { dirty = true; return }
      const { x, y, width, height } = screen.getBoundingClientRect()
      const viewportHeight = window.innerHeight
      if (width <= 0 || height <= 0) return
      pending = true
      try { await transport(async () => { if (!disposed) await invoke("desktop_viewer_attach", { computer, x, y, width, height, viewportHeight }) }); if (!disposed) setConnectionError(null) }
      catch (cause) { if (!disposed) setConnectionError(String(cause)) }
      finally { pending = false; if (dirty) { dirty = false; void attach() } }
    }
    const updateBounds = () => { void attach() }
    refreshAttachment.current = updateBounds
    const observer = new ResizeObserver(updateBounds)
    observer.observe(screen)
    window.addEventListener("resize", updateBounds)
    void attach()
    return () => { disposed = true; refreshAttachment.current = null; observer.disconnect(); window.removeEventListener("resize", updateBounds); void transport(() => invoke("desktop_viewer_detach")).catch(() => {}) }
  }, [computer, streamReady, connection, transport])
  async function handleAction(action: DesktopAction) {
    if (operation.current) return
    operation.current = true
    revision.current += 1
    setBusy(true)
    const previous = state
    if (action === "setup-lcu") { setLcuUpdated(false); setState(current => current ? { ...current, lcuState: "installing" } : current) }
    if (action === "setup-computer-use") { setLcuUpdated(false); setState(current => current?.computerUse ? { ...current, computerUse: { ...current.computerUse, state: "installing", reason: null } } : current) }
    setError(null)
    try {
      const result = parseLinuxDesktopState(await invoke("desktop_action", { computer, action }))
      setState(result)
        if (action === "setup-lcu") setLcuUpdated(result.lcuState === "ready")
      if (action === "setup-computer-use") setLcuUpdated(result.computerUse?.state === "ready")
      if (action === "restart-streamer") {
        setConnectionError(null)
        setConnection(value => value + 1)
      }
    }
    catch (cause) { setError(String(cause)); if (action === "setup-lcu" || action === "setup-computer-use") setState(previous) }
    finally { operation.current = false; setBusy(false) }
  }
  return <LinuxDesktopViewer name={name} state={state} busy={busy} error={error ?? (streamReady ? connectionError : null)} screenRef={screenRef} lcuUpdated={lcuUpdated} MenuComponent={NativeDesktopActionsMenu} transfer={<ViewerTransferStatus drop={drop} />}
    clipboard={<ViewerClipboard computer={computer} name={name} needsUpdate={state?.updateRequired === true} />}
    onAction={action => { void handleAction(action) }}
    onRetry={() => { setConnection(value => value + 1); void refresh(false) }}
    onFullscreen={() => { const window = getCurrentWindow(); void window.isFullscreen().then(value => window.setFullscreen(!value)).catch(cause => setError(String(cause))) }} />
}
