import { useSettingsSelector, useSettingsStore, type SettingsStore } from "./settings-store"
import type { Settings } from "./model/settings"

export type Theme = Settings["theme"]

export function useTheme() {
  const { updateSettings } = useSettingsStore()
  const theme = useSettingsSelector((view) => view.settings.theme)
  return { theme, setTheme: (next: string) => { void updateSettings({ theme: next as Theme }) } }
}

// Initialize before React renders, in both the main window and the status panel.
export function initializeTheme(store: SettingsStore) {
  const system = window.matchMedia("(prefers-color-scheme: dark)")
  function apply() {
    const { theme } = store.getSnapshot().settings
    document.documentElement.classList.toggle("dark", theme === "dark" || (theme === "system" && system.matches))
  }
  apply()
  const unsubscribe = store.subscribe(apply)
  system.addEventListener("change", apply)
  return () => {
    unsubscribe()
    system.removeEventListener("change", apply)
  }
}
