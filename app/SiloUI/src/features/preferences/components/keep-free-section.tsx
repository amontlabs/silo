import { HardDrive, MemoryStick } from "lucide-react"
import { useEffect, useId, useRef, useState } from "react"

import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { SectionHeading } from "@/components/page"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import type { DeviceResources } from "@/desktop/production-schemas"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { useSettings } from "@/features/preferences/settings-store"
import { cn } from "@/lib/utils"

const GIB = 2 ** 30
const gib = (bytes: number) => `${Number((bytes / GIB).toFixed(1))} GiB`

/** One floor: the number of GiB typed, or empty for the default. */
function FloorField({ label, caption, minimum, saved, effective, onSave }: {
  label: string
  caption: string
  minimum: number
  saved: number | null
  /** What applies now, in bytes, once the engine has said. */
  effective?: number
  onSave: (gib: number | null) => void
}) {
  const [typed, setTyped] = useState(saved === null ? "" : String(saved))
  const [problem, setProblem] = useState<string | null>(null)
  const lastCommitted = useRef<number | null>(saved)
  const message = useId()
  function commit() {
    const text = typed.trim()
    if (text === "") {
      setProblem(null)
      if (lastCommitted.current !== null) {
        lastCommitted.current = null
        onSave(null)
      }
      return
    }
    const value = Number(text)
    if (!Number.isInteger(value) || value < minimum) { setProblem(`Enter a whole number of GiB, at least ${minimum}.`); return }
    setProblem(null)
    if (value !== lastCommitted.current) {
      lastCommitted.current = value
      onSave(value)
    }
  }
  const now = effective === undefined ? "" : saved === null ? ` Now ${gib(effective)} (the default).` : ` Now ${gib(effective)}.`
  return <ListRow
    className="row-hover"
    icon={<ListRowIcon aria-hidden="true">{label === "Memory" ? <MemoryStick className="size-3.5" /> : <HardDrive className="size-3.5" />}</ListRowIcon>}
    title={<h4>{label}</h4>}
    detail={<>{caption}{now}{problem && <span id={message} role="alert" className="block text-destructive">{problem}</span>}</>}
    detailClassName="whitespace-normal"
    actions={<div className="flex shrink-0 items-center gap-1.5">
      <Input technical size="sm" inputMode="numeric" className="w-20 text-right" aria-label={`${label} to keep free, in GiB`} aria-describedby={problem ? message : undefined} aria-invalid={problem ? true : undefined}
        placeholder="Default" value={typed} onChange={event => setTyped(event.target.value)} onBlur={commit} onKeyDown={event => { if (event.key === "Enter") commit() }} />
      <Button type="button" variant="ghost" size="xs" disabled={saved === null} aria-label={`Use the default ${label.toLowerCase()} to keep free`} onClick={() => { setProblem(null); lastCommitted.current = null; onSave(null) }}>Default</Button>
    </div>}
  />
}

/**
 * The memory and disk this device keeps free: below them, an agent inside a computer cannot start
 * or create another one. Empty fields use the defaults, which the device's own state sentence
 * shows alongside what applies now.
 */
export function KeepFreeSection({ read }: { read?: ApplicationActions["readDeviceResources"] }) {
  const { settings, updateSettings } = useSettings()
  const [resources, setResources] = useState<DeviceResources>()
  const { memoryFloorGib, diskFloorGib } = settings
  useEffect(() => {
    if (!read) return
    let current = true
    read().then(next => { if (current) setResources(next) }, () => {})
    return () => { current = false }
  }, [read, memoryFloorGib, diskFloorGib])
  if (!read) return null
  return <section className="grid gap-2" aria-label="Keep free for this device">
    <SectionHeading>Keep free for this device</SectionHeading>
    <ListCard divided>
      <FloorField key={`memory:${memoryFloorGib}`} label="Memory" minimum={1} saved={memoryFloorGib} effective={resources?.memoryFloorBytes || undefined}
        caption="An agent inside a computer cannot start or create another one when less memory than this is free."
        onSave={value => void updateSettings({ memoryFloorGib: value })} />
      <FloorField key={`disk:${diskFloorGib}`} label="Disk" minimum={2} saved={diskFloorGib} effective={resources?.diskFloorBytes || undefined}
        caption="An agent inside a computer cannot start or create another one when less disk than this is free."
        onSave={value => void updateSettings({ diskFloorGib: value })} />
      {resources?.message && <ListRow icon={null} title={<span className="sr-only">State</span>} detail={<span role="status" className={cn(resources.state !== "ok" && "text-warning", resources.state === "critical" && "text-destructive")}>{resources.message}</span>} detailClassName="whitespace-normal" />}
    </ListCard>
  </section>
}
