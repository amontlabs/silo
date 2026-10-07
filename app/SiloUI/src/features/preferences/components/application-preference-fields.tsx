import { useEffect, useId, useLayoutEffect, useRef, useState } from "react"
import { Code2, Compass, SquareTerminal } from "lucide-react"

import { ListRow, ListRowIcon } from "@/components/list-row"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import type { ApplicationPreferenceSelection } from "@/features/preferences/model/application-preferences"
import { matchesApplication, useApplications, type ApplicationKind } from "@/features/preferences/application-catalog"
import { errorMessage } from "@/lib/error-message"

const chooseApplication = "__silo-choose-application__"
const unavailableApplication = "__silo-unavailable-application__"
const systemDefaultApplication = "__silo-system-default-application__"
const applicationNoun: Record<ApplicationKind, string> = { terminal: "terminal", editor: "code editor", browser: "browser" }

function ApplicationOptionLabel({ kind, name, icon }: { kind: ApplicationKind; name: string; icon?: string }) {
  const Fallback = kind === "terminal" ? SquareTerminal : kind === "editor" ? Code2 : Compass
  return <span className="flex min-w-0 items-center gap-2">
    {icon ? <img src={icon} alt="" aria-hidden="true" draggable={false} className="size-4 shrink-0 object-contain" /> : <Fallback className="size-4 shrink-0" aria-hidden="true" />}
    <span className="truncate">{name}</span>
  </span>
}

function ApplicationPreferenceRow({
  icon: Icon,
  title,
  description,
  error,
  errorId,
  control,
  compact,
}: {
  icon: typeof Compass
  title: string
  description: string
  error?: string
  errorId: string
  control: React.ReactNode
  compact: boolean
}) {
  return (
    <ListRow
      className={compact ? "hover:bg-muted/35 focus-within:bg-muted/35" : "gap-3 px-0 py-3 first:pt-0 last:pb-0"}
      icon={compact ? <ListRowIcon aria-hidden="true"><Icon className="size-3.5" /></ListRowIcon> : <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />}
      title={<div className={compact ? undefined : "text-sm"}>{title}</div>}
      detail={<>{description}{error && <span id={errorId} role="alert" className="mt-0.5 block text-destructive">{error}</span>}</>}
      detailClassName={compact ? "whitespace-normal" : "mt-0.5 whitespace-normal text-xs"}
      actions={<div className={compact ? "w-40 max-w-[45%] shrink-0" : "w-48 shrink-0"}>{control}</div>}
    />
  )
}

export function ApplicationPreferenceFields({
  value,
  onChange,
  compact = false,
}: {
  value: ApplicationPreferenceSelection
  compact?: boolean
  onChange: (value: ApplicationPreferenceSelection) => void
}) {
  const { catalog, refresh, choose, available, loaded } = useApplications()
  const errorIdPrefix = useId()
  // A failed pick keeps the previous choice; say so beside the select rather than only
  // in the console, and clear it on the next change of that application.
  const [failures, setFailures] = useState<Partial<Record<ApplicationKind, string>>>({})
  const requests = useRef({ terminal: 0, editor: 0, browser: 0 })
  const latest = useRef({ value, onChange })
  useLayoutEffect(() => { latest.current = { value, onChange } })
  useEffect(() => {
    const active = requests.current
    return () => { active.terminal++; active.editor++; active.browser++ }
  }, [])
  const errorId = (kind: ApplicationKind) => `${errorIdPrefix}-${kind}-error`

  async function update(kind: ApplicationKind, selection: string) {
    const request = ++requests.current[kind]
    const publish = (patch: Partial<ApplicationPreferenceSelection>) => {
      const next = { ...latest.current.value, ...patch }
      latest.current.value = next
      latest.current.onChange(next)
    }
    setFailures(({ [kind]: _cleared, ...rest }) => rest)
    try {
      if (selection === systemDefaultApplication) {
        publish({ [`${kind}UseSystemDefault`]: true })
        return
      }
      const application = selection === chooseApplication
        ? await choose(kind)
        : catalog[kind].find(({ path }) => path === selection)
      if (request !== requests.current[kind]) return
      if (application) publish({ [kind]: application.name, [`${kind}Path`]: application.path, [`${kind}UseSystemDefault`]: false })
    } catch (error) {
      if (request !== requests.current[kind]) return
      const detail = errorMessage(error).trim()
      setFailures((current) => ({ ...current, [kind]: `Could not use the chosen ${applicationNoun[kind]}.${detail ? ` ${/[.!?]$/.test(detail) ? detail : `${detail}.`}` : ""}` }))
    }
  }

  function applicationSelect(kind: ApplicationKind, label: string) {
    const savedPath = value[`${kind}Path`]
    const selected = catalog[kind].find((application) => savedPath ? application.path === savedPath : matchesApplication(application, value[kind]))
    const useSystemDefault = value[`${kind}UseSystemDefault`] === true
    const systemDefault = catalog[kind].find(({ path }) => path === catalog.defaults[kind])
    return (
      <Select
        value={useSystemDefault ? systemDefaultApplication : selected?.path ?? unavailableApplication}
        disabled={!loaded}
        onValueChange={(selection) => { void update(kind, selection) }}
        onOpenChange={(open) => { if (open) void refresh().catch((error: unknown) => console.error("Silo application discovery:", error)) }}
      >
        <SelectTrigger className={compact ? "h-7 text-[11px]" : undefined} aria-label={label} aria-invalid={failures[kind] ? true : undefined} aria-describedby={failures[kind] ? errorId(kind) : undefined}>
          <SelectValue>{useSystemDefault ? <ApplicationOptionLabel kind={kind} name={systemDefault ? `${systemDefault.name} (default)` : "System default (not set)"} icon={systemDefault?.icon} /> : undefined}</SelectValue>
        </SelectTrigger>
        <SelectContent className="w-max min-w-[var(--radix-select-trigger-width)] max-w-[min(24rem,var(--radix-select-content-available-width))]">
          <SelectItem value={systemDefaultApplication} disabled={!systemDefault}><ApplicationOptionLabel kind={kind} name={`System default (${systemDefault?.name ?? "not set"})`} icon={systemDefault?.icon} /></SelectItem>
          {loaded && !useSystemDefault && !selected && <SelectItem value={unavailableApplication} disabled><ApplicationOptionLabel kind={kind} name={`${value[kind]} (unavailable)`} /></SelectItem>}
          {catalog[kind].map((application) => <SelectItem key={application.path} value={application.path}><ApplicationOptionLabel kind={kind} name={application.name} icon={application.icon} /></SelectItem>)}
          <SelectItem value={chooseApplication} disabled={!available}>Choose…</SelectItem>
        </SelectContent>
      </Select>
    )
  }

  return (
    <>
      <ApplicationPreferenceRow
        compact={compact}
        icon={SquareTerminal}
        title="Terminal"
        error={failures.terminal}
        errorId={errorId("terminal")}
        description="Used by computer terminal shortcuts."
        control={applicationSelect("terminal", "Terminal")}
      />
      <ApplicationPreferenceRow
        compact={compact}
        icon={Code2}
        title="Code editor"
        error={failures.editor}
        errorId={errorId("editor")}
        description="Used when opening computer files."
        control={applicationSelect("editor", "Code editor")}
      />
      <ApplicationPreferenceRow
        compact={compact}
        icon={Compass}
        title="Browser"
        error={failures.browser}
        errorId={errorId("browser")}
        description="Used when opening computer URLs."
        control={applicationSelect("browser", "Browser")}
      />
    </>
  )
}
