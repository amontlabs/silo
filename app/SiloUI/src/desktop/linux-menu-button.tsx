import { invoke, isTauri } from "@tauri-apps/api/core"
import { Menu } from "lucide-react"
import { isLinux } from "@/lib/platform"
import { Button } from "@/components/ui/button"
import { showActionFailure } from "@/lib/operation-toast"

export function LinuxMenuButton({ disabled = false }: { disabled?: boolean }) {
  if (!isTauri() || !isLinux()) return null
  const open = () => void invoke("show_app_menu").catch((error) => showActionFailure("Could not open the menu", error, open, { native: false }))
  return <div className="relative mx-3 shrink-0">
    <Button variant="ghost" size="sm" disabled={disabled} aria-keyshortcuts="Alt F10" onClick={open}><Menu aria-hidden="true" className="size-3.5" />Menu</Button>
  </div>
}
