import { useCallback, useEffect, useRef, useState } from "react"

import type { ApplicationSecret, ApplicationSource, ApplicationComputer, SecretConfigurationRequest } from "@/features/application/model/application-source"
import type { ComputerCheckpoint } from "@/features/application/model/checkpoint-source"
import { remoteComputerTarget, computerTarget } from "@/features/application/model/connections"

function fixtureCheckpoint(computer: ApplicationComputer, name: string, reason: ComputerCheckpoint["reason"]): ComputerCheckpoint {
  return { id: `c${crypto.randomUUID().replaceAll("-", "").slice(0, 31)}`, name, createdAt: new Date().toISOString(), scope: computer.pendingCheckpointRestore?.state ?? (computer.state === "running" ? "full" : "disk"), reason }
}

function pendingComputer(computer: ApplicationComputer): ApplicationComputer {
  return {
    configuration: computer.configuration, device: computer.device, purpose: "Local MicroSandbox",
    state: "stopped", stateDetail: "Ready to start from checkpoint", freshness: "fresh", settling: false,
    canDismissError: false,
    repositories: [], files: [], ports: [], logs: [], githubRepositories: computer.githubRepositories,
    secretNames: computer.secretNames, pendingSecretRevocations: [], checkpoints: [],
    checkpointOperation: null, unfinishedRestore: null,
  }
}

export function useApplicationFixture(source: ApplicationSource) {
  const [computerSnapshot, setComputerSnapshot] = useState(source.computers)
  const [computers, setComputers] = useState(source.computers)
  const checkpointMembers = useRef(new Map<string, string>())
  useEffect(() => { checkpointMembers.current.clear() }, [source.computers])
  // Native status snapshots clone the source; only changed metadata resets local edits.
  const incomingSecrets = JSON.stringify(source.secrets)
  const [secretSnapshot, setSecretSnapshot] = useState(incomingSecrets)
  const [secrets, setSecrets] = useState(source.secrets)

  if (computerSnapshot !== source.computers) {
    setComputerSnapshot(source.computers)
    setComputers(source.computers)
  }
  if (secretSnapshot !== incomingSecrets) {
    setSecretSnapshot(incomingSecrets)
    setSecrets(source.secrets)
  }

  const onRestoreComplete = useCallback((targetName: string) => {
    setComputers((current) => {
      const sourceComputer = current[0]
      if (!sourceComputer || current.some(({ configuration }) => configuration.name === targetName)) return current
      return [...current, {
        ...sourceComputer,
        configuration: { ...sourceComputer.configuration, id: crypto.randomUUID(), name: targetName },
        state: "stopped",
        stateDetail: "Restored and verified",
        attention: undefined,
      }]
    })
  }, [])
  const removeSecret = useCallback((id: string) => {
    setSecrets((current) => current.filter((secret) => secret.id !== id))
  }, [])

  const createCheckpoint = useCallback(async (target: string, name: string) => {
    const label = name.trim()
    const characters = [...label]
    if (!label || characters.length > 80 || characters.some((character) => {
      const code = character.codePointAt(0)!
      return code < 32 || (code >= 127 && code <= 159)
    })) throw "Checkpoint name must contain 1 to 80 printable characters."
    const selected = computers.find((computer) => computerTarget(computer) === target)
    if (!selected) throw "This computer is not on this device."
    const point = fixtureCheckpoint(selected, label, "manual")
    if (selected.pendingCheckpointRestore) checkpointMembers.current.set(point.id, selected.pendingCheckpointRestore.checkpointId)
    setComputers((current) => current.map((computer) => computerTarget(computer) === target
      ? { ...computer, checkpoints: [point, ...(computer.checkpoints ?? [])], checkpointOperation: null }
      : computer))
  }, [computers])
  const forkCheckpoint = useCallback(async (target: string, checkpointId: string | null, newName: string) => {
    const selected = computers.find((computer) => computerTarget(computer) === target)
    if (!selected) throw "The source computer is not on this device."
    if (computers.some((computer) => computer.device?.id === selected.device?.id && computer.configuration.name === newName)) throw "The fork name is already in use or the computer limit was reached."
    const pending = checkpointId === null ? selected.pendingCheckpointRestore : undefined
    const captured = checkpointId === null && !pending ? fixtureCheckpoint(selected, "Fork point", "manual") : undefined
    const point = captured ?? selected.checkpoints?.find((checkpoint) => checkpoint.id === checkpointId)
    if (!pending && !point) throw "The selected checkpoint no longer exists."
    const id = crypto.randomUUID()
    const child: ApplicationComputer = {
      ...pendingComputer(selected),
      configuration: { ...selected.configuration, id: selected.device ? remoteComputerTarget(selected.device.id, id) : id, name: newName },
      device: selected.device ? { ...selected.device, computerId: id } : undefined,
      pendingCheckpointRestore: pending ?? { checkpointId: checkpointMembers.current.get(point!.id) ?? point!.id, sourceComputer: selected.pendingCheckpointRestore?.sourceComputer ?? selected.configuration.name, state: point!.scope },
    }
    setComputers((current) => [...current.map((computer) => computerTarget(computer) === target && captured
      ? { ...computer, checkpoints: [captured, ...(computer.checkpoints ?? [])] }
      : computer), child])
  }, [computers])
  const restoreCheckpoint = useCallback(async (target: string, checkpointId: string) => {
    const selected = computers.find((computer) => computerTarget(computer) === target)
    if (!selected) throw "This computer is not on this device."
    const point = selected.checkpoints?.find((checkpoint) => checkpoint.id === checkpointId)
    if (!point) throw "The selected checkpoint no longer exists."
    const recovery = fixtureCheckpoint(selected, "Before restore", "before-restore")
    if (selected.pendingCheckpointRestore) checkpointMembers.current.set(recovery.id, selected.pendingCheckpointRestore.checkpointId)
    setComputers((current) => current.map((computer) => computerTarget(computer) === target
      ? { ...pendingComputer(computer), checkpoints: [recovery, ...(computer.checkpoints ?? [])], pendingCheckpointRestore: { checkpointId: checkpointMembers.current.get(point.id) ?? point.id, sourceComputer: computer.pendingCheckpointRestore?.sourceComputer ?? computer.configuration.name, state: point.scope } }
      : computer))
  }, [computers])

  const deleteCheckpoint = useCallback(async (target: string, checkpointId: string) => {
    setComputers((current) => current.map((computer) => computer.configuration.name === target
      ? { ...computer, checkpoints: (computer.checkpoints ?? []).filter((point) => point.id !== checkpointId) }
      : computer))
  }, [])

  const saveSecret = useCallback((request: SecretConfigurationRequest) => {
    // The preview retains metadata only; entered values are deliberately discarded.
    const secret: ApplicationSecret = {
      id: request.operation === "edit" ? request.id : crypto.randomUUID(),
      name: request.name,
      ...(request.kind === "github-token"
        ? { kind: "github-token" as const, account: "preview-user", connection: { state: "connected" as const }, state: "active" as const }
        : { state: "restart-required" as const }),
      computers: request.computers,
      allowedDomains: request.allowedDomains,
    }
    setSecrets((current) => request.operation === "edit"
      ? current.map((existing) => existing.id === request.id ? secret : existing)
      : [...current, secret])
  }, [])

  return { source: { ...source, computers, secrets }, saveSecret, removeSecret, onRestoreComplete, createCheckpoint, forkCheckpoint, restoreCheckpoint, deleteCheckpoint }
}
