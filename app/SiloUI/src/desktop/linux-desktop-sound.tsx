import { Volume2, VolumeX } from "lucide-react"
import { Button } from "@/components/ui/button"
import type { DesktopSound } from "./linux-desktop-sound-state"

export function DesktopSoundButton({ sound }: { sound: DesktopSound }) {
  if (!sound.available) return null
  return <Button variant="ghost" size="icon-xs" aria-label={sound.muted ? "Unmute sound" : "Mute sound"} aria-pressed={sound.muted} onClick={sound.onToggle}>
    {sound.muted ? <VolumeX /> : <Volume2 />}
  </Button>
}
