import { isMac } from "@/lib/platform"

export interface KeyboardShortcut { keys: string[]; aria: string }

const shortcutKeys: Readonly<Record<string, string>> = {
  "go-computers": "1", "go-files": "2", "go-logs": "3", "go-network": "4",
  "go-activity": "5", "go-github": "6", "go-secrets": "7",
  settings: ",", search: "K", "toggle-sidebar": "B", "go-back": "[", "go-forward": "]", "new-computer": "N",
}

export function shortcutFor(action: string, platform?: string): KeyboardShortcut | undefined {
  const key = shortcutKeys[action]
  if (!key) return undefined
  const mac = platform === undefined ? isMac() : platform.startsWith("Mac")
  return { keys: [mac ? "⌘" : "Ctrl", key], aria: `${mac ? "Meta" : "Control"}+${key}` }
}

export function desktopShortcutCommand(event: KeyboardEvent): string | undefined {
  if (!event.ctrlKey || event.metaKey || event.altKey || event.repeat || event.isComposing || event.defaultPrevented) return
  // Some layouts need Shift to produce digits or punctuation, but Ctrl-Shift-B
  // remains a different shortcut from Ctrl-B.
  if (event.shiftKey && /^[a-z]$/i.test(event.key)) return
  // Search owns its existing cross-platform listener in the command palette.
  return Object.keys(shortcutKeys).find(action => action !== "search" && shortcutKeys[action].toLowerCase() === event.key.toLowerCase())
}
