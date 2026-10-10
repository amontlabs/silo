import { useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { getCurrentWindow } from "@tauri-apps/api/window"
import RFB from "@novnc/novnc"
import { CircleAlert, Maximize, Monitor } from "lucide-react"
import { Button } from "@/components/ui/button"
import { TooltipProvider } from "@/components/ui/tooltip"
import {
  RECONNECT_MAX_ATTEMPTS, RESIZE_DEBOUNCE_MS, errorText, isTransientSessionError, reconnectDelay, resizeTarget, statusLabel,
  type MacosDisplayRoute, type MacosDisplaySession, type MacosRemoteStatus,
} from "./macos-remote-viewer-state"

type Listener = (event: CustomEvent) => void

const AUTH_FAILED_MESSAGE = "The computer rejected the sign-in for its screen. Choose Reconnect to try again."

export function MacosRemoteViewer({ name, device, getSession, resize, onFullscreen }: {
  name: string
  device: string | null
  getSession: () => Promise<MacosDisplaySession>
  resize: (size: { widthPx: number; heightPx: number }) => Promise<unknown>
  onFullscreen: () => void
}) {
  const [status, setStatus] = useState<MacosRemoteStatus>({ phase: "connecting" })
  const [notice, setNotice] = useState<string | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const reconnectRef = useRef<() => void>(() => {})
  const getSessionRef = useRef(getSession)
  const resizeRef = useRef(resize)
  useEffect(() => {
    getSessionRef.current = getSession
    resizeRef.current = resize
  })

  useEffect(() => {
    const container = containerRef.current
    if (!container) return
    let disposed = false
    let generation = 0
    let attempts = 0
    let retrying = false
    let rfb: RFB | null = null
    let detach: (() => void) | null = null
    let retryTimer: ReturnType<typeof setTimeout> | undefined
    let resizeTimer: ReturnType<typeof setTimeout> | undefined
    let connected = false
    let lastRequested: { widthPx: number; heightPx: number } | null = null
    let resizeInFlight = false
    let resizeQueued = false

    const dispose = () => {
      detach?.()
      detach = null
      try { rfb?.disconnect() } catch { /* already closed */ }
      rfb = null
      connected = false
      clearTimeout(resizeTimer)
      container.replaceChildren()
    }

    const flushResize = async () => {
      if (disposed || !connected) return
      const target = resizeTarget(container.clientWidth, container.clientHeight, window.devicePixelRatio)
      if (!target) return
      if (lastRequested && lastRequested.widthPx === target.widthPx && lastRequested.heightPx === target.heightPx) return
      if (resizeInFlight) { resizeQueued = true; return }
      resizeInFlight = true
      lastRequested = target
      const owner = generation
      try {
        await resizeRef.current(target)
        if (!disposed && owner === generation) setNotice(null)
      } catch (cause) {
        if (!disposed && owner === generation) {
          lastRequested = null
          setNotice(`The computer's screen size could not be changed: ${errorText(cause)}`)
        }
      } finally {
        resizeInFlight = false
        if (resizeQueued) { resizeQueued = false; void flushResize() }
      }
    }

    const observer = new ResizeObserver(() => {
      clearTimeout(resizeTimer)
      resizeTimer = setTimeout(() => void flushResize(), RESIZE_DEBOUNCE_MS)
    })
    observer.observe(container)

    const scheduleRetry = (reason: string) => {
      if (attempts >= RECONNECT_MAX_ATTEMPTS) {
        retrying = false
        setStatus({ phase: "disconnected", message: reason })
        return
      }
      attempts += 1
      retrying = true
      const delayMs = reconnectDelay(attempts)
      setStatus({ phase: "reconnecting", attempt: attempts, delayMs, message: reason })
      retryTimer = setTimeout(() => void connect(), delayMs)
    }

    const connect = async () => {
      const current = ++generation
      clearTimeout(retryTimer)
      dispose()
      setNotice(null)
      if (!retrying) setStatus({ phase: "connecting" })
      let session: MacosDisplaySession | null
      try {
        session = await getSessionRef.current()
      } catch (cause) {
        if (disposed || current !== generation) return
        const message = errorText(cause)
        if (isTransientSessionError(message)) scheduleRetry(message)
        else { retrying = false; setStatus({ phase: "unavailable", message }) }
        return
      }
      if (disposed || current !== generation) return
      let credentials: { username: string; password: string } | null = { username: session.username, password: session.password }
      const url = session.url
      session = null
      let authFailed = false
      let everConnected = false
      let client: RFB
      try {
        client = new RFB(container, url, { credentials: { ...credentials }, shared: false })
      } catch (cause) {
        credentials = null
        retrying = false
        setStatus({ phase: "unavailable", message: errorText(cause) })
        return
      }
      client.scaleViewport = true
      client.resizeSession = false
      client.clipViewport = false
      client.showDotCursor = false
      client.focusOnClick = true
      client.qualityLevel = 6
      client.compressionLevel = 2
      rfb = client

      const listeners: Array<[string, Listener]> = [
        ["connect", () => {
          if (current !== generation) return
          credentials = null
          everConnected = true
          connected = true
          attempts = 0
          retrying = false
          setStatus({ phase: "connected" })
          client.focus()
          clearTimeout(resizeTimer)
          lastRequested = null
          void flushResize()
        }],
        ["credentialsrequired", () => {
          if (current !== generation || !credentials) return
          client.sendCredentials({ ...credentials })
        }],
        ["securityfailure", () => {
          if (current !== generation) return
          authFailed = true
          credentials = null
          retrying = false
          setStatus({ phase: "auth-failed", message: AUTH_FAILED_MESSAGE })
        }],
        ["serververification", () => { if (current === generation) client.approveServer() }],
        ["disconnect", event => {
          if (current !== generation) return
          credentials = null
          connected = false
          if (authFailed) return
          // noVNC reports a close from the server's side as clean even when the owner dropped the stream,
          // so any close of an established session is retried.
          if (everConnected || retrying) scheduleRetry("The connection to the computer was lost.")
          else {
            retrying = false
            setStatus({ phase: "disconnected", message: event.detail?.clean === true ? "The computer ended the display session." : "Could not connect to the computer's screen." })
          }
        }],
      ]
      for (const [type, listener] of listeners) client.addEventListener(type, listener)
      detach = () => { for (const [type, listener] of listeners) client.removeEventListener(type, listener) }
    }

    reconnectRef.current = () => { attempts = 0; retrying = false; void connect() }
    void connect()
    return () => {
      disposed = true
      generation += 1
      clearTimeout(retryTimer)
      observer.disconnect()
      dispose()
    }
  }, [])

  const failed = status.phase === "disconnected" || status.phase === "auth-failed" || status.phase === "unavailable"
  const overlay = status.phase !== "connected"
  return <TooltipProvider><main className="flex h-dvh min-h-0 flex-col bg-background text-foreground">
    <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-3">
      <Monitor aria-hidden="true" className="size-4" />
      <h1 className="min-w-0 flex-1 truncate text-xs font-medium">{device ? `${name} · ${device}` : name}</h1>
      {notice && <div role="alert" className="flex min-w-0 items-center gap-1 text-xs text-destructive">
        <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" /><span className="truncate" title={notice}>{notice}</span>
      </div>}
      <span role="status" className="shrink-0 text-xs text-muted-foreground">{statusLabel(status)}</span>
      {failed && <Button size="xs" variant="ghost" onClick={() => reconnectRef.current()}>Reconnect</Button>}
      <Button variant="ghost" size="icon-xs" aria-label="Toggle fullscreen" onClick={onFullscreen}><Maximize /></Button>
    </header>
    <section aria-label="Computer display" className="relative flex min-h-0 flex-1 bg-black">
      <div ref={containerRef} aria-label="macOS display" className="min-h-0 min-w-0 flex-1" />
      {overlay && <div className="absolute inset-0 grid place-items-center bg-background/80 p-6 text-center">
        <div className="grid max-w-sm justify-items-center gap-2">
          {failed
            ? <p role="alert" className="text-sm">{status.message}</p>
            : <p className="text-sm text-muted-foreground">{status.phase === "reconnecting" && status.message ? status.message : "Connecting to the computer's screen…"}</p>}
        </div>
      </div>}
    </section>
  </main></TooltipProvider>
}

export function NativeMacosRemoteViewer({ name, device }: Pick<MacosDisplayRoute, "name" | "device">) {
  return <MacosRemoteViewer
    name={name}
    device={device}
    getSession={() => invoke<MacosDisplaySession>("macos_display_session")}
    resize={size => invoke("macos_display_resize", size)}
    onFullscreen={() => { const window = getCurrentWindow(); void window.isFullscreen().then(value => window.setFullscreen(!value)).catch(() => {}) }}
  />
}
