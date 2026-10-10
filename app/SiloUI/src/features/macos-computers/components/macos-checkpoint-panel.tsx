import { useMemo } from "react"

import { CheckpointPanel, type CheckpointActions, type CheckpointSubject } from "@/features/application/components/checkpoint-panel"
import { canCheckpointMacos, type MacosComputer, type MacosComputersStore } from "../model/macos-computers"

const forkDescription = "Creates a new stopped macOS computer with a copy of this checkpoint’s disk. Memory can’t move to a new computer, so a fork always starts from the disk. Silo gives it its own password and keys, which takes about a minute."

/**
 * The Linux checkpoint list and actions for a macOS computer. The list component reads a
 * computer shaped like a Linux one, so this only translates: the same Create, Restore, Fork
 * and Delete, backed by the macOS checkpoint commands.
 */
export function MacosCheckpointPanel({ computer, store, takenNames }: { computer: MacosComputer; store: MacosComputersStore; takenNames: readonly string[] }) {
  const subject = useMemo<CheckpointSubject>(() => ({
    configuration: { id: computer.id, name: computer.name },
    state: computer.state === "running" ? "running" : "stopped",
    checkpoints: computer.checkpoints ?? [],
    checkpointOperation: computer.checkpointOperation ?? null,
  }), [computer.id, computer.name, computer.state, computer.checkpoints, computer.checkpointOperation])
  const actions = useMemo<CheckpointActions>(() => ({
    createCheckpoint: (_target, name) => store.createCheckpoint(computer.id, name),
    restoreCheckpoint: (_target, checkpointId) => store.restoreCheckpoint(computer.id, checkpointId),
    forkCheckpoint: async (_target, checkpointId, newName) => {
      if (!checkpointId) throw new Error("Choose a checkpoint to fork.")
      await store.forkCheckpoint(computer.id, checkpointId, newName)
    },
    deleteCheckpoint: (_target, checkpointId) => store.deleteCheckpoint(computer.id, checkpointId),
  }), [store, computer.id])
  const pending = computer.pendingRestore
  const pendingName = pending ? computer.checkpoints?.find(checkpoint => checkpoint.id === pending.checkpointId)?.name : undefined
  const startAction = { label: "Start", onClick: () => { void store.action(computer.id, "start").catch(() => undefined) } }

  return <div className="grid gap-2" data-macos-checkpoints={computer.id}>
    {pending && computer.state !== "running" && <p className="text-xs text-muted-foreground">
      Restored{pendingName ? ` to “${pendingName}”` : ""}. Start the computer to {pending.memory ? "continue from its saved memory (if this Mac still can; otherwise it boots from the disk)" : "boot from the restored disk"}.
    </p>}
    <CheckpointPanel
      computer={subject}
      target={`macos:${computer.id}`}
      actions={actions}
      takenNames={takenNames}
      disabled={!canCheckpointMacos(computer)}
      forkCopy={{ description: forkDescription, created: name => `${name} is being set up and can be started when that finishes.` }}
      restoredAction={() => startAction}
    />
  </div>
}
