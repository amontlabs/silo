import { ChevronRight, ShieldCheck, X } from "lucide-react"
import { useId, useState, type ReactNode } from "react"

import { InlineAlert } from "@/components/inline-alert"
import { ListCard, ListRowSkeleton } from "@/components/list-row"
import { SectionHeading } from "@/components/page"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import type { NetworkMode, NetworkPolicy, NetworkPolicyState } from "@/desktop/production-schemas"
import type { ApplicationActions } from "@/features/application/model/application-source"

import { addedByReason, hostCount, isStaleSave, modeChoices, timingText, useNetworkPolicy } from "../model/network-policy"

const rowClassName = "flex min-w-0 items-start gap-2 px-3 py-2 text-xs"
const detailsClassName = "grid gap-3 border-t border-border px-3 py-3 text-xs"

function ModeChoices({ draft, disabled, onChange }: { draft: NetworkPolicy; disabled: boolean; onChange: (mode: NetworkMode) => void }) {
  const name = useId()
  return <div role="radiogroup" aria-label="Network mode" className="divide-y divide-border">
    {modeChoices.map(choice => <label key={choice.mode} className={`${rowClassName} ${disabled ? "opacity-60" : "cursor-pointer row-hover"}`}>
      <input type="radio" name={name} value={choice.mode} checked={draft.mode === choice.mode} disabled={disabled} onChange={() => onChange(choice.mode)} className="mt-0.5 accent-primary" />
      <span className="grid gap-0.5">
        <span className="font-medium">{choice.label}</span>
        <span className="text-caption text-muted-foreground">{choice.description}</span>
      </span>
    </label>)}
  </div>
}

function HostList({ hosts, disabled, onChange }: { hosts: readonly string[]; disabled: boolean; onChange: (hosts: string[]) => void }) {
  const [typed, setTyped] = useState("")
  const field = useId()
  const add = () => {
    const host = typed.trim()
    if (!host) return
    if (!hosts.includes(host)) onChange([...hosts, host])
    setTyped("")
  }
  return <section className="grid gap-1.5" aria-label="Allowed hosts">
    <label htmlFor={field} className="font-medium">Hosts you allow</label>
    {hosts.length > 0
      ? <ul className="grid gap-1">{hosts.map(host => <li key={host} className="flex items-center justify-between gap-2 rounded-md bg-muted/40 py-0.5 pr-0.5 pl-2">
        <span className="min-w-0 truncate font-mono" title={host}>{host}</span>
        <Button type="button" variant="ghost" size="icon-xs" aria-label={`Remove ${host}`} disabled={disabled} onClick={() => onChange(hosts.filter(item => item !== host))}><X aria-hidden="true" /></Button>
      </li>)}</ul>
      : <p className="text-caption text-muted-foreground">No hosts yet.</p>}
    <form className="flex items-center gap-2" onSubmit={event => { event.preventDefault(); add() }}>
      <Input id={field} technical size="sm" value={typed} disabled={disabled} placeholder="example.com or *.example.com" className="font-mono" onChange={event => setTyped(event.target.value)} />
      <Button type="submit" variant="outline" size="xs" disabled={disabled || !typed.trim()}>Add</Button>
    </form>
  </section>
}

function PresetList({ state, draft, disabled, onChange }: { state: NetworkPolicyState; draft: NetworkPolicy; disabled: boolean; onChange: (presets: string[]) => void }) {
  const [shown, setShown] = useState<ReadonlySet<string>>(new Set())
  const toggle = (name: string, on: boolean) => onChange(on ? [...draft.presets, name] : draft.presets.filter(item => item !== name))
  const reveal = (name: string) => setShown(current => { const next = new Set(current); if (!next.delete(name)) next.add(name); return next })
  if (state.presets.length === 0) return null
  return <section className="grid gap-1.5" aria-label="Presets">
    <span className="font-medium">Presets</span>
    <ul className="grid gap-1">{state.presets.map(preset => {
      const open = shown.has(preset.name)
      const title = preset.title || preset.name
      return <li key={preset.name} className="grid gap-1">
        <div className="flex items-center justify-between gap-2">
          <label className="flex min-w-0 items-center gap-2">
            <Checkbox aria-label={title} checked={draft.presets.includes(preset.name)} disabled={disabled} onCheckedChange={checked => toggle(preset.name, checked === true)} />
            <span className="truncate">{title}</span>
          </label>
          <Button type="button" variant="ghost" size="xs" aria-expanded={open} aria-label={`${open ? "Hide" : "Show"} the hosts of ${title}`} onClick={() => reveal(preset.name)}>
            <ChevronRight aria-hidden="true" className={open ? "rotate-90" : undefined} />{open ? "Hide hosts" : "Show hosts"}
          </Button>
        </div>
        {open && <p className="pl-6 font-mono text-caption break-words text-muted-foreground">{preset.hosts.join(", ") || "No hosts"}</p>}
      </li>
    })}</ul>
  </section>
}

