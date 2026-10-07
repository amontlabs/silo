import { useEffect, useMemo, useRef, useState } from "react"
import { TriangleAlert } from "lucide-react"

import { PersonalTokenConnection } from "@/features/github/components/personal-token-connection"
import { CopyButton } from "@/components/copy-button"
import { githubFailure } from "./github-failure"

import { InlineConfirmation } from "@/components/inline-confirmation"
import { Button } from "@/components/ui/button"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { dismissOperationToast, showOperationFailure, showOperationProgress, showOperationSuccess } from "@/lib/operation-toast"
import type {
  ApplicationActions,
  ApplicationGitHubConfiguration,
  ApplicationSource,
  GitHubComputerOperation,
} from "@/features/application/model/application-source"
import {
  GitHubAccessEditor,
  type GitHubIdentity,
  type GitHubRepositoryAccess,
  type GitHubRepositorySelection,
} from "@/features/github/components/github-access-editor"

type ComputerSelections = Record<string, GitHubRepositorySelection[]>
type ComputerIdentities = Record<string, GitHubIdentity>
type ComputerOperations = Record<string, GitHubComputerOperation>

interface GitHubDraft {
  access: Record<string, GitHubRepositoryAccess>
  selections: ComputerSelections
  identities: ComputerIdentities
}

function draftFromSource(
  policiesSnapshot: ApplicationSource["github"]["computers"],
  deviceIdentity: ApplicationSource["github"]["deviceIdentity"],
  computers: ApplicationSource["computers"],
): GitHubDraft {
  const policies = computers.map((computer) => policiesSnapshot?.find((policy) => policy.computer === computer.configuration.name) ?? ({
    computer: computer.configuration.name,
    repositoryMode: "selected" as const,
    allRepositoriesAllowChanges: false,
    identity: {
      name: deviceIdentity?.name ?? "",
      email: deviceIdentity?.email ?? "",
      apply: Boolean(deviceIdentity?.name.trim() && deviceIdentity.email.trim()),
    },
    repositories: computer.githubRepositories.map((repository) => ({ repository, allowPushes: false })),
  }))

  return {
    access: Object.fromEntries(policies.map((policy) => [policy.computer, { repositoryMode: policy.repositoryMode ?? "selected", allRepositoriesAllowChanges: policy.allRepositoriesAllowChanges ?? false, ...(policy.authenticationMethod ? { authenticationMethod: policy.authenticationMethod } : {}) }])),
    selections: Object.fromEntries(policies.map((policy) => [
      policy.computer,
      policy.repositories.map((repository) => ({ ...repository })),
    ])),
    identities: Object.fromEntries(policies.map((policy) => [policy.computer, { ...policy.identity }])),
  }
}

function copyDraft(draft: GitHubDraft): GitHubDraft {
  return {
    access: Object.fromEntries(Object.entries(draft.access).map(([name, access]) => [name, { ...access }])),
    selections: Object.fromEntries(Object.entries(draft.selections).map(([computer, selections]) => [
      computer,
      selections.map((selection) => ({ ...selection })),
    ])),
    identities: Object.fromEntries(Object.entries(draft.identities).map(([computer, identity]) => [computer, { ...identity }])),
  }
}

/**
 * A save of the computers in `changed` only, based on the settings revision this page
 * shows. Other computers keep their saved choices (for example an assignment a fork just
 * copied), and access on/off is never part of a save: a save sent right after Disable
 * access must not turn access back on.
 */
function configurationFromDraft(source: ApplicationSource, draft: GitHubDraft, changed: ReadonlySet<string>): ApplicationGitHubConfiguration {
  return {
    baseRevision: source.github.policyRevision,
    deviceIdentity: source.github.deviceIdentity ?? null,
    computers: source.computers.filter((w) => !w.device && changed.has(w.configuration.name)).map(({ configuration }) => ({
      computer: configuration.name,
      ...(draft.access[configuration.name] ?? { repositoryMode: "selected", allRepositoriesAllowChanges: false }),
      identity: draft.identities[configuration.name] ?? { name: "", email: "", apply: false },
      repositories: draft.selections[configuration.name] ?? [],
    })),
  }
}

