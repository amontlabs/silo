import { useId } from "react"

import { InlineConfirmation } from "@/components/inline-confirmation"
import { Button } from "@/components/ui/button"
import { quitConfirmationDetail } from "./quit-confirmation-model"

export function QuitConfirmation({ names, onCancel, onKeepRunning, onStop }: { names: string[]; onCancel: () => void; onKeepRunning: () => void; onStop: () => void }) {
  const titleId = useId()
  const detailId = useId()
  return (
    <InlineConfirmation active onDismiss={onCancel}>
      <div role="alertdialog" aria-labelledby={titleId} aria-describedby={detailId} className="grid w-full gap-2 text-xs">
        <p id={titleId} className="font-medium">Quit Silo?</p>
        <p id={detailId} className="text-muted-foreground">{quitConfirmationDetail(names)}</p>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" size="sm" onClick={onCancel}>Cancel</Button>
          <Button type="button" variant="outline" size="sm" autoFocus onClick={onKeepRunning}>Keep running</Button>
          <Button type="button" variant="destructive" size="sm" onClick={onStop}>Stop and quit</Button>
        </div>
      </div>
    </InlineConfirmation>
  )
}
