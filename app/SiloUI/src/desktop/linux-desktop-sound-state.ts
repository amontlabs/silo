import { useCallback, useEffect, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"

export type DesktopSound = {
  /** Whether this device's web engine can play the computer's sound. */
  available: boolean
  muted: boolean
  onToggle: () => void
}

const MUTED_KEY_PREFIX = "silo-desktop-muted:"
/** Emitted by the native side each time the computer's display page finishes loading. */
export const DESKTOP_PAGE_EVENT = "silo://desktop-viewer-page"
/** Pause between sound checks while the display is still connecting, and how many are made per page. */
export const SOUND_PROBE_RETRY_MS = 3000
const SOUND_PROBE_RETRIES = 40

// The choice belongs to this device and this computer, so it lives in the
// viewer's own storage rather than in the shared settings document, under the
// computer's stable identity so a rename keeps it.
export function readMuted(computerId: string): boolean {
  try { return localStorage.getItem(MUTED_KEY_PREFIX + computerId) === "true" } catch { return false }
}

export function writeMuted(computerId: string, muted: boolean) {
  try {
    if (muted) localStorage.setItem(MUTED_KEY_PREFIX + computerId, "true")
    else localStorage.removeItem(MUTED_KEY_PREFIX + computerId)
  } catch { /* Browser storage can be unavailable. */ }
}

// Sound updates run on separate native workers; the revision lets the native
// side drop an update that a newer one has already overtaken.
let lastRevision = Date.now()
const nextRevision = () => ++lastRevision

/**
 * Asks the native side whether sound can play once the display is attached and
 * its page has loaded (the check repeats while the display is still
 * connecting), keeps the computer's audio stream on only while the viewer is
 * visible, and applies the mute choice. `connection` changes with each new
 * connection; every page load of the display repeats the check and the setup,
 * because a replacement page starts without them.
 */
export function useDesktopSound(computer: string, computerId: string, attached: boolean, connection: number): DesktopSound {
  const [pageLoads, setPageLoads] = useState(0)
  const probeKey = `${computer}:${connection}:${pageLoads}`
  const [probe, setProbe] = useState<{ key: string; sound: boolean } | null>(null)
  const [muted, setMuted] = useState(() => readMuted(computerId))
  const [visible, setVisible] = useState(() => document.visibilityState !== "hidden")
  const available = attached && probe?.key === probeKey && probe.sound
  useEffect(() => {
    const update = () => setVisible(document.visibilityState !== "hidden")
    document.addEventListener("visibilitychange", update)
    return () => document.removeEventListener("visibilitychange", update)
  }, [])
  useEffect(() => {
    let disposed = false
    let stop: (() => void) | undefined
    Promise.resolve().then(() => listen(DESKTOP_PAGE_EVENT, () => setPageLoads(count => count + 1)))
      .then(unlisten => { if (disposed) unlisten(); else stop = unlisten })
      .catch(() => {})
    return () => { disposed = true; stop?.() }
  }, [])
  useEffect(() => {
    if (!attached) return
    let disposed = false
    let timer: number | undefined
    let retries = 0
    const check = () => {
      Promise.resolve(invoke<{ sound: boolean }>("desktop_viewer_sound_support", { computer }))
        .then(result => { if (!disposed) setProbe({ key: probeKey, sound: result?.sound === true }) })
        .catch(() => { if (!disposed && retries++ < SOUND_PROBE_RETRIES) timer = window.setTimeout(check, SOUND_PROBE_RETRY_MS) })
    }
    check()
    return () => { disposed = true; window.clearTimeout(timer) }
  }, [computer, attached, probeKey])
  // A new page or connection restarts the native check by asking again; only
  // leaving the display abandons it.
  useEffect(() => {
    if (!attached) return
    return () => { void Promise.resolve(invoke("desktop_viewer_sound_cancel", { computer })).catch(() => {}) }
  }, [computer, attached])
  useEffect(() => {
    if (!available) return
    void Promise.resolve(invoke("desktop_viewer_set_audio", { computer, muted, active: visible, revision: nextRevision() })).catch(() => {})
  }, [computer, available, muted, visible])
  const onToggle = useCallback(() => {
    writeMuted(computerId, !muted)
    setMuted(!muted)
  }, [computerId, muted])
  return { available, muted, onToggle }
}
