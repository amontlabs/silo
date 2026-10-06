import { TriangleAlert, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { shallowEqual, useSettingsSelector, useSettingsStore } from "@/features/preferences/settings-store"

/** A one-time warning that Silo can lose data. Dismissing it is saved with the other settings. */
export function AlphaNotice() {
  const { updateSettings } = useSettingsStore()
  const { loaded, dismissed } = useSettingsSelector((view) => ({ loaded: view.revision >= 0, dismissed: view.settings.alphaNoticeDismissed }), shallowEqual)
  // Before the saved settings arrive, the default would show it again to someone who dismissed it.
  if (!loaded || dismissed) return null
  const dismiss = () => { void updateSettings({ alphaNoticeDismissed: true }) }
  return <div className="mx-auto w-full max-w-4xl px-4 pt-4 sm:px-6">
    <section aria-labelledby="alpha-notice-title" className="flex items-start gap-2.5 rounded-md border border-warning/30 bg-warning/[.07] px-3 py-2.5 text-xs">
      <TriangleAlert className="mt-px size-3.5 shrink-0 text-warning" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <h2 id="alpha-notice-title" className="font-medium">Silo is in alpha</h2>
        <p className="mt-0.5 text-muted-foreground">An update or a bug can lose computer data. Export computers you care about regularly from their actions menu (Export…), and push your work to GitHub often.</p>
        <Button size="xs" variant="outline" className="mt-2" onClick={dismiss}>Got it</Button>
      </div>
      <Button size="icon-xs" variant="ghost" aria-label="Dismiss alpha notice" onClick={dismiss}><X className="size-3" /></Button>
    </section>
  </div>
}
