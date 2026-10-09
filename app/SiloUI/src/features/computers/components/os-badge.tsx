import { cn } from "@/lib/utils"

export type ComputerOs = "linux" | "macos"

const labels: Record<ComputerOs, string> = { linux: "Linux", macos: "macOS" }

/** Names the operating system a computer runs, in the same pill as the device badge. */
export function OsBadge({ os, className }: { os: ComputerOs; className?: string }) {
  return <span data-slot="os-badge" data-os={os} className={cn("inline-flex shrink-0 items-center rounded-full bg-muted px-1.5 py-0.5 text-[9px] font-medium text-muted-foreground", className)}>{labels[os]}</span>
}

/** Chooses the operating system of a new computer; the fields below follow it. */
export function OperatingSystemField({ value, macosSupported, disabled, onChange }: { value: ComputerOs; macosSupported: boolean; disabled?: boolean; onChange: (os: ComputerOs) => void }) {
  return <label className="grid gap-1 text-[11px] text-muted-foreground">
    Operating system
    <select aria-label="Operating system" className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground" value={value} disabled={disabled} onChange={event => onChange(event.target.value as ComputerOs)}>
      <option value="linux">{labels.linux}</option>
      {macosSupported && <option value="macos">{labels.macos}</option>}
    </select>
  </label>
}
