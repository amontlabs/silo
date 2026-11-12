import { useEffect, useState } from "react"
import { GitFork, Terminal } from "lucide-react"
import { SectionHeading } from "@/components/page"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { DisabledReason } from "./disabled-reason"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { formatAbsoluteTime, formatRelativeTime } from "@/lib/relative-time"
import { runCheckpointOperation } from "@/features/application/model/checkpoint-operation-toast"
import { ForkBody } from "./fork-popover"
import { ForkTree } from "./fork-tree"
import type { ApplicationActions, ApplicationComputer } from "@/features/application/model/application-source"
import type { ComputerStep } from "@/desktop/production-schemas"

export type StepsSubject = Pick<ApplicationComputer, "device"> & { configuration: Pick<ApplicationComputer["configuration"], "id" | "name"> }
export type StepsActions = Pick<ApplicationActions, "listComputerSteps" | "readComputerTree" | "forkCheckpoint">

export const noCheckpointReason = "No checkpoint was saved after this step. Run commands with silo exec --step-checkpoints to save one after each step."

export function StepsPanel({ computer, target, actions, disabled, takenNames, forkedAction }: {
  computer: StepsSubject
  target: string
  actions: StepsActions
  disabled: boolean
  takenNames?: readonly string[]
  forkedAction?: (name: string) => { label: string; onClick: () => void }
}) {
  const [steps, setSteps] = useState<ComputerStep[] | null>(null)
  const [failed, setFailed] = useState(false)
  const [openStep, setOpenStep] = useState<number | null>(null)
  const computerId = computer.configuration.id
  const deviceId = computer.device?.id
  const list = actions.listComputerSteps
  useEffect(() => {
    if (!list) return
    let current = true
    void list(computerId, deviceId).then(value => { if (current) { setSteps(value); setFailed(false) } }, () => { if (current) { setSteps([]); setFailed(true) } })
    return () => { current = false }
  }, [list, computerId, deviceId])
  if (!actions.listComputerSteps) return null

  function fork(step: ComputerStep, newName: string) {
    if (disabled || !step.checkpointId || !actions.forkCheckpoint) return
    const checkpointId = step.checkpointId
    setOpenStep(null)
    void runCheckpointOperation({
      id: `step:${target}:${step.number}:fork`,
      kind: "fork",
      target,
      computer: [computer.configuration.name, newName],
      noticeComputer: { id: computerId, name: computer.configuration.name },
      title: `Creating fork ${newName}`,
      run: () => actions.forkCheckpoint!(target, checkpointId, newName),
      success: { title: "Fork created", description: `${newName} is stopped. Start it when you’re ready.`, action: forkedAction?.(newName) },
      failureTitle: `Could not create fork ${newName}`,
    })
  }

  const ordered = [...(steps ?? [])].sort((left, right) => right.number - left.number)
  return <section aria-label={`Steps for ${computer.configuration.name}`} className="grid gap-1.5 text-xs">
    <div className="flex min-h-6 items-center"><SectionHeading>Steps</SectionHeading></div>
    <p className="text-caption text-muted-foreground">Commands run in this computer through Silo. A step with a checkpoint can be forked into a new stopped computer.</p>
    {failed && <p className="text-destructive">Could not read the steps.</p>}
    {steps !== null && ordered.length === 0 ? (
      <ListCard><ListRow
        icon={<ListRowIcon aria-hidden="true"><Terminal className="size-3.5" /></ListRowIcon>}
        title="No steps yet"
        detail="Step checkpoints are off unless a command runs with silo exec --step-checkpoints. Steps are still recorded for commands run with silo exec, but only a step with a checkpoint can be forked."
        detailClassName="whitespace-normal"
      /></ListCard>
    ) : steps !== null && (
      <ListCard divided aria-label="Step history">
        {ordered.map(step => {
          const reason = !step.checkpointId ? noCheckpointReason : undefined
          return <ListRow
            key={step.id}
            data-step-number={step.number}
            detailClassName="whitespace-normal"
            icon={<ListRowIcon aria-hidden="true"><Terminal className="size-3.5" /></ListRowIcon>}
            title={<span className="truncate font-mono" title={step.command}>#{step.number} {step.command}</span>}
            detail={<>
              {step.exitCode === undefined ? "No exit code" : step.exitCode === 0 ? "Exit 0" : <span className="text-destructive">Exit {step.exitCode}</span>}
              {" · "}<time dateTime={step.finishedAt} title={formatAbsoluteTime(step.finishedAt)}>{formatRelativeTime(step.finishedAt) || formatAbsoluteTime(step.finishedAt)}</time>
              {" · "}{step.checkpointId ? "Checkpoint saved" : "No checkpoint"}
            </>}
            actions={<Popover open={openStep === step.number} onOpenChange={open => setOpenStep(open ? step.number : null)}>
              <DisabledReason reason={reason}>
                <PopoverTrigger asChild>
                  <Button size="xs" variant="outline" disabled={disabled || !step.checkpointId || !actions.forkCheckpoint} aria-label={`Fork from step ${step.number}`}><GitFork className="size-3" />Fork from this step</Button>
                </PopoverTrigger>
              </DisabledReason>
              <PopoverContent align="end" className="w-72">
                <ForkBody computerName={computer.configuration.name} title={`Fork from step ${step.number}`} description="Creates a new stopped computer with a copy of this computer’s files after this step." disabled={disabled} takenNames={takenNames} onFork={name => fork(step, name)} onClose={() => setOpenStep(null)} />
              </PopoverContent>
            </Popover>}
          />
        })}
      </ListCard>
    )}
    {actions.readComputerTree && <ForkTree read={actions.readComputerTree} deviceId={deviceId} currentId={computerId} />}
  </section>
}
