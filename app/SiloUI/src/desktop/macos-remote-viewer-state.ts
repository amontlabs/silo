/** What a macOS computer's remote display window shows. */
export type MacosRemotePhase = "connecting" | "connected" | "reconnecting" | "disconnected" | "auth-failed" | "unavailable"

export interface MacosRemoteStatus {
  phase: MacosRemotePhase
  /** Reason shown for disconnected, authentication and unavailable states. */
  message?: string
  /** Reconnect attempt number and its delay while reconnecting. */
  attempt?: number
  delayMs?: number
}

export interface MacosDisplaySession {
  url: string
  username: string
  password: string
  width: number
  height: number
}

export interface MacosDisplayRoute {
  computer: string
  device: string | null
  name: string
}

export const RECONNECT_BASE_MS = 1000
export const RECONNECT_MAX_MS = 15000
export const RECONNECT_MAX_ATTEMPTS = 8
export const RESIZE_DEBOUNCE_MS = 300
export const MIN_RESIZE_WIDTH_PX = 640
export const MIN_RESIZE_HEIGHT_PX = 400

/** Reads the display window's query: `macosDisplay` is the computer, `device` and `name` label it. */
export function macosDisplayRoute(search: string = window.location.search): MacosDisplayRoute | null {
  const query = new URLSearchParams(search)
  const computer = query.get("macosDisplay")
  if (!computer) return null
  return { computer, device: query.get("device") || null, name: query.get("name") || computer }
}

/** Delay before reconnect attempt `attempt` (1-based): 1s, 2s, 4s, ... capped at 15s. */
export function reconnectDelay(attempt: number): number {
  return Math.min(RECONNECT_BASE_MS * 2 ** Math.max(attempt - 1, 0), RECONNECT_MAX_MS)
}

export function errorText(cause: unknown): string {
  if (typeof cause === "string") return cause
  if (cause instanceof Error) return cause.message
  return String(cause)
}

/** Whether a failed session request may succeed later without the user changing anything. */
export function isTransientSessionError(message: string): boolean {
  if (/not running|not (?:finished|complete)|set ?up|stopped|unavailable|disabled|not enabled/i.test(message)) return false
  return /offline|unreachable|timed out|timeout|connection (?:lost|refused|reset)|temporarily/i.test(message)
}

/** The pixel size to request for a container, even and at least the minimum, or null when too small. */
export function resizeTarget(clientWidth: number, clientHeight: number, pixelRatio: number): { widthPx: number; heightPx: number } | null {
  const ratio = pixelRatio > 0 ? pixelRatio : 1
  const widthPx = Math.round(clientWidth * ratio) & ~1
  const heightPx = Math.round(clientHeight * ratio) & ~1
  if (widthPx < MIN_RESIZE_WIDTH_PX || heightPx < MIN_RESIZE_HEIGHT_PX) return null
  return { widthPx, heightPx }
}

export function statusLabel(status: MacosRemoteStatus): string {
  switch (status.phase) {
    case "connecting": return "Connecting…"
    case "connected": return "Connected"
    case "reconnecting": return `Reconnecting (attempt ${status.attempt ?? 1}, in ${Math.ceil((status.delayMs ?? 0) / 1000)}s)…`
    case "disconnected": return "Disconnected"
    case "auth-failed": return "Authentication failed"
    case "unavailable": return "Unavailable"
  }
}
