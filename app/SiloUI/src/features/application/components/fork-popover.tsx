import { useId, useState } from "react"

import { FormBody } from "@/components/confirm-popover"
import { Input } from "@/components/ui/input"
import { validateComputerName } from "@/features/onboarding/model/computer-configuration"

/** The fork form, shown in an `ActionsMenu` popover (`popover: "fork"`): asks for the new computer name. Owns the name so it resets each time it opens. */
export function ForkBody({ computerName, title, description, disabled = false, takenNames = [], onFork, onClose }: {
  computerName: string
  /** Defaults to "Fork <computerName>"; a checkpoint fork names the source checkpoint. */
  title?: string
  description?: string
  disabled?: boolean
  /** Computer names already used on the device that will own the fork. */
  takenNames?: readonly string[]
  onFork: (name: string) => void | Promise<void>
  onClose: () => void
}) {
  const [name, setName] = useState("")
  const errorID = useId()
  const trimmed = name.trim()
  const nameError = trimmed.length === 0
    ? undefined
    : validateComputerName(trimmed) ?? (takenNames.includes(trimmed) ? `A computer named ${trimmed} already exists.` : undefined)
  return <FormBody
    title={title ?? `Fork ${computerName}`}
    description={description ?? `Creates a new stopped computer with a copy of this computer’s files. Silo adds a “Fork point” checkpoint to ${computerName}’s history, and a running ${computerName} pauses briefly while it is saved.`}
    confirmLabel="Fork"
    canSubmit={!disabled && trimmed.length > 0 && !nameError}
    onSubmit={() => onFork(trimmed)}
    onClose={onClose}
    fields={<>
      <Input size="sm" technical aria-label="New computer name" aria-invalid={Boolean(nameError)} aria-describedby={nameError ? errorID : undefined} maxLength={32} value={name} placeholder="New computer name" onChange={event => setName(event.target.value)} />
      {nameError && <p id={errorID} className="text-xs text-destructive">{nameError}</p>}
    </>}
  />
}
