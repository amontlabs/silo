import { InlineAlert } from "@/components/inline-alert"
import { SiloWindow } from "@/components/silo-window"
import { Button } from "@/components/ui/button"

/** Shown when the window's own code could not load, so nothing else from it can render. */
export function StartupFailure({ message, retry }: { message: string; retry: () => void }) {
  return (
    <SiloWindow title="Silo" label="Silo unavailable">
      <div className="grid flex-1 place-items-center p-6">
        <InlineAlert size="lg" className="max-w-lg gap-0 rounded-lg">
          <h1 className="text-sm font-semibold">Silo could not load</h1>
          <p className="mt-1 whitespace-pre-wrap text-xs text-muted-foreground">{message}</p>
          <Button type="button" variant="outline" size="sm" className="mt-3 justify-self-start" onClick={retry}>Retry</Button>
        </InlineAlert>
      </div>
    </SiloWindow>
  )
}
