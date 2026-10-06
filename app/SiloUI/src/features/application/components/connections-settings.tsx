import { useEffect, useRef, useState } from "react"
import { CopyButton } from "@/components/copy-button"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { showActionFailure } from "@/lib/operation-toast"
import { restoreFocus } from "@/lib/focus"
import type { ApplicationActions, ApplicationSource } from "../model/application-source"
import type { ConnectionsStatus } from "../model/connections"

// The backend refuses to move a saved device to a new address unless the user confirms.
const alreadySaved = "is already saved at"

export function ConnectDeviceForm({ connect, authorize, setupKey, onClose }: { setupKey?: (address: string) => Promise<void>; authorize?: (address: string) => Promise<void>; connect: (address: string, options?: { replaceAddress?: boolean }) => Promise<void>; onClose: () => void }) {
  const [address, setAddress] = useState("")
  const [operation, setOperation] = useState<"connect" | "authorize" | "setupKey" | null>(null)
  const busy = operation !== null
  // Connection failures stay inline: the form is where the user corrects the address, and the
  // SSH recovery actions below only make sense next to the error. Onboarding has no toaster.
  const [error, setError] = useState("")
  const mounted = useRef(false)
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])
  async function attempt(action: () => Promise<void>, kind: "connect" | "authorize" | "setupKey" = "connect") {
    setOperation(kind)
    try { await action() } catch (cause) { if (mounted.current) setError(cause instanceof Error ? cause.message : String(cause)) } finally { if (mounted.current) setOperation(null) }
  }
  return <form aria-label="Connect device" className="grid gap-3 rounded-lg border p-3" onKeyDown={event => { if (event.key === "Escape" && !event.defaultPrevented && !event.nativeEvent.isComposing && !busy) { event.preventDefault(); onClose() } }} onSubmit={async event => {
    event.preventDefault()
    if (!address.trim() || busy) return
    setError("")
    await attempt(async () => { await connect(address.trim()); if (mounted.current) onClose() })
  }}>
    <label className="grid gap-1 text-xs">Device address<Input technical autoFocus aria-label="Device address" placeholder="user@device or SSH alias" value={address} disabled={busy} onChange={event => setAddress(event.target.value)} /></label>
    <p className="text-xs text-muted-foreground">Open Silo on that device and turn on Connections. Uses your existing SSH keys and configuration.</p>
    {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
    {error.includes(alreadySaved) && <div className="grid justify-items-start gap-1"><Button type="button" variant="outline" size="sm" disabled={busy || !address.trim()} onClick={() => { void attempt(async () => { await connect(address.trim(), { replaceAddress: true }); if (mounted.current) onClose() }) }}>Use this address</Button><p className="text-xs text-muted-foreground">Only if that device now uses this address. Its computers and connections stay as they are.</p></div>}
    {error && !error.includes(alreadySaved) && authorize && <div className="grid justify-items-start gap-1"><Button type="button" variant="outline" size="sm" disabled={busy || !address.trim()} onClick={() => { void attempt(() => authorize(address.trim()), "authorize") }}>{operation === "authorize" ? "Opening Terminal…" : "Authorize SSH in Terminal…"}</Button><p className="text-xs text-muted-foreground">Confirm the device’s fingerprint and unlock your SSH key, then connect again.</p></div>}
    {error && !error.includes(alreadySaved) && setupKey && <div className="grid justify-items-start gap-1"><Button type="button" variant="outline" size="sm" disabled={busy || !address.trim()} onClick={() => { void attempt(() => setupKey(address.trim()), "setupKey") }}>{operation === "setupKey" ? "Setting up SSH key…" : "Set up Silo SSH key…"}</Button><p className="text-xs text-muted-foreground">Adds Silo’s public SSH key to your account on the other device. You may be asked for its password. Then connect again.</p></div>}
    <div className="flex justify-end gap-2"><Button type="button" size="sm" variant="ghost" disabled={busy} onClick={onClose}>Cancel</Button><Button size="sm" disabled={busy || !address.trim()}>{operation === "connect" ? "Connecting…" : "Connect"}</Button></div>
  </form>
}

const addressKinds = { name: "Local network name", tailscale: "Tailscale", network: "Local network" } as const

// A host name alone often does not resolve from another device, so every candidate is offered.
function ManagementAddresses({ management }: { management: ConnectionsStatus }) {
  const addresses = management.addresses?.length ? management.addresses : [{ address: management.address, kind: "name" as const }]
  return <div className="grid gap-1">
    <p className="text-xs text-muted-foreground">Other devices can connect using one of these addresses.</p>
    <ul aria-label="Addresses for other devices" className="grid gap-1">
      {addresses.map(entry => <li key={entry.address} className="flex items-center justify-between gap-2">
        <span className="min-w-0"><code className="select-text break-all text-xs">{entry.address}</code> <span className="text-xs text-muted-foreground">{addressKinds[entry.kind]}</span></span>
        <CopyButton size="xs" variant="outline" value={entry.address} labels={{ idle: `Copy ${entry.address}`, copied: "Address copied", failed: "Copy failed" }} text={{ idle: "Copy", copied: "Copied", failed: "Copy failed" }} />
      </li>)}
    </ul>
  </div>
}

