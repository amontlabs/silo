import { useEffect, useEffectEvent, useId, useState } from "react"

import { Button } from "@/components/ui/button"
import { formatBinaryBytes } from "@/lib/format-bytes"
import { deleteComputerDescription, deleteComputerTitle } from "@/features/computers/model/delete-computer-copy"

/** What the Delete computer dialog states and offers, shared by the list row and the computer page. */
export interface DeleteComputerDetails {
  /** Checkpoints deleted with the computer, when known. */
  checkpoints?: number
  /** Reads the computer's size on this device, in bytes. Shown when it resolves. */
  readSize?: () => Promise<number | null>
  /** Exports the computer and resolves true only once the export was verified. */
  exportFirst?: () => Promise<boolean>
}

/**
 * The one Delete computer dialog (decision 6), identical from the list row and the computer
 * page: "Delete {name} permanently?", what is lost with its size, and Delete permanently
 * (destructive) or Cancel. With `exportFirst`, "Export, then delete" deletes only after a
 * verified export.
 */
export function DeleteComputerBody({ displayName, details = {}, draft = false, onDelete, onClose }: {
  /** "dev", or "dev on Office" for a remote computer. */
  displayName: string
  details?: DeleteComputerDetails
  /** The computer has not been created yet, so nothing but its saved settings goes away. */
  draft?: boolean
  onDelete: () => void | Promise<unknown>
  onClose: () => void
}) {
  const [size, setSize] = useState<string>()
  const titleId = useId()
  const descriptionId = useId()
  const { exportFirst, checkpoints } = details
  // Read the size once when the dialog opens; the size is informative and deletion never waits for it.
  const readSize = useEffectEvent(() => details.readSize?.() ?? Promise.resolve(null))
  useEffect(() => {
    let current = true
    readSize()
      .then((bytes) => { if (current && bytes !== null) setSize(formatBinaryBytes(bytes)) })
      .catch(() => undefined)
    return () => { current = false }
  }, [])

  function run(action: () => void | Promise<unknown>) {
    onClose()
    void Promise.resolve().then(action)
  }

  const cancel = <Button type="button" variant="ghost" size="sm" onClick={onClose}>Cancel</Button>
  const remove = <Button type="button" variant="destructive" size="sm" aria-describedby={`${titleId} ${descriptionId}`} autoFocus data-popover-initial-focus="" onClick={() => run(onDelete)}>{draft ? "Remove" : "Delete permanently"}</Button>
  return <div className="grid gap-2">
    <p id={titleId} className="font-medium">{draft ? `Remove ${displayName} from setup?` : deleteComputerTitle(displayName)}</p>
    <div id={descriptionId} className="text-muted-foreground">{draft ? "It has not been created yet, so no files are affected." : deleteComputerDescription(checkpoints, size)}</div>
    {/* Three choices do not fit one row of the popover, so they stack full width like a macOS alert. */}
    {exportFirst
      ? <div className="grid gap-1.5 pt-1">
        {remove}
        <Button type="button" variant="outline" size="sm" aria-describedby={`${titleId} ${descriptionId}`} onClick={() => run(async () => { if (await exportFirst()) await onDelete() })}>Export, then delete</Button>
        {cancel}
      </div>
      : <div className="flex justify-end gap-2">{cancel}{remove}</div>}
  </div>
}
