import { useEffect, useId } from "react"

import { Switch } from "@/components/ui/switch"
import { approvalStatus, useComputerUseApproval } from "./computer-use-approval"

/**
 * Body of the "Created {name}" notification: the computer's computer use approval as an
 * "Allow without asking" switch. The caller only mounts it for a computer known to have built-in
 * computer use, so the switch shows at once (disabled) while the setting is read. A freshly
 * created computer is stopped and reports its computer use as "unavailable" until its first
 * start, but its approval is stored now and applied at start, so that state keeps the switch.
 * It disappears only when the computer reports no computer use at all.
 */
export function CreatedComputerApprovalSwitch({ computer }: { computer: string }) {
  const approval = useComputerUseApproval(computer, 5000, false)
  const switchId = useId()
  const refresh = approval?.refresh
  useEffect(() => { refresh?.() }, [refresh])

  if (!approval || (approval.state && !approval.computerUse)) return null
  const computerUse = approval.computerUse
  const status = computerUse ? approvalStatus(computerUse, approval.running) : null
  return <div className="grid gap-1 text-xs">
    <div className="flex items-center justify-between gap-3">
      <label htmlFor={switchId}>Allow without asking</label>
      <span className="flex shrink-0 items-center gap-2">
        {status?.applying && <span role="status" className="text-caption text-muted-foreground">Applying…</span>}
        <Switch id={switchId} checked={computerUse?.approval === "auto"} disabled={!computerUse || approval.busy || computerUse.approval === "unknown"}
          onCheckedChange={checked => approval.setApproval(checked ? "auto" : "ask")} />
      </span>
    </div>
    {(approval.error ?? (!computerUse ? approval.loadError : null)) && <span role="alert" className="text-destructive">{approval.error ?? approval.loadError}</span>}
  </div>
}
