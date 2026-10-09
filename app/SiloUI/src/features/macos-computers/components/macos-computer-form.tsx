import { useId, type ReactNode } from "react"
import { Monitor } from "lucide-react"

import { ConfirmPopover } from "@/components/confirm-popover"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import type { DeviceCapacity } from "@/features/computers/model/computer-limits"
import { showActionFailure } from "@/lib/operation-toast"
import {
  macosRequestFrom,
  validateMacosRequest,
  type MacosComputerRequest,
  type MacosFormFields,
  type MacosTemplate,
} from "../model/macos-computers"

export const macosLicenseNotice = "Silo downloads macOS from Apple (about 20 GB) and installs it. Apple's macOS license allows up to two macOS virtual computers per Mac, for software development, testing, or personal non-commercial use. After installation, Silo sets the computer up for computer use, which takes a few minutes."

/** The fields of a new macOS computer, shown in place of the Linux editor when macOS is chosen as its operating system. The host owns the values so they survive navigation. */
export function MacosComputerForm({ fields, onChange, creating, existingNames, otherNames, capacity, template, minDiskGiB, onRemoveTemplate, osField, onCancel, onCreate }: {
  fields: MacosFormFields
  onChange: (changes: Partial<MacosFormFields>) => void
  /** A creation is in flight: every field stays locked. */
  creating: boolean
  existingNames: readonly string[]
  /** This device's Linux computers, whose names a macOS computer cannot reuse. */
  otherNames: readonly string[]
  capacity?: DeviceCapacity
  /** The set-up computer new ones are copied from, if there is one. */
  template?: MacosTemplate | null
  /** A copy keeps its template's disk size. */
  minDiskGiB?: number
  onRemoveTemplate?: () => Promise<void>
  osField: ReactNode
  onCancel: () => void
  onCreate: (request: MacosComputerRequest) => void
}) {
  const noticeId = useId()
  const request = macosRequestFrom(fields)
  const errors = validateMacosRequest(request, existingNames, { maxCPUs: capacity?.logicalCPUs, maxMemoryGiB: capacity?.memoryGiB, minDiskGiB, otherNames })
  const invalid = Object.keys(errors).length > 0

  function field(label: string, key: keyof MacosFormFields, numeric: boolean) {
    // An untouched name is incomplete, not wrong.
    const error = key === "name" && fields.name === "" ? undefined : errors[key]
    const errorId = `${noticeId}-${key}`
    return <label className="grid min-w-0 gap-1 text-[11px] font-medium text-muted-foreground">
      {label}
      <Input technical aria-label={label} aria-invalid={Boolean(error)} aria-describedby={error ? errorId : undefined} type={numeric ? "number" : "text"} inputMode={numeric ? "numeric" : undefined} maxLength={key === "name" ? 32 : undefined} autoComplete="off" value={fields[key]} onChange={event => onChange({ [key]: event.target.value })} />
      {error && <span id={errorId} className="text-destructive">{error}</span>}
    </label>
  }

  return <div className="grid min-w-0 gap-3 p-3" data-testid="macos-computer-form">
    <div className="flex min-w-0 items-center gap-2">
      <Monitor className="size-4 shrink-0" aria-hidden="true" />
      <span className="min-w-0 flex-1 text-xs font-semibold">Computer details</span>
    </div>
    {osField}
    <fieldset disabled={creating} className="m-0 grid min-w-0 gap-3 border-0 p-0">
      {field("Computer name", "name", false)}
      <div className="grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-3">
        {field("CPUs", "cpus", true)}
        {field("Memory (GiB)", "memoryGiB", true)}
        {field("Disk (GiB)", "diskGiB", true)}
      </div>
      <p className="text-[11px] text-muted-foreground">{macosLicenseNotice}</p>
      {template && <div className="grid justify-items-start gap-1 text-[11px] text-muted-foreground" data-testid="macos-template-notice">
        <p>{template.current
          ? `New macOS computers are copied from a set-up template (macOS ${template.macosVersion}); the first takes about 15 minutes, later ones about a minute.`
          : `A template from an earlier setup (macOS ${template.macosVersion}) is kept but no longer used.`}</p>
        {onRemoveTemplate && <ConfirmPopover
          align="start"
          tone="destructive"
          title="Remove the template?"
          description="New macOS computers are installed from scratch again. Computers already created are not affected."
          confirmLabel="Remove template"
          cancelLabel="Keep template"
          onConfirm={async () => { try { await onRemoveTemplate() } catch (error) { showActionFailure("Could not remove the template", error, undefined, { native: false }) } }}
        >
          <Button type="button" variant="ghost" size="xs">Remove template</Button>
        </ConfirmPopover>}
      </div>}
    </fieldset>
    <div className="flex justify-end gap-2">
      <Button type="button" variant="outline" size="sm" disabled={creating} onClick={onCancel}>Cancel</Button>
      <Button type="button" size="sm" disabled={creating || invalid} onClick={() => onCreate(request)}>{creating ? "Creating…" : "Create"}</Button>
    </div>
  </div>
}
