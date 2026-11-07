import { AlertDialog } from "radix-ui"

import { Button } from "@/components/ui/button"
export interface ShareLoginsRequest {
  computerName: string
  logins: string[]
  /** Why Silo could not check the computer (stopped, a checkpoint, a failed check). Asked about like logins. */
  unchecked?: string
  answer: (share: boolean) => void
}

/** Asks before a computer that holds logins is exported. The files stay in the export either way; this only names them. */
export function ShareLoginsConfirmation({ request }: { request: ShareLoginsRequest | null }) {
  return <AlertDialog.Root open={request !== null} onOpenChange={(open) => { if (!open) request?.answer(false) }}>
    <AlertDialog.Portal>
      <AlertDialog.Overlay className="fixed inset-0 z-50 bg-black/20" />
      <AlertDialog.Content className="fixed left-1/2 top-1/2 z-50 grid w-[28rem] max-w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 gap-3 rounded-lg border bg-popover p-4 text-ui shadow-lg">
        <AlertDialog.Title className="text-ui font-medium">Share {request?.computerName}?</AlertDialog.Title>
        <AlertDialog.Description asChild>
          <div className="grid gap-2 text-muted-foreground">
            {request && request.logins.length > 0 && <>
              <p>This computer contains:</p>
              <ul className="grid list-disc gap-1 pl-5">
                {request.logins.map((login) => <li key={login}>{login}</li>)}
              </ul>
              <p>Anyone who gets the export file can use them. Silo keeps these files in the export.</p>
            </>}
            {request?.unchecked && <p>Silo could not check {request.computerName} for {request.logins.length > 0 ? "other " : ""}logins ({request.unchecked.trim().replace(/\.$/, "")}).</p>}
          </div>
        </AlertDialog.Description>
        <div className="flex justify-end gap-2">
          <AlertDialog.Cancel asChild><Button type="button" variant="ghost" size="sm" autoFocus>Cancel</Button></AlertDialog.Cancel>
          <AlertDialog.Action asChild><Button type="button" variant="outline" size="sm" onClick={() => request?.answer(true)}>Share anyway</Button></AlertDialog.Action>
        </div>
      </AlertDialog.Content>
    </AlertDialog.Portal>
  </AlertDialog.Root>
}
