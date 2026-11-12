import { useState } from "react"
import { Cpu } from "lucide-react"

import { ConfirmPopover } from "@/components/confirm-popover"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { Spinner } from "@/components/ui/spinner"
import { performMacosAction } from "./macos-detail-menu"
import {
  canRetryMacosSetup,
  isMacosCreating,
  isMacosSettingUp,
  macosStateLabel,
  type MacosComputer,
  type MacosComputersStore,
} from "../model/macos-computers"

/**
 * Where a macOS computer is while it is created or set up, in place of its tabs: the step, its
 * progress, Cancel while that can still be undone, and Retry setup when setup stopped.
 */
export function MacosSetupPanel({ computer, store, unavailable, onDeleted }: { computer: MacosComputer; store: MacosComputersStore; unavailable?: string; onDeleted: () => void }) {
  const [pending, setPending] = useState(false)
  const creating = isMacosCreating(computer)
  const settingUp = isMacosSettingUp(computer)
  const retry = canRetryMacosSetup(computer)
  const failed = computer.state === "failed"
  const title = creating || settingUp ? macosStateLabel(computer) : computer.needsPersonalizing ? "This copy needs setting up again" : failed ? "Setup stopped" : "Setup is not finished"
  const detail = computer.detail ?? (computer.needsPersonalizing ? "It still has its template’s credentials, so it can only be set up again or deleted." : retry ? "Retry setup to finish it, or delete the computer." : undefined)
  const cancelLabel = creating ? "Cancel creating" : "Cancel setting up"

  async function run(action: "setup" | "delete") {
    setPending(true)
    try {
      if (await performMacosAction(store, computer, action) && action === "delete") onDeleted()
    } finally {
      setPending(false)
    }
  }

  return <div className="grid gap-5" data-macos-setup={computer.id}>
    <ListCard>
      <ListRow
        icon={<ListRowIcon aria-hidden="true">{creating || settingUp ? <Spinner /> : <Cpu className="size-3.5" />}</ListRowIcon>}
        title={title}
        detailClassName="whitespace-normal"
        detail={<span className="grid gap-1.5">
          {detail && <span role={failed ? "alert" : undefined}>{detail}</span>}
          {(creating || settingUp) && <Progress value={computer.progress == null ? null : computer.progress * 100} aria-label={`${computer.name} progress`} />}
        </span>}
        actions={<>
          {retry && <Button type="button" variant="outline" size="xs" aria-label={`Retry setup of ${computer.name}`} disabled={pending || Boolean(unavailable)} onClick={() => void run("setup")}>Retry setup</Button>}
          {(creating || settingUp) && <ConfirmPopover
            align="end"
            tone="destructive"
            title={`${cancelLabel} ${computer.name}?`}
            description={creating && computer.state === "copying" ? "The copy stops and the computer is removed." : creating ? "The download and installation stop and the computer is removed." : "The setup stops and the computer is removed."}
            confirmLabel={creating ? "Cancel creation" : "Cancel setup"}
            cancelLabel={creating ? "Keep creating" : "Keep setting up"}
            onConfirm={async () => { await run("delete") }}
          >
            <Button type="button" variant="ghost" size="xs" aria-label={`${cancelLabel} ${computer.name}`} disabled={pending}>Cancel</Button>
          </ConfirmPopover>}
        </>}
      />
    </ListCard>
  </div>
}
