import { formatMonthDayTime } from "@/lib/format-date"
import { useEffect, useEffectEvent, useState } from "react"
import { History, ShieldCheck, TriangleAlert } from "lucide-react"
import { ActionsMenu } from "@/components/actions-menu"
import { ConfirmBody, ConfirmPopover, FormPopover } from "@/components/confirm-popover"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { TooltipProvider } from "@/components/ui/tooltip"
import { formatAbsoluteTime, formatRelativeTime } from "@/lib/relative-time"
import { runCheckpointOperation } from "@/features/application/model/checkpoint-operation-toast"
import { ForkBody } from "./fork-popover"
import type { ApplicationActions, ApplicationComputer } from "@/features/application/model/application-source"
import type { CheckpointUsage, ComputerCheckpoint } from "@/features/application/model/checkpoint-source"
import { formatBinaryBytes } from "@/lib/format-bytes"

function suggestedName(now = new Date()) {
  return `Checkpoint ${formatMonthDayTime(now)}`
}

function checkpointTag(checkpoint: ComputerCheckpoint) {
  if (checkpoint.reason === "before-restore") return "Recovery"
  return checkpoint.scope === "full" ? "Includes memory" : "Disks only"
}

type CheckpointUsageEntry = CheckpointUsage["checkpoints"][number]

function deleteDescription(checkpoint: ComputerCheckpoint, info: CheckpointUsageEntry | undefined) {
  const recovery = checkpoint.reason === "before-restore" ? "This is the recovery point saved before a Restore; you can no longer undo the Restore it was saved for. " : ""
  const freed = info?.sizeBytes != null ? `, freeing up to ${formatBinaryBytes(info.sizeBytes)}` : ""
  return `${recovery}Its saved state is removed from this device${freed}. This can’t be undone.`
}

