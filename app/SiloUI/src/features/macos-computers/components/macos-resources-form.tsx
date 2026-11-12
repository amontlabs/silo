import { useState } from "react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { showActionFailure } from "@/lib/operation-toast"
import { macosLimits, type MacosComputer, type MacosComputersStore } from "../model/macos-computers"

/**
 * The CPUs and memory of a stopped macOS computer, saved for its next start. The values it was
 * opened with go along as the expected ones, so a change made meanwhile is not overwritten.
 */
export function MacosResourcesForm({ computer, store, onClose }: { computer: MacosComputer; store: MacosComputersStore; onClose: () => void }) {
  const [cpus, setCpus] = useState(String(computer.cpus))
  const [memoryGiB, setMemoryGiB] = useState(String(computer.memoryGiB))
  const [expected] = useState({ cpus: computer.cpus, memoryGiB: computer.memoryGiB, diskGiB: computer.diskGiB })
  const [error, setError] = useState<string | null>(null)
  const [pending, setPending] = useState(false)

  async function save() {
    const parsedCpus = Number(cpus)
    const parsedMemory = Number(memoryGiB)
    if (!Number.isSafeInteger(parsedCpus) || parsedCpus < macosLimits.minCPUs || !Number.isSafeInteger(parsedMemory) || parsedMemory < macosLimits.minMemoryGiB) {
      setError(`Enter at least ${macosLimits.minCPUs} CPUs and ${macosLimits.minMemoryGiB} GiB of memory.`)
      return
    }
    setPending(true)
    try {
      await store.action(computer.id, { "change-resources": { cpus: parsedCpus, memoryGiB: parsedMemory, expected } })
      onClose()
    } catch (cause) {
      showActionFailure(`Could not change CPU and memory for ${computer.name}`, cause, undefined, { native: false })
    } finally {
      setPending(false)
    }
  }

  return <form className="grid gap-3" onSubmit={event => { event.preventDefault(); void save() }}>
    <div className="grid gap-1.5">
      <label htmlFor={`macos-cpus-${computer.id}`} className="font-medium">CPUs</label>
      <Input id={`macos-cpus-${computer.id}`} type="number" min={macosLimits.minCPUs} step={1} required value={cpus} onChange={event => setCpus(event.target.value)} />
    </div>
    <div className="grid gap-1.5">
      <label htmlFor={`macos-memory-${computer.id}`} className="font-medium">Memory (GiB)</label>
      <Input id={`macos-memory-${computer.id}`} type="number" min={macosLimits.minMemoryGiB} step={1} required value={memoryGiB} onChange={event => setMemoryGiB(event.target.value)} />
    </div>
    <p className="text-muted-foreground">Changes apply the next time this computer starts.</p>
    {error && <p role="alert" className="text-destructive select-text">{error}</p>}
    <div className="flex justify-end gap-2">
      <Button type="button" variant="ghost" size="sm" onClick={onClose}>Cancel</Button>
      <Button type="submit" size="sm" disabled={pending}>Save</Button>
    </div>
  </form>
}
