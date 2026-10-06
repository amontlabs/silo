import { useEffect, useId, useRef, useState } from "react"
import { KeyRound, LoaderCircle } from "lucide-react"

import { FilterCombobox } from "@/components/filter-combobox"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import type { ApplicationSecret, ApplicationSource, SecretConfigurationRequest } from "../model/application-source"
import { secretConfiguration, type SecretDraft, type SecretValidationErrors } from "../model/secret-configuration"

export function SecretEditor({ secret, source, onSave, onCancel, saving = false, saveError, initialComputers }: {
  secret?: ApplicationSecret
  source: ApplicationSource
  onSave: (request: SecretConfigurationRequest) => void
  onCancel: () => void
  saving?: boolean
  saveError?: string
  /** Preselected computers when adding a new secret (e.g. scoped to one computer on its page). */
  initialComputers?: string[]
}) {
  const [baseline, setBaseline] = useState(secret)
  const [draft, setDraft] = useState<SecretDraft>(() => ({
    name: secret?.name ?? "", value: "", computers: secret?.computers ?? initialComputers ?? [],
    domains: secret?.allowedDomains.join(", ") ?? "", allowAnyDomain: secret?.allowedDomains.includes("*") ?? false,
  }))
  const [errors, setErrors] = useState<SecretValidationErrors>({})
  const formRef = useRef<HTMLFormElement>(null)
  const id = useId()
  const computers = source.computers.filter(w => !w.device)
  const title = secret ? `Edit ${secret.name}` : "Add secret"
  const settingsChanged = Boolean(secret && baseline && (
    secret.computers.length !== baseline.computers.length || !secret.computers.every(name => baseline.computers.includes(name))
    || secret.allowedDomains.length !== baseline.allowedDomains.length || !secret.allowedDomains.every(domain => baseline.allowedDomains.includes(domain))
  ))
  const anyDomain = draft.domains.split(/[\s,]+/).includes("*")

  useEffect(() => {
    formRef.current?.querySelector<HTMLInputElement>("input:not(:disabled)")?.focus()
  }, [])
  useEffect(() => {
    formRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus()
  }, [errors])

  function update(changes: Partial<SecretDraft>) {
    setDraft((current) => ({ ...current, ...changes }))
    setErrors({})
  }

  function fieldError(field: keyof SecretDraft) {
    return errors[field] && <span id={`${id}-${field}-error`} className="text-[11px] text-destructive" role="alert">{errors[field]}</span>
  }

  return <form ref={formRef} aria-label={title} className="grid min-w-0 gap-3 p-3" noValidate onSubmit={(event) => {
    event.preventDefault()
    if (saving || settingsChanged) return
    const result = secretConfiguration(draft, source.secrets, computers.map(({ configuration }) => configuration.name), secret)
    if (result.errors) {
      setErrors(result.errors)
      return
    }
    const request = result.request
    const unchanged = secret && request.value === undefined
      && request.computers.length === secret.computers.length && request.computers.every((name) => secret.computers.includes(name))
      && request.allowedDomains.length === secret.allowedDomains.length && request.allowedDomains.every((domain) => secret.allowedDomains.includes(domain))
    if (unchanged) onCancel()
    else onSave(request)
  }} onKeyDown={(event) => {
    if (!saving && event.key === "Escape" && !event.defaultPrevented && !event.nativeEvent.isComposing) {
      event.preventDefault()
      event.stopPropagation()
      onCancel()
    }
  }}>
    <h3 className="flex min-w-0 items-center gap-2 text-xs font-semibold"><KeyRound className="size-4 shrink-0" aria-hidden="true" /><span className="break-all">{title}</span></h3>
    {settingsChanged && <div className="grid gap-2 rounded-md bg-warning/10 p-2.5 text-[11px]">
      <p role="alert">This secret changed while you were editing. Reload its current settings before saving.</p>
      <Button type="button" variant="outline" size="sm" disabled={saving} onClick={() => {
        if (!secret) return
        setBaseline(secret)
        update({ name: secret.name, computers: [...secret.computers], domains: secret.allowedDomains.join(", "), allowAnyDomain: secret.allowedDomains.includes("*") })
      }}>Reload settings</Button>
    </div>}
    <fieldset disabled={saving || settingsChanged} className="grid min-w-0 gap-3">
    <div className="grid min-w-0 gap-3 sm:grid-cols-2">
      <div className="grid content-start gap-1">
        <label htmlFor={`${id}-name`} className="text-[11px] font-medium text-muted-foreground">Name</label>
        <Input technical id={`${id}-name`} value={draft.name} disabled={Boolean(secret)} autoComplete="off" spellCheck={false} autoCapitalize="off" className="font-mono text-xs md:text-xs" placeholder="SERVICE_TOKEN" aria-invalid={Boolean(errors.name)} aria-describedby={errors.name ? `${id}-name-error` : undefined} onChange={(event) => update({ name: event.target.value })} />
        {fieldError("name")}
      </div>
      <div className="grid content-start gap-1">
        <label htmlFor={`${id}-value`} className="text-[11px] font-medium text-muted-foreground">{secret ? "Replacement value" : "Value"}</label>
        <Input technical id={`${id}-value`} type="password" value={draft.value} autoComplete="new-password" spellCheck={false} autoCapitalize="off" className="text-xs md:text-xs" aria-invalid={Boolean(errors.value)} aria-describedby={errors.value ? `${id}-value-error` : secret ? `${id}-value-hint` : undefined} onChange={(event) => update({ value: event.target.value })} />
        {secret && <p id={`${id}-value-hint`} className="text-[11px] text-muted-foreground">Leave blank to keep the current value.</p>}
        {fieldError("value")}
      </div>
    </div>
    <fieldset className="grid min-w-0 gap-2">
      <legend className="mb-2 text-[11px] font-medium text-muted-foreground">Computers</legend>
      <FilterCombobox
        options={computers.map(({ configuration }) => ({ value: configuration.name, label: configuration.name }))}
        selectedValues={new Set(draft.computers)}
        onChange={(values) => update({ computers: [...values] })}
        label="Secret computers"
        inputLabel="Add computer"
        placeholder="Select computers…"
        listLabel="Available computers"
        selectedLabel="Selected computers"
        emptyMessage={computers.length === 0 ? "Add a computer to assign secrets." : "No computers available."}
        inputInvalid={Boolean(errors.computers)}
        inputDescribedBy={errors.computers ? `${id}-computers-error` : undefined}
      />
      {fieldError("computers")}
    </fieldset>
    <div className="grid gap-1">
      <label htmlFor={`${id}-domains`} className="text-[11px] font-medium text-muted-foreground">Allowed domains</label>
      <Input technical id={`${id}-domains`} value={draft.domains} autoComplete="off" spellCheck={false} autoCapitalize="off" className="text-xs md:text-xs" placeholder="api.example.com, *.example.com" aria-invalid={Boolean(errors.domains)} aria-describedby={`${id}-domains-hint${errors.domains ? ` ${id}-domains-error` : ""}`} onChange={(event) => update({ domains: event.target.value, allowAnyDomain: false })} />
      <p id={`${id}-domains-hint`} className="text-[11px] text-muted-foreground">Separate hosts with commas. Use * to allow any HTTPS destination.</p>
      {fieldError("domains")}
    </div>
    {anyDomain && <div className="grid gap-2 rounded-md bg-warning/10 p-2.5 text-[11px] text-warning">
      <p>Any HTTPS server could receive this secret.</p>
      <label className="flex items-center gap-2"><Checkbox checked={draft.allowAnyDomain} aria-invalid={Boolean(errors.allowAnyDomain)} aria-describedby={errors.allowAnyDomain ? `${id}-allowAnyDomain-error` : undefined} onCheckedChange={(checked) => update({ allowAnyDomain: checked === true })} />Allow any HTTPS destination</label>
      {fieldError("allowAnyDomain")}
    </div>}
    </fieldset>
    {saveError && <p role="alert" className="text-[11px] text-destructive">{saveError}</p>}
    <div className="flex justify-end gap-2">
      <Button type="button" variant="outline" size="sm" disabled={saving} onClick={onCancel}>Cancel</Button>
      <Button type="submit" size="sm" disabled={saving || settingsChanged}>{saving && <LoaderCircle className="animate-spin" aria-hidden="true" />}{saving ? "Saving…" : saveError ? "Retry" : "Save"}</Button>
    </div>
  </form>
}