export function ConnectionsSettings({ source, actions }: { source: ApplicationSource; actions: ApplicationActions; active?: boolean }) {
  return <div className="grid gap-6">
    {actions.connectDevice && <DevicesSection source={source} actions={actions} />}
  </div>
}

function DevicesSection({ source, actions }: { source: ApplicationSource; actions: ApplicationActions }) {
  const [connecting, setConnecting] = useState(false)
  const [busy, setBusy] = useState(false)
  const connectButton = useRef<HTMLButtonElement>(null)
  const shouldRestoreFocus = useRef(false)
  useEffect(() => {
    if (!connecting && shouldRestoreFocus.current) {
      shouldRestoreFocus.current = false
      restoreFocus(connectButton.current)
    }
  }, [connecting])
  function closeConnectionForm() {
    shouldRestoreFocus.current = true
    setConnecting(false)
  }
  const pending = useRef(false)
  const unmounts = useRef(0)
  const generations = useRef(new Map<string, number>())
  useEffect(() => () => { unmounts.current++ }, [])
  async function perform(resource: string, operation: () => Promise<void>) {
    if (pending.current) return
    const epoch = unmounts.current
    const generation = (generations.current.get(resource) ?? 0) + 1
    generations.current.set(resource, generation)
    const current = () => epoch === unmounts.current && generation === generations.current.get(resource)
    pending.current = true
    setBusy(true)
    try { await operation() } catch (cause) {
      if (epoch === unmounts.current) showActionFailure("Device setting not changed", cause, () => { if (current()) void perform(resource, operation) }, { native: false })
    } finally { if (epoch === unmounts.current) { pending.current = false; setBusy(false) } }
  }
  const removeDevice = actions.removeDevice
  if (!actions.connectDevice) return null
  return <section aria-label="Connections" className="grid gap-3">
    <h2 className="text-xs font-medium">Connections</h2>
    <div className="grid gap-3 rounded-lg border p-3">
      <div className="flex items-center justify-between gap-4"><div><label htmlFor="connections-enabled" className="text-xs font-medium">Allow connections from other devices</label><p className="text-xs text-muted-foreground">Let devices with SSH access to your account manage these computers while Silo is running.</p><p className="text-xs text-muted-foreground">Quit stops local computers and disconnects other devices.</p></div><Switch id="connections-enabled" checked={source.connections?.enabled ?? false} disabled={busy || !source.connections || !actions.setConnectionsEnabled} onCheckedChange={enabled => { void perform("connections-enabled", () => actions.setConnectionsEnabled!(enabled)) }} /></div>
      {source.connections?.error && <p role="alert" className="text-xs text-destructive">{source.connections.error}</p>}
      {source.connections?.enabled && <p className="text-xs text-muted-foreground">Enable Remote Login on macOS or the SSH server on Linux so other devices can connect.</p>}
      {source.connections?.enabled && <ManagementAddresses management={source.connections} />}
      {source.devices?.map(device => <div key={device.id} className="flex items-center justify-between gap-3 border-t pt-3"><div className="min-w-0 [overflow-wrap:anywhere]"><p className="truncate text-xs font-medium" title={device.name}>{device.name}</p><p className="text-xs text-muted-foreground">{device.busy ? "Updating…" : device.connected ? "Connected" : "Offline · last known status"} · {device.address}</p>{device.error && <p className="text-xs text-destructive">{device.error}</p>}<p className="text-xs text-muted-foreground">Removing the connection leaves computers on {device.name} unchanged.</p></div><Button size="xs" variant="ghost" disabled={busy || !removeDevice} aria-label={`Remove connection to ${device.name}`} onClick={() => { if (removeDevice) void perform(`device:${device.id}`, () => removeDevice(device.id)) }}>Remove connection</Button></div>)}
      {!connecting && <Button ref={connectButton} size="sm" variant="outline" className="justify-self-start" onClick={() => setConnecting(true)}>Connect device…</Button>}
      {connecting && <ConnectDeviceForm connect={actions.connectDevice} authorize={actions.authorizeDevice} setupKey={actions.setupDeviceKey} onClose={closeConnectionForm} />}
      {source.devicesError && <p role="alert" className="text-xs text-destructive">{source.devicesError}</p>}
      {source.connectionsError && <p role="alert" className="text-xs text-destructive">{source.connectionsError}</p>}
    </div>
  </section>
}
