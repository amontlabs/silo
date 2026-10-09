import { useId, useRef, useState, type ReactNode } from "react"
import { Monitor } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import type { DeviceCapacity } from "@/features/computers/model/computer-limits"
import { showActionFailure } from "@/lib/operation-toast"
import {
  macosDefaults,
  validateMacosRequest,
  type MacosComputerRequest,
  type MacosComputersStore,
} from "../model/macos-computers"

export const macosLicenseNotice = "Silo downloads macOS from Apple (about 20 GB) and installs it. Apple's macOS license allows up to two macOS virtual computers per Mac, for software development, testing, or personal non-commercial use. After installation, Silo sets the computer up for computer use, which takes a few minutes."

function parseNumber(text: string) {
  return /^\d+$/.test(text.trim()) ? Number(text.trim()) : Number.NaN
}

/** The fields of a new macOS computer, shown in place of the Linux editor when macOS is chosen as its operating system. */
export function MacosComputerForm({ store, existingNames, capacity, osField, onCancel, onCreated }: {
  store: MacosComputersStore
  existingNames: readonly string[]
  capacity?: DeviceCapacity
  osField: ReactNode
  onCancel: () => void
  onCreated: () => void
}) {
  const [name, setName] = useState("")
  const [cpus, setCpus] = useState(String(Math.min(macosDefaults.cpus, capacity?.logicalCPUs ?? macosDefaults.cpus)))
  const [memory, setMemory] = useState(String(Math.min(macosDefaults.memoryGiB, capacity?.memoryGiB ?? macosDefaults.memoryGiB)))
  const [disk, setDisk] = useState(String(macosDefaults.diskGiB))
  const [saving, setSaving] = useState(false)
  const noticeId = useId()
  const request: MacosComputerRequest = { name, cpus: parseNumber(cpus), memoryGiB: parseNumber(memory), diskGiB: parseNumber(disk) }
  const errors = validateMacosRequest(request, existingNames, { maxCPUs: capacity?.logicalCPUs, maxMemoryGiB: capacity?.memoryGiB })
  const invalid = Object.keys(errors).length > 0
  const nameInput = useRef<HTMLInputElement>(null)

  async function create() {
    setSaving(true)
    try {
      await store.create(request)
      onCreated()
    } catch (error) {
      showActionFailure(`Could not create ${request.name}`, error, undefined, { native: false })
      setSaving(false)
    }
  }

  function field(label: string, key: keyof MacosComputerRequest, value: string, onChange: (value: string) => void, numeric: boolean) {
    // An untouched name is incomplete, not wrong.
    const error = key === "name" && name === "" ? undefined : errors[key]
    const errorId = `${noticeId}-${key}`
    return <label className="grid min-w-0 gap-1 text-[11px] font-medium text-muted-foreground">
      {label}
      <Input technical ref={key === "name" ? nameInput : undefined} aria-label={label} aria-invalid={Boolean(error)} aria-describedby={error ? errorId : undefined} type={numeric ? "number" : "text"} inputMode={numeric ? "numeric" : undefined} maxLength={key === "name" ? 32 : undefined} autoComplete="off" value={value} onChange={event => onChange(event.target.value)} />
      {error && <span id={errorId} className="text-destructive">{error}</span>}
    </label>
  }

  return <div className="grid min-w-0 gap-3 p-3" data-testid="macos-computer-form">
    <div className="flex min-w-0 items-center gap-2">
      <Monitor className="size-4 shrink-0" aria-hidden="true" />
      <span className="min-w-0 flex-1 text-xs font-semibold">Computer details</span>
    </div>
    {osField}
    <fieldset disabled={saving} className="m-0 grid min-w-0 gap-3 border-0 p-0">
      {field("Computer name", "name", name, setName, false)}
      <div className="grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-3">
        {field("CPUs", "cpus", cpus, setCpus, true)}
        {field("Memory (GiB)", "memoryGiB", memory, setMemory, true)}
        {field("Disk (GiB)", "diskGiB", disk, setDisk, true)}
      </div>
      <p className="text-[11px] text-muted-foreground">{macosLicenseNotice}</p>
    </fieldset>
    <div className="flex justify-end gap-2">
      <Button type="button" variant="outline" size="sm" disabled={saving} onClick={onCancel}>Cancel</Button>
      <Button type="button" size="sm" disabled={saving || invalid} onClick={() => void create()}>{saving ? "Creating…" : "Create"}</Button>
    </div>
  </div>
}
