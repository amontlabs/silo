import { SiloWindow } from "@/components/silo-window"

/** Shown when the window's own code could not load, so nothing else from it can render. */
export function StartupFailure({ message, retry }: { message: string; retry: () => void }) {
  return (
    <SiloWindow title="Silo" label="Silo unavailable">
      <div className="grid flex-1 place-items-center p-6">
        <div className="max-w-lg rounded-lg border border-destructive/25 bg-destructive/[.06] p-4" role="alert">
          <h1 className="text-sm font-semibold">Silo could not load</h1>
          <p className="mt-1 whitespace-pre-wrap text-xs text-muted-foreground">{message}</p>
          <button type="button" className="mt-3 rounded-md border px-3 py-1.5 text-xs" onClick={retry}>Retry</button>
        </div>
      </div>
    </SiloWindow>
  )
}