function operationsFromSource(operations: readonly GitHubComputerOperation[] | undefined): ComputerOperations {
  return Object.fromEntries((operations ?? []).map((operation) => [operation.computer, operation]))
}

function sameIdentity(left: GitHubIdentity | undefined, right: GitHubIdentity) {
  return left?.name === right.name && left.email === right.email && left.apply === right.apply
}

/** Small persistent label in the computer header; the transient progress and results live in toasts. */
function ComputerSyncStatus({ operation }: { operation: GitHubComputerOperation }) {
  if (operation.status !== "failed") return null
  const failure = githubFailure(operation.message)
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button type="button" variant="ghost" size="xs" className="h-6 gap-1 px-1.5 text-caption text-destructive hover:text-destructive" aria-label={`GitHub settings not applied for ${operation.computer}. View details`}>
          <TriangleAlert className="size-3" aria-hidden="true" />Not applied
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-80 space-y-2 p-3 text-caption">
        <p className="font-medium">{failure.message}</p>
        <p className="whitespace-pre-wrap leading-5">{failure.details}</p>
        <CopyButton variant="ghost" size="xs" value={failure.details} labels={{ idle: "Copy details", copied: "Details copied", failed: "Copy failed" }} text={{ idle: "Copy details", copied: "Copied", failed: "Copy failed" }} />
      </PopoverContent>
    </Popover>
  )
}

function firstLine(text: string) {
  return text.split("\n")[0]?.trim() ?? ""
}

