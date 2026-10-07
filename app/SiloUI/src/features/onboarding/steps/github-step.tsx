import { useEffect, useState } from "react"
import { ListRowDetails } from "@/components/list-row"
import { Progress } from "@/components/ui/progress"
import { ActivityOutput } from "@/features/onboarding/components/activity-output"
import type { SiloProgressEvent } from "@/contracts/silo"
import type { ReviewQueueItemView } from "@/features/onboarding/model/onboarding-state"
import { githubFailure } from "@/features/application/pages/github-failure"
import {
  GitHubAccessEditor,
  type GitHubAccessEditorProps,
} from "@/features/github/components/github-access-editor"

export function GitHubStep({ queueItems = [], activityEvents = [], ...props }: GitHubAccessEditorProps & {
  queueItems?: ReviewQueueItemView[]
  activityEvents?: SiloProgressEvent[]
}) {
  const items = queueItems.filter(({ id }) => ["identityRun", "identityVerify", "githubRun", "githubVerify"].includes(id))
  const events = activityEvents.filter(({ phase }) => phase === "github" || phase === "identity")
  const connecting = props.connectionState === "connecting"
  const started = items.some(({ id, status }) => (id === "githubRun" || id === "githubVerify") && status !== "idle")
  const completed = items.filter(({ status }) => status === "succeeded").length
  const failure = items.find(({ status }) => status === "failed")
  const current = items.find(({ status }) => status === "running")
  const [now, setNow] = useState(Date.now)
  const running = Boolean(current)
  useEffect(() => {
    if (!running) return
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [running])
  const latestAttempt = events.at(-1)?.requestId
  const attemptEvents = events.filter(({ requestId }) => requestId === latestAttempt)
  const start = attemptEvents[0]?.timestamp
  const end = current ? now : attemptEvents.at(-1)?.timestamp
  const seconds = start !== undefined && end !== undefined ? Math.max(0, Math.floor((end - start) / 1000)) : 0
  const elapsed = `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`
  const detail = failure ? githubFailure(failure.failure ?? "").message
    : current ? `${current.label}…`
    : completed === items.length && items.length > 0 ? (props.connectionState === "connected" ? "GitHub and Git identity setup complete." : "Git identity setup complete. GitHub skipped.")
    : items.some(({ status }) => status === "queued") ? "Waiting for earlier setup tasks." : "Continue to apply GitHub settings."

  return (
    <section aria-labelledby="github-title" className="flex h-full min-h-0 flex-col">
      <h2 id="github-title" className="sr-only" data-visual-heading="hidden">GitHub</h2>
      <GitHubAccessEditor compactConnection confirmRepositoryClear {...props}
        connectedDetail={started ? detail : props.connectedDetail}
        connectionProgress={started && <>
          <ListRowDetails label="GitHub setup details">
            <Progress value={connecting ? undefined : items.length ? completed / items.length * 100 : undefined} aria-label="GitHub setup progress" />
            <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 text-caption text-muted-foreground" role={failure ? "alert" : "status"} aria-live="polite">
              <span>{connecting ? "Waiting for browser authorization." : `${completed} of ${items.length} operations complete`}</span>
              {!connecting && <span aria-label="Elapsed time" className="shrink-0 font-mono tabular-nums">{elapsed}</span>}
            </div>
          </ListRowDetails>
          <ActivityOutput events={events} embedded />
        </>}
      />
    </section>
  )
}
