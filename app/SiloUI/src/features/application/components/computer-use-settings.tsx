import { useCallback, useMemo, useRef, useSyncExternalStore } from "react"
import { Button } from "@/components/ui/button"
import { SectionHeading } from "@/components/page"
import { Switch } from "@/components/ui/switch"
import { useComputerUseBridge, type ChatGptAppSnapshot, type ChatGptAppStore } from "@/desktop/computer-use-bridge"
import { useSettings } from "@/features/preferences/settings-store"
import { CHATGPT_DOWNLOAD_NOTE } from "@/desktop/computer-use-panel"
import type { ApplicationSource } from "../model/application-source"

const noStatus: ChatGptAppSnapshot = { status: null, busy: false, error: null, loadError: null, subscriptionError: null }

/** The snapshots of several devices' stores together; the array is replaced only when one of them changed. */
function useChatGptApps(stores: Array<ChatGptAppStore | undefined>, active: boolean): ChatGptAppSnapshot[] {
  const last = useRef<ChatGptAppSnapshot[]>([])
  const subscribe = useCallback((listener: () => void) => {
    if (!active) return () => {}
    const stops = stores.map(store => store?.subscribe(listener))
    return () => stops.forEach(stop => stop?.())
  }, [stores, active])
  const getSnapshot = useCallback(() => {
    const next = stores.map(store => store?.getSnapshot() ?? noStatus)
    if (next.length === last.current.length && next.every((snapshot, index) => snapshot === last.current[index])) return last.current
    last.current = next
    return next
  }, [stores])
  return useSyncExternalStore(subscribe, getSnapshot)
}

/** One device whose ChatGPT for Linux download failed or whose status cannot be read. The download itself runs by itself in the background. */
function ComputerUseProblemRow({ name, store, snapshot }: { name: string; store: ChatGptAppStore; snapshot: ChatGptAppSnapshot }) {
  const { status, busy, error, loadError, subscriptionError } = snapshot
  const failed = status?.state === "failed"
  return <li className="flex items-start justify-between gap-3">
    <div className="min-w-0 [overflow-wrap:anywhere]">
      <p className="truncate text-xs font-medium" title={name}>{name}</p>
      {failed && <p role="alert" className="break-words text-xs text-destructive">{status.reason}{status.retryable ? " Silo tries again automatically." : ""}</p>}
      {subscriptionError && <p role="alert" className="break-words text-xs text-destructive">{subscriptionError}</p>}
      {loadError && <p role="alert" className="break-words text-xs text-destructive">{`Silo could not read the computer use status: ${loadError}`}</p>}
      {error && <p role="alert" className="break-words text-xs text-destructive">{error}</p>}
    </div>
    <div className="flex shrink-0 gap-1.5">
      {(loadError || subscriptionError) && <Button size="xs" variant="outline" aria-label={`Refresh ChatGPT for Linux status on ${name}`} onClick={() => { void store.refresh() }}>Refresh</Button>}
      {failed && <Button size="xs" variant="outline" disabled={busy} aria-label={`Retry ChatGPT for Linux on ${name}`} onClick={() => { void store.retry() }}>Retry</Button>}
    </div>
  </li>
}

/** Appears only when a device needs the user: every device prepares ChatGPT for Linux by itself, so nothing shows while that works. */
function ComputerUseProblems({ source, active }: { source: ApplicationSource; active: boolean }) {
  const bridge = useComputerUseBridge()
  const devices = source.devices
  const entries = useMemo(() => bridge ? [
    { key: "local", name: "This device", store: bridge.chatGptFor() },
    // An offline device has no status to read: that is not a problem to act on.
    ...(devices ?? []).filter(device => device.connected).map(device => ({ key: device.id, name: device.name, store: bridge.chatGptFor(device.id) })),
  ] : [], [bridge, devices])
  const snapshots = useChatGptApps(useMemo(() => entries.map(entry => entry.store), [entries]), active)
  const problems = entries.flatMap((entry, index) => {
    const snapshot = snapshots[index] ?? noStatus
    return snapshot.status?.state === "failed" || snapshot.loadError || snapshot.subscriptionError ? [{ ...entry, snapshot }] : []
  })
  if (problems.length === 0) return null
  return <section aria-label="Computer use tools" className="grid gap-3">
    <SectionHeading>Computer use tools</SectionHeading>
    <div className="grid gap-3 rounded-lg border p-3">
      <p className="text-xs text-muted-foreground">{CHATGPT_DOWNLOAD_NOTE}</p>
      <ul aria-label="Devices that need attention" className="grid gap-3">
        {problems.map(problem => <ComputerUseProblemRow key={problem.key} name={problem.name} store={problem.store} snapshot={problem.snapshot} />)}
      </ul>
    </div>
  </section>
}

function NewComputerApprovalSetting() {
  const { settings, updateSettings } = useSettings()
  if (!useComputerUseBridge()) return null
  return <section aria-label="Computer use" className="grid gap-3">
    <SectionHeading>Computer use</SectionHeading>
    <div className="rounded-lg border p-3">
      <div className="flex items-center justify-between gap-4"><div><label htmlFor="computer-use-auto-approval" className="text-xs font-medium">Allow agents to use the desktop without asking in new computers</label><p className="text-xs text-muted-foreground">Claude Code, Codex and similar agents stop asking before using the computer’s desktop. Not a security boundary.</p></div><Switch id="computer-use-auto-approval" checked={settings.computerUseAutoApproval} onCheckedChange={enabled => { void updateSettings({ computerUseAutoApproval: enabled }) }} /></div>
    </div>
  </section>
}


/** The computer use settings: the approval default for new computers, and any device whose computer use components need attention. */
export function ComputerUseSettings({ source, active }: { source: ApplicationSource; active: boolean }) {
  return <>
    <NewComputerApprovalSetting />
    <ComputerUseProblems source={source} active={active} />
  </>
}