function PresetUpdates({ state, disabled, dirty, onAccept }: { state: NetworkPolicyState; disabled: boolean; dirty: boolean; onAccept: () => void }) {
  if (state.presetUpdates.length === 0) return null
  return <section className="grid gap-2" aria-label="Preset updates">
    {state.presetUpdates.map(update => {
      const title = update.title || update.name
      return <div key={update.name} className="grid gap-1.5 rounded-md border border-border p-2">
        <div className="flex items-center justify-between gap-2">
          <span className="min-w-0"><span className="font-medium">{title}</span>: This preset now includes {hostCount(update.addedHosts.length)}</span>
          <Button type="button" variant="outline" size="xs" aria-label={`Accept the update to ${title}`} disabled={disabled || dirty} onClick={onAccept}>Accept</Button>
        </div>
        <p className="font-mono text-caption break-words text-muted-foreground">{update.addedHosts.join(", ")}</p>
        {dirty && <p className="text-caption text-muted-foreground">Save or discard your changes first.</p>}
      </div>
    })}
  </section>
}

function AddedBySilo({ state }: { state: NetworkPolicyState }) {
  const added = state.allowedHosts.flatMap(host => { const reason = addedByReason(host.from); return reason ? [{ host: host.host, reason }] : [] })
  return <section className="grid gap-1.5" aria-label="Added by Silo">
    <span className="font-medium">Silo adds these</span>
    {added.length > 0 && <ul className="grid gap-1">{added.map(item => <li key={`${item.host}:${item.reason}`} className="flex items-baseline justify-between gap-3">
      <span className="min-w-0 truncate font-mono" title={item.host}>{item.host}</span>
      <span className="shrink-0 text-caption text-muted-foreground">{item.reason}</span>
    </li>)}</ul>}
    <p className="text-caption text-muted-foreground">Silo's own channel to this computer is always allowed. You cannot remove what is listed here.</p>
  </section>
}

/**
 * A computer's network: how much of it the computer may reach, the hosts and presets of an
 * allowlist, and what Silo adds on its own. A saved change that waits for a restart says so.
 */
export function NetworkPolicySection({ target, computerName, running, readOnly, enabled, refreshKey, actions, restart }: {
  target: string
  computerName: string
  running: boolean
  readOnly: boolean
  /** Whether the section is on screen; the policy is read while it is. */
  enabled: boolean
  refreshKey?: string
  actions: Pick<ApplicationActions, "readNetworkPolicy" | "saveNetworkPolicy">
  /** The control that restarts this computer. */
  restart?: ReactNode
}) {
  const policy = useNetworkPolicy({ read: actions.readNetworkPolicy, save: actions.saveNetworkPolicy, target, enabled, refreshKey })
  const { state, draft } = policy
  const disabled = readOnly || policy.saving
  const timing = state ? timingText(state.timing) : undefined
  const body = state && draft ? <ListCard>
    <ModeChoices draft={draft} disabled={disabled} onChange={mode => policy.setDraft({ ...draft, mode })} />
    {draft.mode === "allowlist" && <div className={detailsClassName}>
      <HostList hosts={draft.hosts} disabled={disabled} onChange={hosts => policy.setDraft({ ...draft, hosts })} />
      <PresetList state={state} draft={draft} disabled={disabled} onChange={presets => policy.setDraft({ ...draft, presets })} />
      <PresetUpdates state={state} disabled={disabled} dirty={policy.dirty} onAccept={() => void policy.submit(state.policy)} />
      <AddedBySilo state={state} />
    </div>}
    {(policy.dirty || policy.saveError) && <div className="flex flex-wrap items-center justify-end gap-2 border-t border-border px-3 py-2">
      {policy.saveError && <p role="alert" className="mr-auto min-w-0 text-xs break-words whitespace-pre-wrap text-destructive">{policy.saveError}</p>}
      {policy.saveError && isStaleSave(policy.saveError)
        ? <Button type="button" size="xs" onClick={() => void policy.reload(true)}>Read again</Button>
        : <>
          <Button type="button" variant="ghost" size="xs" disabled={policy.saving || !policy.dirty} onClick={policy.discard}>Discard</Button>
          <Button type="button" size="xs" disabled={disabled || !policy.dirty} onClick={() => void policy.submit(draft)}>Save</Button>
        </>}
    </div>}
    {timing && !policy.dirty && <div role="status" className="flex flex-wrap items-center justify-between gap-2 border-t border-border px-3 py-2 text-xs text-muted-foreground">
      <span className="flex items-center gap-1.5"><ShieldCheck className="size-3.5 shrink-0" aria-hidden="true" />{timing}</span>
      {state.timing === "nextRestart" && running && restart}
    </div>}
  </ListCard> : null

  return <section className="grid gap-1.5" aria-label="Network">
    <div className="flex min-h-6 items-center justify-between gap-2"><SectionHeading>Network</SectionHeading></div>
    {policy.loadError && <InlineAlert className="flex items-center justify-between gap-3">
      <span>{policy.loadError}</span>
      <Button type="button" size="sm" variant="ghost" onClick={() => void policy.reload()}>Retry</Button>
    </InlineAlert>}
    {policy.loading && <ListCard><ListRowSkeleton label={`Loading the network of ${computerName}`} /></ListCard>}
    {state?.notices.map(notice => <InlineAlert key={notice} tone="warning" role="status">{notice}</InlineAlert>)}
    {body}
  </section>
}
