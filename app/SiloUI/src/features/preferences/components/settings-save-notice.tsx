import { useState } from "react"
import { Button } from "@/components/ui/button"
import { InlineAlert } from "@/components/inline-alert"
import { useSettings } from "@/features/preferences/settings-store"

export function SettingsSaveNotice() {
  const { store, saveError, writeProtected, canResetProtected, resetProtected } = useSettings()
  const [saving, setSaving] = useState(false)
  const [confirming, setConfirming] = useState(false)
  if (!saveError) return null
  return <InlineAlert className="gap-0">
    <p>{writeProtected ? "Settings are protected from writes. Changes last for this session." : "Settings could not be saved. Keep Silo open and retry."}</p>
    <p className="mt-1 whitespace-pre-wrap text-muted-foreground">{saveError}</p>
    {!writeProtected && <Button type="button" size="xs" variant="outline" className="mt-2" disabled={saving} onClick={() => {
      setSaving(true)
      void store.flush().finally(() => setSaving(false))
    }}>{saving ? "Saving settings…" : "Retry saving settings"}</Button>}
    {writeProtected && canResetProtected && (confirming
      ? <div className="mt-2 space-y-2">
        <p className="text-muted-foreground">Silo keeps the current settings file next to it with an .invalid suffix and starts again from the default settings. Your computers are not affected.</p>
        <div className="flex gap-2">
          <Button type="button" size="xs" variant="destructive" onClick={() => { setConfirming(false); void resetProtected() }}>Reset settings</Button>
          <Button type="button" size="xs" variant="outline" onClick={() => setConfirming(false)}>Cancel</Button>
        </div>
      </div>
      : <Button type="button" size="xs" variant="outline" className="mt-2" onClick={() => setConfirming(true)}>Reset settings…</Button>)}
  </InlineAlert>
}