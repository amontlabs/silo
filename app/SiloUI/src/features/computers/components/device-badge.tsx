import { formatLocalTimestamp } from "@/lib/format-date"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import type { ComputerDevice } from "@/features/application/model/connections"

export function DeviceBadge({ device }: { device: ComputerDevice }) {
  const detail = `${device.name} · ${device.busy ? "Updating…" : device.connected ? "Connected" : "Offline · last known status"} · ${device.address}${!device.connected && device.lastSeen ? ` · Last seen ${formatLocalTimestamp(device.lastSeen)}` : ""}`
  return <TooltipProvider><Tooltip><TooltipTrigger asChild><span role="note" tabIndex={0} aria-label={`Computer on ${detail}`} className="inline-flex shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-0.5 text-caption font-medium text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">{device.name}</span></TooltipTrigger><TooltipContent>{detail}</TooltipContent></Tooltip></TooltipProvider>
}
