import { useState } from "react"
import { KeyRound } from "lucide-react"

import { CopyButton } from "@/components/copy-button"
import { ListRow, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { DisabledReason } from "@/features/application/components/disabled-reason"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { errorMessage } from "@/lib/error-message"

type NameState = { status: "idle" } | { status: "loading" } | { status: "ready"; alias: string } | { status: "failed"; message: string }

/**
 * The SSH host name (key the line by the computer so another computer starts clean) desktop apps on this device use to reach a computer (an agent's desktop
 * app, T3 Code, an editor). It is the computer's alias that Open editor also uses; making it
 * adds the alias to the SSH configuration, so it is made when asked for, not when the page opens.
 * Only a running computer can be asked.
 */
export function SshNameLine({ target, running, ensure }: { target: string; running: boolean; ensure: NonNullable<ApplicationActions["ensureSshName"]> }) {
  const [state, setState] = useState<NameState>({ status: "idle" })

  async function show() {
    setState({ status: "loading" })
    try {
      const named = await ensure(target)
      setState({ status: "ready", alias: named.alias })
    } catch (cause) {
      setState({ status: "failed", message: errorMessage(cause) })
    }
  }

  const detail = state.status === "failed" ? state.message
    : state.status === "ready" ? "Use this host name in a desktop app that connects over SSH. Silo added it to your SSH configuration."
      : "For desktop apps that connect over SSH, such as an agent's desktop app, T3 Code or your editor."
  return <ListRow
    icon={<ListRowIcon aria-hidden="true"><KeyRound className="size-3.5" /></ListRowIcon>}
    role="group"
    aria-label="SSH name for desktop apps"
    title={<>SSH name on this device{state.status === "ready" && <code className="select-text truncate rounded-sm bg-muted px-1 font-mono text-caption font-normal">{state.alias}</code>}</>}
    detail={detail}
    detailClassName={state.status === "failed" ? "whitespace-normal break-words text-destructive" : "whitespace-normal"}
    actions={state.status === "ready"
      ? <CopyButton variant="outline" size="xs" value={state.alias} labels={{ idle: "Copy SSH name", copied: "SSH name copied", failed: "Copy failed" }} text={{ idle: "Copy", copied: "Copied", failed: "Copy failed" }} />
      : <DisabledReason reason={running ? undefined : "Start the computer to get its SSH name."}>
        <Button type="button" variant="outline" size="xs" disabled={!running || state.status === "loading"} onClick={() => void show()}>
          {state.status === "loading" ? <><Spinner size="sm" data-icon="inline-start" />Preparing…</> : state.status === "failed" ? "Try again" : "Show SSH name"}
        </Button>
      </DisabledReason>}
  />
}
