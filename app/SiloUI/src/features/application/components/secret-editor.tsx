import { useEffect, useId, useRef, useState } from "react"
import { KeyRound } from "lucide-react"

import { FilterCombobox } from "@/components/filter-combobox"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import type { ApplicationSecret, ApplicationSource, SecretConfigurationRequest, SecretKind } from "../model/application-source"
import { secretConfiguration, sharedHostingWarnings, type SecretDraft, type SecretValidationErrors } from "../model/secret-configuration"

export function SecretEditor({ secret, source, onSave, onCancel, saving = false, saveError, initialComputers, kind: initialKind }: {
  secret?: ApplicationSecret
  source: ApplicationSource
  onSave: (request: SecretConfigurationRequest) => void
  onCancel: () => void
  saving?: boolean
  saveError?: string
  /** Preselected computers when adding a new secret (e.g. scoped to one computer on its page). */
  initialComputers?: string[]
  /** What kind of secret to add; an existing secret keeps its own. */
  kind?: SecretKind
}) {
  const kind = secret?.kind ?? initialKind ?? "generic"
  const token = kind === "github-token"
  const [baseline, setBaseline] = useState(secret)
  const [draft, setDraft] = useState<SecretDraft>(() => ({
    kind, name: secret?.name ?? "", value: "", computers: secret?.computers ?? initialComputers ?? [],
    domains: secret?.allowedDomains.join(", ") ?? "", allowAnyDomain: secret?.allowedDomains.includes("*") ?? false,
    allowSharedHosting: false,
  }))
  const [errors, setErrors] = useState<SecretValidationErrors>({})
  // A GitHub token leaves the form once it is submitted; after a failed save it is asked for again.
  const [valueSubmitted, setValueSubmitted] = useState(false)
  const formRef = useRef<HTMLFormElement>(null)
  const id = useId()
  const computers = source.computers.filter(w => !w.device)
  const title = secret ? `Edit ${secret.name}` : token ? "Add GitHub token" : "Add secret"
  const settingsChanged = Boolean(secret && baseline && !token && (
    secret.computers.length !== baseline.computers.length || !secret.computers.every(name => baseline.computers.includes(name))
    || secret.allowedDomains.length !== baseline.allowedDomains.length || !secret.allowedDomains.every(domain => baseline.allowedDomains.includes(domain))
  ))
  const domainList = draft.domains.split(/[\s,]+/).filter(Boolean).map((domain) => domain.toLowerCase())
  const anyDomain = domainList.includes("*")
  const sharedHosting = sharedHostingWarnings([...new Set(domainList)])

  useEffect(() => {
    formRef.current?.querySelector<HTMLInputElement>("input:not(:disabled)")?.focus()
  }, [])
  useEffect(() => {
    formRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus()
  }, [errors])

  function update(changes: Partial<SecretDraft>) {
    setDraft((current) => ({ ...current, ...changes }))
    setErrors({})
    if (changes.value) setValueSubmitted(false)
  }

  function fieldError(field: keyof SecretDraft) {
    return errors[field] && <span id={`${id}-${field}-error`} className="text-caption text-destructive" role="alert">{errors[field]}</span>
  }

  return <form ref={formRef} aria-label={title} className="grid min-w-0 gap-3 p-3" noValidate onSubmit={(event) => {
    event.preventDefault()
    if (saving || settingsChanged) return
    if (token && valueSubmitted && draft.value === "") {
      setErrors({ value: "Enter the token again to retry." })
      return
    }
    const result = secretConfiguration(draft, source.secrets, computers.map(({ configuration }) => configuration.name), secret)
    if (result.errors) {
      setErrors(result.errors)
      return
    }
    const request = result.request
    const unchanged = secret && request.value === undefined && !request.allowSharedHosting
      && (token || (request.computers.length === secret.computers.length && request.computers.every((name) => secret.computers.includes(name))
      && request.allowedDomains.length === secret.allowedDomains.length && request.allowedDomains.every((domain) => secret.allowedDomains.includes(domain))))
    if (unchanged) {
      onCancel()
      return
    }
    if (token && request.value !== undefined) {
      setDraft((current) => ({ ...current, value: "" }))
      setValueSubmitted(true)
    }
    onSave(request)
  }} onKeyDown={(event) => {
    if (!saving && event.key === "Escape" && !event.defaultPrevented && !event.nativeEvent.isComposing) {
      event.preventDefault()
      event.stopPropagation()
      onCancel()
    }
  }}>
    <h3 className="flex min-w-0 items-center gap-2 text-xs font-semibold"><KeyRound className="size-4 shrink-0" aria-hidden="true" /><span className="break-all">{title}</span></h3>
    {settingsChanged && <div className="grid gap-2 rounded-md bg-warning/10 p-2.5 text-caption">
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
        <label htmlFor={`${id}-name`} className="text-caption font-medium text-muted-foreground">Name</label>
        <Input technical id={`${id}-name`} value={draft.name} disabled={Boolean(secret)} autoComplete="off" spellCheck={false} autoCapitalize="off" className="font-mono" placeholder={token ? "work" : "SERVICE_TOKEN"} aria-invalid={Boolean(errors.name)} aria-describedby={errors.name ? `${id}-name-error` : undefined} onChange={(event) => update({ name: event.target.value })} />
        {fieldError("name")}
      </div>
      <div className="grid content-start gap-1">
        <label htmlFor={`${id}-value`} className="text-caption font-medium text-muted-foreground">{token ? (secret ? "Replacement token" : "Token") : secret ? "Replacement value" : "Value"}</label>
        <Input technical id={`${id}-value`} type="password" value={draft.value} autoComplete="new-password" spellCheck={false} autoCapitalize="off" aria-invalid={Boolean(errors.value)} aria-describedby={errors.value ? `${id}-value-error` : secret ? `${id}-value-hint` : undefined} onChange={(event) => update({ value: event.target.value })} />
        {secret && <p id={`${id}-value-hint`} className="text-caption text-muted-foreground">{token ? "Leave blank to keep the current token." : "Leave blank to keep the current value."}</p>}
        {fieldError("value")}
      </div>
    </div>
    {token ? <p className="rounded-md bg-warning/10 p-2.5 text-caption text-warning">
      Silo checks the token with GitHub and shows the account it belongs to. Choose which computers use it on the GitHub page. A computer using it can do everything the token allows, with no repository limits from Silo. Silo sends it only to GitHub and never shows it inside the computer.
    </p> : <>
    <fieldset className="grid min-w-0 gap-2">
      <legend className="mb-2 text-caption font-medium text-muted-foreground">Computers</legend>
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
      <label htmlFor={`${id}-domains`} className="text-caption font-medium text-muted-foreground">Allowed domains</label>
      <Input technical id={`${id}-domains`} value={draft.domains} autoComplete="off" spellCheck={false} autoCapitalize="off" placeholder="api.example.com, *.example.com" aria-invalid={Boolean(errors.domains)} aria-describedby={`${id}-domains-hint${errors.domains ? ` ${id}-domains-error` : ""}`} onChange={(event) => update({ domains: event.target.value, allowAnyDomain: false })} />
      <p id={`${id}-domains-hint`} className="text-caption text-muted-foreground">Separate hosts with commas. Use * to allow any HTTPS destination.</p>
      {fieldError("domains")}
    </div>
    {anyDomain && <div className="grid gap-2 rounded-md bg-warning/10 p-2.5 text-caption text-warning">
      <p>Any HTTPS server could receive this secret.</p>
      <label className="flex items-center gap-2"><Checkbox checked={draft.allowAnyDomain} aria-invalid={Boolean(errors.allowAnyDomain)} aria-describedby={errors.allowAnyDomain ? `${id}-allowAnyDomain-error` : undefined} onCheckedChange={(checked) => update({ allowAnyDomain: checked === true })} />Allow any HTTPS destination</label>
      {fieldError("allowAnyDomain")}
    </div>}
    {sharedHosting.length > 0 && <div className="grid gap-2 rounded-md bg-warning/10 p-2.5 text-caption text-warning">
      {sharedHosting.map((warning) => <p key={warning}>{warning}</p>)}
      <label className="flex items-center gap-2"><Checkbox checked={draft.allowSharedHosting} onCheckedChange={(checked) => update({ allowSharedHosting: checked === true })} />Let computers reach every site under these</label>
    </div>}
    </>}
    </fieldset>
    {saveError && <p role="alert" className="text-caption text-destructive">{saveError}</p>}
    <div className="flex justify-end gap-2">
      <Button type="button" variant="outline" size="sm" disabled={saving} onClick={onCancel}>Cancel</Button>
      <Button type="submit" size="sm" disabled={saving || settingsChanged}>{saving && <Spinner />}{saving ? "Saving…" : saveError ? "Retry" : "Save"}</Button>
    </div>
  </form>
}
