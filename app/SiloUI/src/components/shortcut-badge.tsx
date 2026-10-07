import type { KeyboardShortcut } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"

export function ShortcutBadge({ shortcut, className }: { shortcut: KeyboardShortcut; className?: string }) {
  return <kbd aria-hidden="true" className={cn("inline-flex shrink-0 items-center gap-0.5 rounded border border-current/20 bg-current/5 px-1 py-0.5 font-sans text-caption leading-none whitespace-nowrap", className)}>
    {shortcut.keys.map((key, index) => <span key={`${index}-${key}`}>{key}</span>)}
  </kbd>
}