export function CheckpointPanel({ computer, target, actions, disabled, onExport, exportDisabled = false, forkedAction, restoredAction, takenNames }: {
  computer: ApplicationComputer
  /** Computer names already used on this computer's device, so a fork name conflict shows inline. */
  takenNames?: readonly string[]
  target: string
  actions: ApplicationActions
  disabled: boolean
  onExport?: (checkpoint: ComputerCheckpoint) => void
  exportDisabled?: boolean
  /** Action on the "Fork created" notification, e.g. Open. Receives the new computer name. */
  forkedAction?: (name: string) => { label: string; onClick: () => void }
  /** Action on the "Restored" notification, e.g. Start. */
  restoredAction?: (checkpoint: ComputerCheckpoint) => { label: string; onClick: () => void }
}) {
  const [createOpen, setCreateOpen] = useState(false)
  const [name, setName] = useState(suggestedName)
  const [pending, setPending] = useState(false)
  /** True once the user started an operation here, so its outcome is reported by a notification rather than an inline label. */
  const [started, setStarted] = useState(false)
  const checkpoints = [...(computer.checkpoints ?? [])].sort((left, right) => right.createdAt.localeCompare(left.createdAt))
  const operation = computer.checkpointOperation
  const running = operation?.status === "running"
  const busy = pending || running
  const locked = disabled || busy
  const isLocal = !computer.device
  const computerName = computer.configuration.name
  const noticeComputer = { id: computer.configuration.id, name: computerName }

  // Sizes and Delete availability come from the native store, so they are read on demand
  // for checkpoints on this device and again after every checkpoint change.
  const [usage, setUsage] = useState<ReadonlyMap<string, CheckpointUsageEntry>>(new Map())
  const [usageRequest, setUsageRequest] = useState(0)
  const canReadUsage = isLocal && Boolean(actions.readCheckpointUsage)
  const usageKey = `${computer.configuration.id}:${checkpoints.map(checkpoint => checkpoint.id).join(",")}:${running}:${usageRequest}`
  const readUsage = useEffectEvent(() => actions.readCheckpointUsage!(computer.configuration.id))
  useEffect(() => {
    if (!canReadUsage || running) return
    let current = true
    void readUsage().then(
      value => { if (current) setUsage(new Map(value.checkpoints.map(entry => [entry.id, entry]))) },
      () => { if (current) setUsage(new Map()) },
    )
    return () => { current = false }
  }, [canReadUsage, running, usageKey])

  async function run(spec: Parameters<typeof runCheckpointOperation>[0]) {
    setPending(true)
    setStarted(true)
    try { return await runCheckpointOperation(spec) } finally { setPending(false) }
  }

  function create() {
    const title = name.trim()
    if (locked || !title || !actions.createCheckpoint) return
    void run({
      id: `checkpoint:${target}:capture`,
      kind: "capture",
      target,
      computer: computerName,
      noticeComputer,
      title: `Creating checkpoint “${title}”`,
      run: () => actions.createCheckpoint!(target, title),
      success: { title: "Checkpoint created", description: title },
      failureTitle: `Could not create checkpoint “${title}”`,
    }).then(ok => { if (ok) setName(suggestedName()) })
  }

  function restore(checkpoint: ComputerCheckpoint) {
    if (locked || !actions.restoreCheckpoint) return
    void run({
      id: `checkpoint:${target}:restore`,
      kind: "restore",
      target,
      computer: computerName,
      noticeComputer,
      title: `Restoring “${checkpoint.name}”`,
      run: () => actions.restoreCheckpoint!(target, checkpoint.id),
      success: { title: `Restored “${checkpoint.name}”`, description: `${computerName} is stopped. A recovery checkpoint was saved first.`, action: restoredAction?.(checkpoint) },
      failureTitle: `Could not restore “${checkpoint.name}”`,
    })
  }

  function fork(checkpoint: ComputerCheckpoint, newName: string) {
    if (locked || !actions.forkCheckpoint) return
    void run({
      id: `checkpoint:${target}:fork`,
      kind: "fork",
      target,
      computer: [computerName, newName],
      noticeComputer,
      title: `Creating fork ${newName}`,
      run: () => actions.forkCheckpoint!(target, checkpoint.id, newName),
      success: { title: "Fork created", description: `${newName} is stopped. Start it when you’re ready.`, action: forkedAction?.(newName) },
      failureTitle: `Could not create fork ${newName}`,
    })
  }

  function remove(checkpoint: ComputerCheckpoint) {
    if (locked || !actions.deleteCheckpoint) return
    void run({
      id: `checkpoint:${target}:delete`,
      kind: "delete",
      target,
      computer: computerName,
      noticeComputer,
      title: `Deleting “${checkpoint.name}”`,
      run: () => actions.deleteCheckpoint!(target, checkpoint.id),
      success: { title: "Checkpoint deleted", description: checkpoint.name },
      failureTitle: `Could not delete “${checkpoint.name}”`,
    }).finally(() => setUsageRequest(request => request + 1))
  }

  function abandon() {
    if (locked || !actions.abandonRestore) return
    void run({
      id: `checkpoint:${target}:restore`,
      kind: "restore",
      target,
      computer: computerName,
      noticeComputer,
      title: "Abandoning Restore",
      run: () => actions.abandonRestore!(target),
      success: { title: "Restore abandoned", description: `${computerName} keeps its current state.` },
      failureTitle: "Could not abandon the Restore",
    })
  }

  const unfinished = computer.unfinishedRestore
  const unfinishedTarget = unfinished ? checkpoints.find(checkpoint => checkpoint.id === unfinished.checkpointId) : undefined
  // A secured Restore whose original computer was already removed finishes on Start.
  const replaced = Boolean(unfinished && computer.pendingCheckpointRestore)
  const unfinishedError = unfinished && operation?.kind === "restore" && operation.status === "failed" ? operation.error ?? operation.stage : null
  // A failure that predates this session is only noted quietly; failures of operations started here are notified.
  const staleFailure = operation?.status === "failed" && !started && !unfinished ? operation.error ?? operation.stage : null

  return <TooltipProvider delayDuration={250}>
    <section aria-label={`Checkpoints for ${computer.configuration.name}`} aria-busy={busy || undefined} className="grid gap-1.5 text-xs">
      <div className="flex min-h-6 items-center justify-between gap-2">
        <h3 className="text-xs font-medium">Checkpoints</h3>
        {actions.createCheckpoint && <FormPopover
          open={createOpen}
          onOpenChange={open => { if (!open || !locked) { setCreateOpen(open); if (open) setName(suggestedName()) } }}
          align="end"
          title="New checkpoint"
          confirmLabel="Create"
          canSubmit={!locked && name.trim().length > 0}
          onSubmit={create}
          fields={<Input aria-label="Checkpoint name" className="h-7 text-xs" maxLength={80} value={name} placeholder="Checkpoint name" onChange={event => setName(event.target.value)} />}
        >
          <Button size="xs" variant="outline" className="shrink-0" disabled={locked}>New checkpoint</Button>
        </FormPopover>}
      </div>
      <p className="text-[11px] text-muted-foreground">Checkpoints let you rewind this computer. Restore replaces its current files; Fork creates a new stopped computer with a copy of its files.</p>

      {staleFailure && <p className="text-muted-foreground">Last checkpoint operation failed: <span className="text-destructive">{staleFailure}</span></p>}

      {unfinished && <div role="group" aria-label="Unfinished Restore" className="grid gap-2 rounded-md border border-border p-2.5">
        <div className="flex items-start gap-2">
          <TriangleAlert aria-hidden="true" className="mt-0.5 size-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
          <div className="grid gap-1">
            <p>
              The Restore to {unfinished.checkpointName ? `“${unfinished.checkpointName}”` : "a checkpoint"} did not finish.{" "}
              {replaced
                ? `Start ${computerName} to finish it.`
                : unfinished.phase === "capturing"
                  ? `Silo had not saved its recovery checkpoint yet, so ${computerName} was not changed.`
                  : `Its recovery checkpoint was saved, but ${computerName} was not replaced yet.`}
            </p>
            {unfinishedError && <p className="text-muted-foreground">Last error: <span className="text-destructive">{unfinishedError}</span></p>}
          </div>
        </div>
        <div className="flex justify-end gap-1">
          {!replaced && isLocal && actions.abandonRestore && <ConfirmPopover
            align="end"
            title="Abandon this Restore?"
            description={`${computerName} keeps its current state and is resumed if the Restore left it paused. A recovery checkpoint that was already saved stays in the list.`}
            confirmLabel="Abandon"
            onConfirm={abandon}
          >
            <Button size="xs" variant="ghost" disabled={locked}>Abandon Restore…</Button>
          </ConfirmPopover>}
          {unfinishedTarget && actions.restoreCheckpoint && <Button size="xs" variant="outline" disabled={locked} onClick={() => restore(unfinishedTarget)}>Retry Restore</Button>}
        </div>
      </div>}

      {checkpoints.length === 0 ? (
        <ListCard>
          <ListRow
            icon={<ListRowIcon aria-hidden="true"><History className="size-3.5" /></ListRowIcon>}
            title="No checkpoints yet"
            detail="Save a checkpoint to rewind or fork this computer later."
            detailClassName="whitespace-normal"
          />
        </ListCard>
      ) : (
        <ListCard divided aria-label="Checkpoint history">
          {checkpoints.map(checkpoint => {
            const Icon = checkpoint.reason === "before-restore" ? ShieldCheck : History
            const info = usage.get(checkpoint.id)
            return <ListRow
              key={checkpoint.id}
              data-checkpoint-name={checkpoint.name}
              detailClassName="whitespace-normal"
              icon={<ListRowIcon aria-hidden="true"><Icon className="size-3.5" /></ListRowIcon>}
              title={<span className="truncate" title={checkpoint.name}>{checkpoint.name}</span>}
              detail={<>
                <time dateTime={checkpoint.createdAt} title={formatAbsoluteTime(checkpoint.createdAt)}>{formatRelativeTime(checkpoint.createdAt) || formatAbsoluteTime(checkpoint.createdAt)}</time>
                {" · "}{checkpointTag(checkpoint)}
                {info?.sizeBytes != null && <>{" · "}{formatBinaryBytes(info.sizeBytes)}</>}
                {info?.usedBy?.length ? <>{" · "}Used by {info.usedBy.join(", ")}</> : null}
                {info?.deleteBlocker && <span className="block text-muted-foreground">{info.deleteBlocker}</span>}
              </>}
              actions={<div className="flex shrink-0 items-center gap-1">
                <ConfirmPopover
                  align="end"
                  title={`Restore “${checkpoint.name}”?`}
                  description={computer.state === "running"
                    ? `${computerName} is running. Silo pauses it, saves a recovery checkpoint that includes its memory (this can use a lot of disk space), force-stops it, then rewinds it. It stays stopped. Changes outside the computer, such as pushed commits or sent requests, are not undone.`
                    : `Silo saves a recovery checkpoint first, then rewinds ${computerName}. It stays stopped. Changes outside the computer, such as pushed commits or sent requests, are not undone.`}
                  confirmLabel="Restore"
                  onConfirm={() => restore(checkpoint)}
                >
                  <Button size="xs" variant="outline" disabled={locked || !actions.restoreCheckpoint}>Restore</Button>
                </ConfirmPopover>
                <ActionsMenu
                  label={`Checkpoint actions for ${checkpoint.name}`}
                  disabled={locked}
                  popovers={{
                    fork: close => <ForkBody computerName={computerName} title={`Fork from “${checkpoint.name}”`} description="Creates a new stopped computer with a copy of this computer’s files at this checkpoint. Select Start when ready." disabled={locked} takenNames={takenNames} onFork={newName => fork(checkpoint, newName)} onClose={close} />,
                    delete: close => <ConfirmBody tone="destructive" title={`Delete “${checkpoint.name}”?`} description={deleteDescription(checkpoint, info)} confirmLabel="Delete" onConfirm={() => remove(checkpoint)} onClose={close} />,
                  }}
                  items={[
                    ...(actions.forkCheckpoint ? [{ label: "Fork…", accessibleLabel: `Fork ${checkpoint.name}`, disabled: locked, popover: "fork" }] : []),
                    ...(isLocal && onExport ? [{ label: "Export…", accessibleLabel: `Export ${checkpoint.name}`, disabled: locked || exportDisabled, onSelect: () => onExport(checkpoint) }] : []),
                    // Delete runs on this device only; a pinned checkpoint says what still needs it.
                    ...(isLocal && actions.deleteCheckpoint ? [{ label: "Delete…", accessibleLabel: `Delete ${checkpoint.name}`, destructive: true, separatorBefore: true, disabled: locked || Boolean(info?.deleteBlocker), tooltip: info?.deleteBlocker, popover: "delete" }] : []),
                  ]}
                />
              </div>}
            />
          })}
        </ListCard>
      )}

      {!actions.createCheckpoint && <p className="text-muted-foreground">Checkpoints cannot be created or restored here. Use the Silo desktop app instead.</p>}

    </section>
  </TooltipProvider>
}
