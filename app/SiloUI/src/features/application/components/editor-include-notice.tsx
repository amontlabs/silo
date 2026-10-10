import { TriangleAlert, X } from "lucide-react"
import { CopyButton } from "@/components/copy-button"
import { Button } from "@/components/ui/button"
import { useEditorIncludeLine } from "@/features/application/model/editor-include"
import { shallowEqual, useSettingsSelector, useSettingsStore } from "@/features/preferences/settings-store"

/**
 * The line the user has to add to their SSH configuration because Silo can't change it. It stays
 * until the line is there or the user dismisses it. The dismissal names the line, so a different
 * needed line shows the notice again, and it is saved with the other settings.
 */
export function EditorIncludeNotice() {
  const line = useEditorIncludeLine()
  const { updateSettings } = useSettingsStore()
  const { loaded, dismissed } = useSettingsSelector((view) => ({ loaded: view.revision >= 0, dismissed: view.settings.editorIncludeNoticeDismissed }), shallowEqual)
  // Before the saved settings arrive, the default would show it again to someone who dismissed it.
  if (!loaded || !line || dismissed === line) return null
  const dismiss = () => { void updateSettings({ editorIncludeNoticeDismissed: line }) }
  return <div className="mx-auto w-full max-w-4xl px-4 pt-4 sm:px-6">
    <section aria-labelledby="editor-include-notice-title" className="flex items-start gap-2.5 rounded-md border border-warning/30 bg-warning/[.07] px-3 py-2.5 text-xs">
      <TriangleAlert className="mt-px size-3.5 shrink-0 text-warning" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <h2 id="editor-include-notice-title" className="font-medium">Add a line to your SSH configuration</h2>
        <p className="mt-0.5 text-muted-foreground">Silo couldn't update your SSH config, which links to a file it can't change. Add this line at the top so editors reconnect to your current computers.</p>
        <div className="mt-2 flex items-start gap-2">
          <code className="min-w-0 flex-1 select-all break-all rounded border bg-background/60 px-2 py-1 font-mono text-caption">{line}</code>
          <CopyButton size="xs" variant="outline" value={line} labels={{ idle: "Copy line to add", copied: "Line copied", failed: "Copy failed" }} text={{ idle: "Copy", copied: "Copied", failed: "Copy failed" }} />
        </div>
        <Button size="xs" variant="outline" className="mt-2" onClick={dismiss}>Got it</Button>
      </div>
      <Button size="icon-xs" variant="ghost" aria-label="Dismiss SSH configuration notice" onClick={dismiss}><X className="size-3" /></Button>
    </section>
  </div>
}
