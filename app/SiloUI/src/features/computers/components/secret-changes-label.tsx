import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { ComputerState } from "@/features/application/model/application-source"

export function SecretChangesLabel({ computer, state, secrets, inline = false }: { computer: string; state: ComputerState; secrets: string[]; inline?: boolean }) {
  const stopped = state === "stopped"
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          role="note"
          tabIndex={0}
          aria-label={stopped ? `Secret changes apply on next start for ${computer}` : `Restart required for ${computer}`}
          className={inline
            ? "cursor-help rounded-sm text-warning focus-ring"
            : "shrink-0 cursor-help rounded-full bg-warning/10 px-1.5 py-0.5 text-caption font-medium text-warning focus-ring"}
        >
          {stopped ? "Applies on next start" : "Restart required"}
        </span>
      </TooltipTrigger>
      <TooltipContent className="break-words">
        {stopped ? "Start" : "Restart"} {computer} to apply secret changes: {secrets.join(", ")}.
      </TooltipContent>
    </Tooltip>
  )
}
