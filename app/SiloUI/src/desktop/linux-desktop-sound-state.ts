import { useCallback, useEffect, useState } from "react"
import { invoke } from "@tauri-apps/api/core"

export type DesktopSound = {
  /** Whether this device's web engine can play the computer's sound. */
  available: boolean
  muted: boolean
  onToggle: () => void
}

const MUTED_KEY_PREFIX = "silo-desktop-muted:"

// The choice belongs to this device and this computer, so it lives in the
// viewer's own storage rather than in the shared settings document.
export function readMuted(computer: string): boolean {
  try { return localStorage.getItem(MUTED_KEY_PREFIX + computer) === "true" } catch { return false }
}

export function writeMuted(computer: string, muted: boolean) {
  try {
    if (muted) localStorage.setItem(MUTED_KEY_PREFIX + computer, "true")
    else localStorage.removeItem(MUTED_KEY_PREFIX + computer)
  } catch { /* Browser storage can be unavailable. */ }
}

/**
 * Asks the native side whether sound can play once the display is attached
 * (`generation` changes with each new connection), keeps the computer's audio
 * stream on only while the viewer is visible, and applies the mute choice.
 */
export function useDesktopSound(computer: string, connected: boolean, generation: number): DesktopSound {
  const probeKey = `${computer}:${generation}`
  const [probe, setProbe] = useState<{ key: string; sound: boolean } | null>(null)
  const [muted, setMuted] = useState(() => readMuted(computer))
  const [visible, setVisible] = useState(() => document.visibilityState !== "hidden")
  const available = connected && probe?.key === probeKey && probe.sound
  useEffect(() => {
    const update = () => setVisible(document.visibilityState !== "hidden")
    document.addEventListener("visibilitychange", update)
    return () => document.removeEventListener("visibilitychange", update)
  }, [])
  useEffect(() => {
    if (!connected) return
    let disposed = false
    Promise.resolve(invoke<{ sound: boolean }>("desktop_viewer_sound_support", { computer }))
      .then(result => { if (!disposed) setProbe({ key: probeKey, sound: result?.sound === true }) })
      .catch(() => {})
    return () => { disposed = true }
  }, [computer, connected, probeKey])
  useEffect(() => {
    if (!available) return
    void Promise.resolve(invoke("desktop_viewer_set_audio", { computer, muted, active: visible })).catch(() => {})
  }, [computer, available, muted, visible])
  const onToggle = useCallback(() => {
    writeMuted(computer, !muted)
    setMuted(!muted)
  }, [computer, muted])
  return { available, muted, onToggle }
}
