import { useState } from "react"
import { Download, RefreshCw, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { SectionHeading } from "@/components/page"
import { Progress } from "@/components/ui/progress"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"
import { InlineConfirmation } from "@/components/inline-confirmation"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { useUpdates, type UpdateSnapshot } from "./update-store"

function description(state: UpdateSnapshot) {
  switch (state.phase) {
    case "checking": return "Checking for updates…"
    case "available": return `Version ${state.availableVersion} is available.`
    case "downloading": return "Downloading update…"
    case "ready": return state.canInstall ? "Ready to install. Silo will restart." : state.installBlockReason ?? "Installation is unavailable. Try checking again."
    case "installing": return state.installStatus ?? "Installing update. Silo will restart…"
    default: return state.phase === "idle" && state.lastChecked ? `Version ${state.currentVersion} · Silo is up to date` : `Version ${state.currentVersion}`
  }
}

export function UpdatesCard() {
  const updates = useUpdates()
  if (!updates) return null
  const confirm = updates.installConfirmation
  const { snapshot: state, pending } = updates
  const busy = pending || state?.phase === "checking" || state?.phase === "downloading" || state?.phase === "installing"
  const error = state?.error ?? updates.connectionError
  const installing = state?.phase === "ready" || state?.retryAction === "install" || (state?.packageKind === "debian" && state?.phase === "available")
  const requestInstall = updates.requestInstall
  const retry = () => {
    if (!state) updates.reconnect()
    else if (state.packageKind === "manual" && state.phase === "available") updates.openRelease()
    else if (state.retryAction === "download") updates.download()
    else if (installing) requestInstall()
    else if (state.phase === "available") updates.download()
    else updates.check()
  }
  const percent = state?.totalBytes ? Math.min(100, Math.round(state.downloadedBytes / state.totalBytes * 100)) : undefined
  return <section className="grid gap-2" aria-label="Updates">
    <SectionHeading>Updates</SectionHeading>
    <ListCard divided>
      <div>
        <ListRow icon={<ListRowIcon><Download aria-hidden="true" className="size-3.5" /></ListRowIcon>}
          title="Silo" detail={state ? description(state) : <Skeleton className="h-2.5 w-40" />} detailClassName="whitespace-normal"
          actions={<InlineConfirmation active={confirm} onDismiss={updates.cancelInstall}>
            {confirm && installing && state ? <span className="flex shrink-0 gap-1.5">
              <Button size="xs" variant="outline" disabled={busy} onClick={updates.cancelInstall}>Cancel</Button>
              <Button size="xs" disabled={busy || !state.canInstall} onClick={() => updates.install(true)}>Stop computers and update</Button>
            </span> : state?.phase === "available" && state.packageKind === "debian" ? <Button size="xs" variant="outline" disabled={busy || !state.canInstall} onClick={requestInstall}>Update</Button>
              : state?.phase === "available" ? <Button size="xs" variant="outline" disabled={busy} onClick={state.packageKind === "manual" ? updates.openRelease : updates.download}>{state.packageKind === "manual" ? "View installers on GitHub" : "Download update"}</Button>
              : state?.phase === "ready" ? <Button size="xs" variant="outline" disabled={busy || !state.canInstall} onClick={requestInstall}>Restart and update</Button>
                : error || state?.phase === "downloading" || state?.phase === "installing" ? null : <Button size="xs" variant="outline" disabled={busy || !state} onClick={updates.check}><RefreshCw aria-hidden="true" className="size-3" />Check for updates</Button>}
          </InlineConfirmation>} />
        {confirm && installing && state && <p className="px-2 pb-2 text-caption text-muted-foreground">{state.runningComputers.join(", ")} will stop and restart after updating. Save your work before continuing.</p>}
        {state?.phase === "downloading" && <div className="px-2 pb-2">
          <Progress value={percent} aria-label="Update download" aria-valuetext={percent === undefined ? `${state.downloadedBytes.toLocaleString()} bytes downloaded` : `${percent}%`} />
          <p className="mt-1 text-caption text-muted-foreground">{percent === undefined ? `${(state.downloadedBytes / 1048576).toFixed(1)} MiB downloaded` : `${percent}%`}</p>
        </div>}
        {error && <div role="alert" className="mx-2 mb-2 rounded-md border border-destructive/25 bg-destructive/[.06] p-2 text-xs">
          <div className="flex items-center justify-between gap-2"><p>{error}</p>{!confirm && state?.retryAction !== "relaunch" && <Button size="xs" variant="outline" disabled={busy || (installing && !state?.canInstall)} onClick={retry}>Retry</Button>}</div>
          {state?.errorDetails && <details className="mt-1 text-caption text-muted-foreground"><summary className="cursor-pointer">Details</summary><p className="mt-1 whitespace-pre-wrap break-words">{state.errorDetails}</p></details>}
        </div>}
        {state?.packageKind === "manual" && state.phase === "available" && <details className="px-2 pb-2 text-caption text-muted-foreground">
          <summary className="cursor-pointer">How to install</summary>
          <div className="mt-1 grid gap-2">
            <p>Quit Silo before installing. Quitting stops local computers.</p>
            <p>On Ubuntu, use Software Updater if you enabled Silo’s software source. If it shows no update, refresh the package list and upgrade Silo in Terminal:</p>
            <code className="whitespace-pre-wrap break-words">sudo apt update &amp;&amp; sudo apt install --only-upgrade silo</code>
            <p>For a manual download, open Assets on GitHub and choose the installer for your system. If the Debian installer does not open, install it in Terminal, replacing the path below with the downloaded file:</p>
            <code className="whitespace-pre-wrap break-words">sudo apt install /path/to/silo.deb</code>
          </div>
        </details>}
        {state?.packageKind === "debian" && state.phase === "available" && <p className="px-2 pb-2 text-caption text-muted-foreground">{state.installBlockReason ?? "Your system will ask for authentication. Silo will update and restart."}</p>}
        {state?.releaseNotes && state.availableVersion && <details className="px-2 pb-2 text-caption text-muted-foreground"><summary className="cursor-pointer">Release notes</summary><p className="mt-1 whitespace-pre-wrap break-words">{state.releaseNotes}</p></details>}
      </div>
      <ListRow icon={<ListRowIcon><RefreshCw aria-hidden="true" className="size-3.5" /></ListRowIcon>} title="Automatically check for updates" detail="Checks after launch, when you return to Silo, and daily. Failed checks retry automatically. You choose when to download and install."
        detailClassName="whitespace-normal" actions={<Switch aria-label="Automatically check for updates" checked={state?.automaticChecks ?? false} disabled={!state || busy} onCheckedChange={updates.setAutomaticChecks} />} />
    </ListCard>
  </section>
}

export function UpdateNotice({ onOpen }: { onOpen: () => void }) {
  const updates = useUpdates()
  const [dismissed, setDismissed] = useState<string | null>(null)
  const state = updates?.snapshot
  if (!state || !["available", "ready"].includes(state.phase) || !state.availableVersion || dismissed === `${state.availableVersion}:${state.phase}`) return null
  return <div role="status" className="pointer-events-auto flex max-w-full items-center gap-2 rounded-lg border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-md">
    <Download className="size-3.5 shrink-0" aria-hidden="true" />
    <span className="min-w-0">Silo {state.availableVersion} {state.phase === "ready" ? "is ready to install." : "is available."}</span>
    <Button size="xs" variant="outline" onClick={onOpen}>View update</Button>
    <Button size="icon-xs" variant="ghost" aria-label="Dismiss update notice" onClick={() => setDismissed(`${state.availableVersion}:${state.phase}`)}><X className="size-3" /></Button>
  </div>
}
