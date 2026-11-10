import { AlertDialog } from "radix-ui"

import { Button } from "@/components/ui/button"

/** What the person chose: stop, export without the logins, or keep the logins in the file. */
export type ShareLoginsChoice = "cancel" | "leave-out" | "include"

export interface ShareLoginsRequest {
  computerName: string
  logins: string[]
  /** Why Silo could not check the computer (stopped, a checkpoint, a failed check). */
  unchecked?: string
  /** The export removes the logins from the exported computer unless the person asks to include them. False when it cannot: they stay in the file. */
  leavesOut: boolean
  answer: (choice: ShareLoginsChoice) => void
}

/**
 * Names the logins on a computer before it is exported. Where Silo can strip them (a Linux computer's current state)
 * the export removes them from the exported computer, with the caveat that traces may remain in the file's disk layers,
 * and the person can choose to include them; where it cannot, they stay in the file and the person is asked whether to
 * share anyway.
 */
export function ShareLoginsConfirmation({ request }: { request: ShareLoginsRequest | null }) {
  return <AlertDialog.Root open={request !== null} onOpenChange={(open) => { if (!open) request?.answer("cancel") }}>
    <AlertDialog.Portal>
      <AlertDialog.Overlay className="fixed inset-0 z-50 bg-black/20" />
      <AlertDialog.Content className="fixed left-1/2 top-1/2 z-50 grid w-[28rem] max-w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 gap-3 rounded-lg border bg-popover p-4 text-ui shadow-lg">
        <AlertDialog.Title className="text-ui font-medium">{request ? (request.leavesOut ? `Export ${request.computerName}?` : `Share ${request.computerName}?`) : ""}</AlertDialog.Title>
        <AlertDialog.Description asChild>
          <div className="grid gap-2 text-muted-foreground">
            {request && request.logins.length > 0 && <>
              <p>This computer contains:</p>
              <ul className="grid list-disc gap-1 pl-5">
                {request.logins.map((login) => <li key={login}>{login}</li>)}
              </ul>
              {request.leavesOut
                ? <p>Silo removes these files from the export, but traces may remain in the file. Keep the file private either way, and include the logins only if you mean to share them.</p>
                : <p>Anyone who gets the export file can use them. Silo cannot leave these files out of this export.</p>}
            </>}
            {request && !request.leavesOut && request.unchecked && <p>Silo could not check {request.computerName} for {request.logins.length > 0 ? "other " : ""}logins ({request.unchecked.trim().replace(/\.$/, "")}).</p>}
          </div>
        </AlertDialog.Description>
        <div className="flex justify-end gap-2">
          <AlertDialog.Cancel asChild><Button type="button" variant="ghost" size="sm" autoFocus>Cancel</Button></AlertDialog.Cancel>
          {request?.leavesOut
            ? <>
              <AlertDialog.Action asChild><Button type="button" variant="ghost" size="sm" onClick={() => request.answer("include")}>Include logins</Button></AlertDialog.Action>
              <AlertDialog.Action asChild><Button type="button" variant="outline" size="sm" onClick={() => request.answer("leave-out")}>Remove logins and export</Button></AlertDialog.Action>
            </>
            : <AlertDialog.Action asChild><Button type="button" variant="outline" size="sm" onClick={() => request?.answer("include")}>Share anyway</Button></AlertDialog.Action>}
        </div>
      </AlertDialog.Content>
    </AlertDialog.Portal>
  </AlertDialog.Root>
}
