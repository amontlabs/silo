import { cn } from "@/lib/utils"

export type ComputerOs = "linux" | "macos" | "container"

const labels: Record<ComputerOs, string> = { linux: "Linux", macos: "macOS", container: "Container" }

/** Shown on a container computer, which shares the host's kernel. */
export const containerWarning = "Less isolated than a VM"

/** Said on a container computer's page when it has a desktop: its browser runs without its own sandbox. */
export const containerBrowserWarning = "This computer shares the host's kernel, and its browser runs without its sandbox. A web page that compromises the browser reaches everything in this computer: it can act with this computer's GitHub access and secrets, and use its silo client."

export function ContainerBrowserNotice({ className }: { className?: string }) {
  return <p role="note" data-slot="container-browser-notice" className={cn("rounded-lg bg-warning/15 px-3 py-2 text-caption text-warning", className)}>{containerBrowserWarning}</p>
}

/** Names the operating system a computer runs, in the same pill as the device badge. A container computer says it is less isolated. */
export function OsBadge({ os, className }: { os: ComputerOs; className?: string }) {
  const container = os === "container"
  return <span
    data-slot="os-badge"
    data-os={os}
    title={container ? containerWarning : undefined}
    aria-label={container ? `${labels.container}. ${containerWarning}` : undefined}
    className={cn("inline-flex shrink-0 items-center rounded-full bg-muted px-1.5 py-0.5 text-caption font-medium text-muted-foreground", container && "bg-warning/15 text-warning", className)}
  >{labels[os]}</span>
}

/** Chooses the operating system of a new computer; the fields below follow it. A container computer is offered only for a Linux host, after the others. */
export function OperatingSystemField({ value, macosSupported, containerSupported = false, disabled, onChange }: { value: ComputerOs; macosSupported: boolean; containerSupported?: boolean; disabled?: boolean; onChange: (os: ComputerOs) => void }) {
  return <label className="grid gap-1 text-caption text-muted-foreground">
    Operating system
    <select aria-label="Operating system" className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground" value={value} disabled={disabled} onChange={event => onChange(event.target.value as ComputerOs)}>
      <option value="linux">{labels.linux}</option>
      {macosSupported && <option value="macos">{labels.macos}</option>}
      {containerSupported && <option value="container">{labels.container} ({containerWarning.toLowerCase()})</option>}
    </select>
  </label>
}