export function GitHubPage({
  source,
  actions,
  onBusyChange,
}: {
  source: ApplicationSource
  actions: ApplicationActions
  onBusyChange?: (busy: boolean) => void
}) {
  const sourceDraft = useMemo(
    () => draftFromSource(source.github.computers, source.github.deviceIdentity, source.computers.filter(w => !w.device)),
    [source.github.deviceIdentity, source.github.computers, source.computers],
  )
  const computerOwners = useMemo(
    () => new Map(source.computers.filter(computer => !computer.device).map(({ configuration }) => [configuration.name, configuration.id])),
    [source.computers],
  )
  const [draft, setDraft] = useState(() => copyDraft(sourceDraft))
  const [connectionState, setConnectionState] = useState(source.github.state)
  const [accessEnabled, setAccessEnabled] = useState(source.github.accessEnabled ?? true)
  const [computerOperations, setComputerOperations] = useState<ComputerOperations>(() => operationsFromSource(source.github.computerOperations))
  const [confirmingDisconnect, setConfirmingDisconnect] = useState(false)
  const identityIntent = useRef<ComputerIdentities>(copyDraft(sourceDraft).identities)
  const saveSequence = useRef(0)
  const rejectedSaves = useRef(new Set<string>())
  const pendingSaves = useRef(new Map<string, number>())
  const saveIntents = useRef(new Map<string, ApplicationGitHubConfiguration["computers"][number]>())
  const computerOwnersRef = useRef(computerOwners)
  const sourceDraftKey = useRef(JSON.stringify([sourceDraft, [...computerOwners]]))
  const sourceOperationsKey = useRef(JSON.stringify([source.github.policyRevision, source.github.computerOperations]))
  // Only operations the user started here notify; remember their toasts for owner changes.
  const userInitiated = useRef(new Set<string>())
  const toastComputers = useRef(new Set<string>())
  const catalogAvailable = source.github.repositoryCatalogStatus?.status !== "unavailable"
  const tokenConnected = source.github.personalToken?.state === "connected"
  const applying = Object.values(computerOperations).some((operation) => operation.status === "applying")
  const busy = connectionState === "connecting" || applying

  useEffect(() => {
    onBusyChange?.(busy)
  }, [busy, onBusyChange])

  useEffect(() => {
    const key = JSON.stringify([sourceDraft, [...computerOwners]])
    if (sourceDraftKey.current === key) return
    sourceDraftKey.current = key
    const changedOwners = new Set<string>()
    for (const [name, id] of computerOwnersRef.current) {
      if (computerOwners.get(name) === id) continue
      changedOwners.add(name)
      pendingSaves.current.delete(name)
      rejectedSaves.current.delete(name)
      saveIntents.current.delete(name)
      userInitiated.current.delete(name)
      if (toastComputers.current.delete(name)) dismissOperationToast(`github-apply:${name}`)
    }
    computerOwnersRef.current = computerOwners
    const submittedIdentities = identityIntent.current
    const next = copyDraft(sourceDraft)
    for (const [computer, policy] of saveIntents.current) {
      if (!next.access[computer]) continue
      next.access[computer] = { repositoryMode: policy.repositoryMode ?? "selected", allRepositoriesAllowChanges: policy.allRepositoriesAllowChanges ?? false, authenticationMethod: policy.authenticationMethod }
      next.selections[computer] = policy.repositories.map((repository) => ({ ...repository }))
      next.identities[computer] = { ...policy.identity }
    }
    identityIntent.current = copyDraft(next).identities
    // Preserve text still being edited; blur submits it separately.
    // oxlint-disable-next-line react/set-state-in-effect
    setDraft((current) => {
      for (const [computer, identity] of Object.entries(current.identities)) {
        if (!changedOwners.has(computer) && next.identities[computer] && !sameIdentity(submittedIdentities[computer], identity)) next.identities[computer] = identity
      }
      return next
    })
  }, [sourceDraft, computerOwners])

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    setConnectionState(source.github.state)
  }, [source.github])

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    setAccessEnabled(source.github.accessEnabled ?? true)
    // oxlint-disable-next-line react/set-state-in-effect
    setConfirmingDisconnect(false)
  }, [source.github.state, source.github.accessEnabled])

  useEffect(() => {
    const key = JSON.stringify([source.github.policyRevision, source.github.computerOperations])
    if (sourceOperationsKey.current === key) return
    sourceOperationsKey.current = key
    // Consecutive saves can finish with identical messages before an applying snapshot reaches the UI.
    // A new revision still settles the optimistic progress for that save.
    // oxlint-disable-next-line react/set-state-in-effect
    setComputerOperations(operationsFromSource(source.github.computerOperations))
  }, [source.github.policyRevision, source.github.computerOperations])

  const retryRef = useRef(retryComputer)
  retryRef.current = retryComputer
  const computersRef = useRef(source.computers)
  computersRef.current = source.computers
  const announced = useRef(new Map<string, string>(Object.entries(computerOperations).map(([name, operation]) => [name, `${operation.status}|${operation.message}`])))

  useEffect(() => {
    for (const [name, operation] of Object.entries(computerOperations)) {
      const key = `${operation.status}|${operation.message}`
      if (announced.current.get(name) === key) continue
      announced.current.set(name, key)
      if (!userInitiated.current.has(name)) continue
      toastComputers.current.add(name)
      if (operation.status !== "applying") userInitiated.current.delete(name)
      const id = `github-apply:${name}`
      const configuration = computersRef.current.find((computer) => !computer.device && computer.configuration.name === name)?.configuration
      const noticeComputer = configuration ? { id: configuration.id, name: configuration.name } : undefined
      if (operation.status === "applying") showOperationProgress(id, { title: operation.message, step: name, computer: name })
      else if (operation.status === "succeeded") showOperationSuccess(id, "GitHub settings applied", { description: name, computer: name, persist: true, noticeComputer })
      else {
        const failure = githubFailure(operation.message)
        showOperationFailure(id, failure.message, {
          description: `${name}: ${firstLine(failure.details)}`,
          computer: name,
          noticeComputer,
          retry: failure.canRetry && configuration ? () => {
            const current = computersRef.current.find(computer => !computer.device && computer.configuration.name === name)
            if (current?.configuration.id === configuration.id) retryRef.current(name)
          } : undefined,
        })
      }
    }
    for (const name of [...announced.current.keys()]) if (!computerOperations[name]) announced.current.delete(name)
  }, [computerOperations])

  function applyComputerDraft(computer: string, nextDraft: GitHubDraft, message: string) {
    setDraft(nextDraft)
    userInitiated.current.add(computer)
    setComputerOperations((current) => ({
      ...current,
      [computer]: { computer, status: "applying", message },
    }))
    const sequence = ++saveSequence.current
    // A save carries every edit not yet confirmed: this one, earlier pending ones and
    // rejected ones, so one failure settles them together and a retry resends them.
    for (const name of rejectedSaves.current) pendingSaves.current.set(name, sequence)
    rejectedSaves.current.clear()
    pendingSaves.current.set(computer, sequence)
    const submittedDraft = copyDraft({ ...nextDraft, identities: identityIntent.current })
    const intent = configurationFromDraft(source, submittedDraft, new Set([computer])).computers[0]
    if (intent) saveIntents.current.set(computer, intent)
    const configuration = configurationFromDraft(source, submittedDraft, new Set(pendingSaves.current.keys()))
    configuration.computers = configuration.computers.map((policy) => saveIntents.current.get(policy.computer) ?? policy)
    void Promise.resolve().then(() => actions.saveGitHubConfiguration?.(configuration)).then(() => {
      for (const [name, pendingSequence] of pendingSaves.current) {
        if (pendingSequence <= sequence) {
          pendingSaves.current.delete(name)
          saveIntents.current.delete(name)
        }
      }
    }).catch((cause: unknown) => {
      if (sequence !== saveSequence.current) return
      // Each save contains the complete configuration, including earlier pending edits.
      const failedComputers = [...pendingSaves.current.keys()]
      pendingSaves.current.clear()
      failedComputers.forEach((name) => rejectedSaves.current.add(name))
      setComputerOperations((current) => {
        const next = { ...current }
        for (const name of failedComputers) {
          next[name] = { computer: name, status: "failed", message: cause instanceof Error ? cause.message : "GitHub settings could not be saved.", canRetry: true }
        }
        return next
      })
    })
  }

  function updateSelections(computer: string, selections: GitHubRepositorySelection[]) {
    applyComputerDraft(computer, {
      ...draft,
      selections: { ...draft.selections, [computer]: selections },
    }, "Applying repository access…")
  }

  function updateIdentity(computer: string, identity: GitHubIdentity) {
    const previous = draft.identities[computer]
    const nextDraft = {
      ...draft,
      identities: { ...draft.identities, [computer]: identity },
    }
    setDraft(nextDraft)
    if (previous?.apply !== identity.apply) commitIdentity(computer, identity, nextDraft)
  }

  function commitIdentity(computer: string, identity: GitHubIdentity, currentDraft = draft) {
    if ((identity.apply && (!identity.name.trim() || !identity.email.trim())) || sameIdentity(identityIntent.current[computer], identity)) return
    identityIntent.current = { ...identityIntent.current, [computer]: { ...identity } }
    applyComputerDraft(computer, {
      ...currentDraft,
      identities: { ...currentDraft.identities, [computer]: identity },
    }, "Applying Git identity…")
  }

  function resetIdentity(computer: string) {
    if (!source.github.deviceIdentity) return
    const identity = { ...source.github.deviceIdentity, apply: true }
    const nextDraft = {
      ...draft,
      identities: { ...draft.identities, [computer]: identity },
    }
    identityIntent.current = { ...identityIntent.current, [computer]: identity }
    applyComputerDraft(computer, nextDraft, "Applying Git identity…")
  }

  function retryComputer(computer: string) {
    if (rejectedSaves.current.has(computer)) {
      applyComputerDraft(computer, draft, "Retrying GitHub access…")
      return
    }
    userInitiated.current.add(computer)
    setComputerOperations((current) => ({
      ...current,
      [computer]: { computer, status: "applying", message: "Retrying GitHub access…" },
    }))
    actions.retryGitHubConfiguration?.(computer)
  }

  function toggleAccess() {
    const nextEnabled = !accessEnabled
    source.computers.filter((w) => !w.device).forEach(({ configuration }) => userInitiated.current.add(configuration.name))
    actions.setGitHubAccessEnabled?.(nextEnabled)
  }

  function disconnect() {
    setConfirmingDisconnect(false)
    actions.disconnectGitHub?.()
  }

  const catalogNotice = source.github.repositoryCatalogStatus?.status === "unavailable" ? (
    <div className="flex items-center gap-3 rounded-md border border-destructive/25 bg-destructive/8 px-3 py-2 text-xs" role="alert">
      <TriangleAlert className="size-3.5 shrink-0 text-destructive" aria-hidden="true" />
      <span className="min-w-0 flex-1">{source.github.repositoryCatalogStatus.message}</span>
      {source.github.repositoryCatalogStatus.canRetry && <Button type="button" variant="outline" size="xs" onClick={() => actions.retryGitHubRepositoryCatalog?.()}>Retry repositories</Button>}
    </div>
  ) : undefined

  const accessToggle = <Button type="button" variant="outline" size="xs" disabled={applying} onClick={toggleAccess}>{accessEnabled ? "Disable for all computers" : "Enable for all computers"}</Button>
  const connectedActions = (
    <InlineConfirmation active={confirmingDisconnect} onDismiss={() => setConfirmingDisconnect(false)}>
      {confirmingDisconnect ? (
        <>
          <span className="max-w-xs text-caption text-muted-foreground">Revokes Silo's GitHub authorization and removes repository access from every computer on this device.</span>
          <Button type="button" variant="ghost" size="xs" onClick={() => setConfirmingDisconnect(false)}>Cancel</Button>
          <Button type="button" variant="destructive" size="xs" onClick={disconnect}>Disconnect</Button>
        </>
      ) : (
        <>
          {accessToggle}
          <Button type="button" variant="ghost" size="xs" disabled={applying} onClick={() => setConfirmingDisconnect(true)}>Disconnect</Button>
        </>
      )}
    </InlineConfirmation>
  )

  return (
    <div className="mx-auto flex h-full min-h-0 w-full max-w-4xl flex-col px-4 py-5 sm:px-6 sm:py-6">
      <div className="mb-2 flex shrink-0 items-center justify-between gap-3">
        <p className="text-caption text-muted-foreground">GitHub access for computers on this device.</p>
        {connectionState !== "connected" && tokenConnected && accessToggle}
      </div>
      <GitHubAccessEditor
        compactConnection
        computers={source.computers.filter(w => !w.device).map(({ configuration }) => ({ name: configuration.name }))}
        connectionState={connectionState}
        tokenConnected={tokenConnected}
        tokenConnection={<PersonalTokenConnection status={source.github.personalToken}
          onSave={actions.saveGitHubPersonalToken} onRemove={actions.removeGitHubPersonalToken} />}
        repositoryOptions={source.github.repositoryCatalog ?? []}
        computerSelections={draft.selections}
        computerRepositoryAccess={draft.access}
        onComputerRepositoryAccessChange={(computer, access) => applyComputerDraft(computer, { ...draft, access: { ...draft.access, [computer]: access } }, "Applying repository access…")}
        computerIdentities={draft.identities}
        currentDeviceGitIdentity={source.github.deviceIdentity ?? null}
        onConnect={() => {
          setConnectionState("connecting")
          actions.connectGitHub?.()
        }}
        onCancelConnection={actions.cancelGitHubConnection}
        onReopenAuthorization={actions.reopenGitHubAuthorization}
        onManageRepositories={actions.manageGitHubRepositories}
        onComputerSelectionsChange={updateSelections}
        onComputerIdentityChange={updateIdentity}
        onCommitComputerIdentity={commitIdentity}
        onResetComputerIdentity={resetIdentity}
        connectedTitle={`Connected as @${source.github.account ?? "unknown"}`}
        connectedDetail={accessEnabled
          ? "Repository credentials are scoped to each computer. Disabling access turns GitHub off for every computer on this device."
          : "GitHub access is off for every computer, including computers that use a personal token."}
        connectedActions={connectedActions}
        notice={catalogNotice}
        renderComputerActions={({ name }) => {
          const operation = computerOperations[name]
          return operation ? <ComputerSyncStatus operation={operation} /> : undefined
        }}
        repositoryControlsAvailable={catalogAvailable && accessEnabled}
        confirmRepositoryClear
        busy={applying}
      />
    </div>
  )
}
