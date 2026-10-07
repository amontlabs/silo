import { type ReactNode, useEffect, useMemo, useRef, useState } from "react"

import { TabsContent } from "@/components/ui/tabs"
import { PageContainer } from "@/components/page"
import { DeletionConfirmation } from "@/features/onboarding/components/deletion-confirmation"
import { SetupComplete } from "@/features/onboarding/components/setup-complete"
import type { ApplicationGitHubComputerPolicy } from "@/features/application/model/application-source"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import { OnboardingShell } from "@/features/onboarding/components/onboarding-shell"
import type {
  GitHubConnectionState,
  OnboardingActions,
  OnboardingCompletionRequest,
  OnboardingSource,
  OnboardingSubmissionOptions,
  ComputerGitIdentity,
  ComputerRepositorySelection,
} from "@/features/onboarding/model/onboarding-source"
import { onboardingSteps, projectOnboarding, type OnboardingStep, type ComputerView } from "@/features/onboarding/model/onboarding-state"
import { configurationRequest } from "@/features/onboarding/model/computer-configuration"
import { DependenciesStep } from "@/features/onboarding/steps/dependencies-step"
import { GitHubStep } from "@/features/onboarding/steps/github-step"
import { ReviewStep } from "@/features/onboarding/steps/review-step"
import { ComputersStep } from "@/features/onboarding/steps/computers-step"
import type { OnboardingDraft } from "@/features/onboarding/model/onboarding-draft"
import { useSettings } from "@/features/preferences/settings-store"
import { SettingsSaveNotice } from "@/features/preferences/components/settings-save-notice"
import { applicationPreferenceChanges } from "@/features/preferences/model/application-preferences"

export interface OnboardingAppProps {
  source: OnboardingSource
  actions: OnboardingActions
  githubConnectionState: GitHubConnectionState
  operationError?: string | null
  completed: boolean
  presentationOnlyCompleted?: boolean
  repositoryOptions?: readonly string[]
  repositoryPolicies?: readonly ApplicationGitHubComputerPolicy[]
  tokenConnected?: boolean
  onOpenApp?: () => void
  onRetryDependencies?: () => void
  onConnectDevice?: () => void
}

function OnboardingPanel({ step, activeStep, notice, children }: { step: OnboardingStep; activeStep: OnboardingStep; notice?: ReactNode; children: ReactNode }) {
  const active = step === activeStep
  // Retain layout as well as state: display:none restarts disclosure animations
  // and can clamp the panel's scroll offset when the step becomes visible again.
  return <TabsContent
    forceMount
    value={step}
    aria-hidden={!active}
    inert={!active}
    style={{ visibility: active ? "visible" : "hidden" }}
    className="absolute inset-0 mt-0 flex h-full min-h-0 flex-col overflow-y-auto outline-none"
  >
    <PageContainer className="flex-1">{active && <SettingsSaveNotice />}{active && notice}{children}</PageContainer>
  </TabsContent>
}

function repositoryKey(repository: string): string {
  return repository.toLowerCase()
}

function computerValue<T>(values: Record<string, T> | undefined, name: string): T | undefined {
  return values && Object.hasOwn(values, name) ? values[name] : undefined
}

function uniqueRepositoryOptions(repositories: readonly string[]): string[] {
  const seen = new Set<string>()
  return repositories.filter((repository) => {
    const key = repositoryKey(repository)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

function uniqueComputerSelections(selections: readonly ComputerRepositorySelection[]): ComputerRepositorySelection[] {
  const seen = new Set<string>()
  return selections.filter(({ repository }) => {
    const key = repositoryKey(repository)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

function initialComputerSelections(source: OnboardingSource): Record<string, ComputerRepositorySelection[]> {
  return Object.fromEntries(source.computerConfigurations.map(({ name }) => {
    const repositories = source.githubPolicies
      .filter(({ computer }) => computer === name)
      .flatMap(({ repositories: policyRepositories }) => policyRepositories)
    return [name, uniqueComputerSelections(repositories.map((repository) => ({
      repository: repository.fullName,
      allowPushes: repository.mode === "read-write",
    })))]
  }))
}

function defaultComputerIdentity(source: OnboardingSource): ComputerGitIdentity {
  const { name = "", email = "" } = source.currentDeviceGitIdentity ?? {}
  return { name, email, apply: Boolean(name.trim() && email.trim()) }
}

function initialComputerIdentities(source: OnboardingSource): Record<string, ComputerGitIdentity> {
  return Object.fromEntries(source.computerConfigurations.map((computer) => [
    computer.name,
    defaultComputerIdentity(source),
  ]))
}

function gitIdentityLabel(identity: ComputerGitIdentity): string {
  return `${identity.name || "No name"} <${identity.email || "No email"}>`
}

function computerIdentitySummary(
  identities: Record<string, ComputerGitIdentity>,
  computerNames: readonly string[],
): string {
  const appliedGroups = new Map<string, { identity: ComputerGitIdentity; computers: string[] }>()
  const notApplied: string[] = []

  for (const computer of computerNames) {
    const identity = computerValue(identities, computer)
    if (!identity?.apply) {
      notApplied.push(computer)
      continue
    }
    const key = JSON.stringify([identity.name, identity.email])
    const group = appliedGroups.get(key)
    if (group) group.computers.push(computer)
    else appliedGroups.set(key, { identity, computers: [computer] })
  }

  const summaries = [...appliedGroups.values()].map(({ identity, computers }) => {
    const target = computers.length === computerNames.length
      ? `all ${computerNames.length} ${computerNames.length === 1 ? "computer" : "computers"}`
      : computers.join(", ")
    return `${gitIdentityLabel(identity)} → ${target}`
  })
  if (notApplied.length > 0) {
    summaries.push(`not applied → ${notApplied.join(", ")}`)
  }
  return summaries.join("; ")
}

function defaultRepositoryOptions(source: OnboardingSource): string[] {
  return source.githubPolicies.flatMap(({ repositories }) => repositories.map(({ fullName }) => fullName))
}

export function OnboardingApp({
  source,
  actions,
  githubConnectionState,
  operationError,
  completed,
  presentationOnlyCompleted = false,
  repositoryOptions,
  repositoryPolicies,
  tokenConnected = false,
  onOpenApp,
  onRetryDependencies,
  onConnectDevice,
}: OnboardingAppProps) {
  const { settings, onboardingDraft, updateSettings, updateOnboardingDraft } = useSettings(source.applicationPreferences)
  const [draft, setDraft] = useState<OnboardingDraft>(() => {
    const restored = onboardingDraft ?? {
      currentStep: "dependencies" as const,
      computers: source.computerConfigurations.map((configuration) => ({ ...configuration })),
      unfinishedComputerEditor: null,
      computerRepositoryAccess: Object.fromEntries((repositoryPolicies ?? []).map((policy) => [policy.computer, { repositoryMode: policy.repositoryMode ?? "selected", allRepositoriesAllowChanges: policy.allRepositoriesAllowChanges ?? false, ...(policy.authenticationMethod ? { authenticationMethod: policy.authenticationMethod } : {}) }])),
      computerSelections: repositoryPolicies ? Object.fromEntries(repositoryPolicies.map((policy) => [policy.computer, [...policy.repositories]])) : initialComputerSelections(source),
      computerIdentities: repositoryPolicies ? { ...initialComputerIdentities(source), ...Object.fromEntries(repositoryPolicies.map((policy) => [policy.computer, { ...policy.identity }])) } : initialComputerIdentities(source),
    }
    return completed ? { ...restored, currentStep: "review" } : restored
  })
  // A restored draft is the user's; otherwise the draft is seeded again from this
  // device's computers once they load (a fallback seed is only a placeholder).
  const configurationsInitialized = useRef(onboardingDraft !== null || (draft.computers.length > 0 && source.configurationsAuthoritative !== false))
  const currentDraft = useRef(draft)
  const editedIdentities = useRef(new Set(Object.keys(onboardingDraft?.computerIdentities ?? {})))
  const editedSelections = useRef(new Set(Object.keys(onboardingDraft?.computerSelections ?? {})))
  const editedRepositoryAccess = useRef(new Set(Object.keys(onboardingDraft?.computerRepositoryAccess ?? {})))
  const editedAuthenticationMethods = useRef(new Set(Object.entries(onboardingDraft?.computerRepositoryAccess ?? {}).filter(([, access]) => access.authenticationMethod !== undefined).map(([name]) => name)))
  const policiesInitialized = useRef(new Set(onboardingDraft ? [] : (repositoryPolicies ?? []).map(({ computer }) => computer)))
  const recoveryCleared = useRef(false)
  // Existing computers the user deleted with the list's own Delete confirmation.
  const confirmedRemovals = useRef(new Set<string>())
  const [pendingDeletion, setPendingDeletion] = useState<{ configurations: SetupComputerConfiguration[]; run: () => void } | null>(null)
  const { currentStep: activeStep, computers, computerSelections, computerIdentities } = draft
  const viewModel = useMemo(() => {
    const projectedSource = source.setupQueue ? {
      ...source,
      computerConfigurations: computers,
      bootstrapConfiguration: { ...source.bootstrapConfiguration, computers: computers.map((configuration) => ({ name: configuration.name, cpu: configuration.cpus, cpuCeiling: configuration.maxCPUs, memoryGiB: configuration.memoryGiB, memoryCeilingGiB: configuration.maxMemoryGiB, workspaceStorageGiB: configuration.workspaceStorageGiB, runtimeStorageGiB: configuration.runtimeStorageGiB })) },
    } : source
    return projectOnboarding(projectedSource, githubConnectionState)
  }, [githubConnectionState, source, computers])
  const applicationPreferences = useMemo(() => ({
    terminal: settings.terminal, editor: settings.editor, browser: settings.browser,
    terminalUseSystemDefault: settings.terminalUseSystemDefault,
    editorUseSystemDefault: settings.editorUseSystemDefault,
    browserUseSystemDefault: settings.browserUseSystemDefault,
    ...(settings.terminalPath && { terminalPath: settings.terminalPath }),
    ...(settings.editorPath && { editorPath: settings.editorPath }),
    ...(settings.browserPath && { browserPath: settings.browserPath }),
  }), [settings])
  const completionInputs = useRef({ applications: applicationPreferences, githubConnectionState })
  useEffect(() => {
    completionInputs.current = { applications: applicationPreferences, githubConnectionState }
  }, [applicationPreferences, githubConnectionState])
  const availableRepositories = useMemo(
    () => uniqueRepositoryOptions(repositoryOptions ?? defaultRepositoryOptions(source)),
    [repositoryOptions, source],
  )

  useEffect(() => {
    const current = currentDraft.current
    if (configurationsInitialized.current || current.unfinishedComputerEditor || source.computerConfigurations.length === 0) return
    const authoritative = source.configurationsAuthoritative !== false
    if (authoritative) configurationsInitialized.current = true
    const configurations = source.computerConfigurations.map((configuration) => ({ ...configuration }))
    if (JSON.stringify(configurations) === JSON.stringify(current.computers)) return
    // Keep choices already made for computers that remain in the list.
    const names = new Set(configurations.map(({ name }) => name))
    const kept = <T,>(values: Record<string, T>) => Object.fromEntries(Object.entries(values).filter(([name]) => names.has(name)))
    const next = { ...current, computers: configurations,
      computerSelections: { ...initialComputerSelections(source), ...kept(current.computerSelections) },
      computerIdentities: { ...initialComputerIdentities(source), ...kept(current.computerIdentities) },
    }
    currentDraft.current = next
    setDraft(next)
    // A placeholder seed is not saved as the user's draft.
    if (authoritative) void updateOnboardingDraft(next)
  }, [source, draft.unfinishedComputerEditor, updateOnboardingDraft])

  useEffect(() => {
    if (completed || !repositoryPolicies) return
    const current = currentDraft.current
    const names = new Set(current.computers.map(({ name }) => name))
    const next = { ...current,
      computerRepositoryAccess: { ...current.computerRepositoryAccess },
      computerSelections: { ...current.computerSelections },
      computerIdentities: { ...current.computerIdentities },
    }
    let changed = false
    for (const policy of repositoryPolicies) {
      const name = policy.computer
      if (!names.has(name) || policiesInitialized.current.has(name)) continue
      policiesInitialized.current.add(name)
      if (!editedRepositoryAccess.current.has(name)) {
        next.computerRepositoryAccess[name] = { repositoryMode: policy.repositoryMode ?? "selected", allRepositoriesAllowChanges: policy.allRepositoriesAllowChanges ?? false }
        changed = true
      }
      if (!editedAuthenticationMethods.current.has(name) && policy.authenticationMethod) {
        next.computerRepositoryAccess[name] = {
          ...next.computerRepositoryAccess[name], authenticationMethod: policy.authenticationMethod,
        }
        changed = true
      }
      if (!editedSelections.current.has(name)) {
        next.computerSelections[name] = policy.repositories.map((repository) => ({ ...repository }))
        changed = true
      }
      if (!editedIdentities.current.has(name)) {
        next.computerIdentities[name] = { ...policy.identity }
        changed = true
      }
    }
    if (!changed) return
    currentDraft.current = next
    setDraft(next)
    void updateOnboardingDraft(next)
  }, [completed, repositoryPolicies, computers, updateOnboardingDraft])

  useEffect(() => {
    const host = source.currentDeviceGitIdentity
    if (completed || !host) return
    const current = currentDraft.current
    const identities = { ...current.computerIdentities }
    let changed = false
    for (const { name } of current.computers) {
      const identity = computerValue(identities, name)
      if (editedIdentities.current.has(name) || (identity?.apply === false && policiesInitialized.current.has(name)) || identity?.name.trim() || identity?.email.trim()) continue
      identities[name] = { ...host, apply: Boolean(host.name.trim() && host.email.trim()) }
      changed = true
    }
    if (!changed) return
    const next = { ...current, computerIdentities: identities }
    currentDraft.current = next
    setDraft(next)
    void updateOnboardingDraft(next)
  }, [completed, source.currentDeviceGitIdentity, computers, updateOnboardingDraft])

  // Completion comes from the existing action's result, never from a recovered
  // draft. A failed or unfinished completion leaves recovery data intact.
  useEffect(() => {
    if (completed && !presentationOnlyCompleted && !recoveryCleared.current) {
      recoveryCleared.current = true
      void updateOnboardingDraft(null)
    }
  }, [completed, presentationOnlyCompleted, updateOnboardingDraft])

  function updateDraft(changes: Partial<OnboardingDraft>) {
    if (completed || Object.entries(changes).every(([key, value]) => currentDraft.current[key as keyof OnboardingDraft] === value)) return
    const next = { ...currentDraft.current, ...changes }
    currentDraft.current = next
    setDraft(next)
    void updateOnboardingDraft(next)
  }

  function setActiveStep(currentStep: OnboardingStep) {
    updateDraft({ currentStep })
  }

  function move(offset: -1 | 1) {
    const current = onboardingSteps.indexOf(activeStep)
    const next = onboardingSteps[current + offset]
    if (next) setActiveStep(next)
  }

  // Existing computers a submission would delete without the user having deleted them.
  function unconfirmedDeletions(): SetupComputerConfiguration[] {
    const kept = new Set(currentDraft.current.computers.map(({ id }) => id))
    return (source.existingConfigurations ?? []).filter(({ id }) => !kept.has(id) && !confirmedRemovals.current.has(id))
  }

  // Trailing submission arguments: the confirmed deletions, when there are any.
  function submissionOptions(): [] | [OnboardingSubmissionOptions] {
    return confirmedRemovals.current.size ? [{ confirmedDeletions: [...confirmedRemovals.current] }] : []
  }

  // Every submission builds its request from the draft when it runs, after any
  // confirmation below.
  function submitChecked(run: () => void) {
    const missing = unconfirmedDeletions()
    if (missing.length === 0) { setPendingDeletion(null); run(); return }
    setPendingDeletion({ configurations: missing, run })
  }

  function keepExistingConfigurations() {
    const pending = pendingDeletion
    if (!pending) return
    setPendingDeletion(null)
    const existing = source.existingConfigurations ?? []
    const existingIds = new Set(existing.map(({ id }) => id))
    const restoredNames = new Set(pending.configurations.map(({ name }) => name.toLowerCase()))
    const current = currentDraft.current
    // A new draft computer reusing a restored name (typically the default seeded before
    // the real computers loaded) would collide with it, so it gives way.
    const configurations = current.computers.filter(({ id, name }) => existingIds.has(id) || !restoredNames.has(name.toLowerCase()))
    for (const configuration of pending.configurations) {
      configurations.splice(Math.min(existing.findIndex(({ id }) => id === configuration.id), configurations.length), 0, { ...configuration })
    }
    const host = defaultComputerIdentity(source)
    const savedPolicies = new Map((repositoryPolicies ?? []).map((policy) => [policy.computer, policy]))
    updateDraft({
      computers: configurations,
      computerSelections: Object.fromEntries(configurations.map(({ name }) => [name, computerValue(current.computerSelections, name) ?? savedPolicies.get(name)?.repositories.map((repository) => ({ ...repository })) ?? []])),
      computerIdentities: Object.fromEntries(configurations.map(({ name }) => [name, computerValue(current.computerIdentities, name) ?? { ...(savedPolicies.get(name)?.identity ?? host) }])),
      computerRepositoryAccess: Object.fromEntries(configurations.map(({ name }) => {
        const policy = savedPolicies.get(name)
        return [name, computerValue(current.computerRepositoryAccess, name) ?? {
          repositoryMode: policy?.repositoryMode ?? "selected" as const,
          allRepositoriesAllowChanges: policy?.allRepositoriesAllowChanges ?? false,
          ...(policy?.authenticationMethod ? { authenticationMethod: policy.authenticationMethod } : {}),
        }]
      })),
    })
    pending.run()
  }

  function deleteExistingConfigurations() {
    const pending = pendingDeletion
    if (!pending) return
    setPendingDeletion(null)
    for (const { id } of pending.configurations) confirmedRemovals.current.add(id)
    pending.run()
  }

  function saveConfigurations(updated: SetupComputerConfiguration[]) {
    const request = configurationRequest(updated)
    configurationsInitialized.current = true
    const current = currentDraft.current
    // The list asks before deleting a computer; that confirmation covers existing ones.
    const remaining = new Set(request.computers.map(({ id }) => id))
    for (const { id } of current.computers) if (!remaining.has(id)) confirmedRemovals.current.add(id)
    const previousNameByID = new Map(current.computers.map(({ id, name }) => [id, name]))
    const selections = Object.fromEntries(request.computers.map(({ id, name }) => {
      const previousName = previousNameByID.get(id)
      return [name, computerValue(current.computerSelections, name) ?? (previousName ? computerValue(current.computerSelections, previousName) : undefined) ?? []]
    }))
    const identities = Object.fromEntries(request.computers.map(({ id, name }) => {
      const previousName = previousNameByID.get(id)
      return [name, computerValue(current.computerIdentities, name) ?? (previousName ? computerValue(current.computerIdentities, previousName) : undefined)
        ?? defaultComputerIdentity(source)]
    }))
    const computerRepositoryAccess = Object.fromEntries(request.computers.map(({ id, name }) => [name, computerValue(current.computerRepositoryAccess, name) ?? computerValue(current.computerRepositoryAccess, previousNameByID.get(id) ?? "") ?? { repositoryMode: "selected" as const, allRepositoriesAllowChanges: false }]))
    updateDraft({ computers: request.computers, computerRepositoryAccess, computerSelections: selections, computerIdentities: identities, unfinishedComputerEditor: null })
    submitChecked(() => actions.saveComputerConfiguration(configurationRequest(currentDraft.current.computers), ...submissionOptions()))
  }

  function updateComputerSelections(computer: string, selections: ComputerRepositorySelection[]) {
    editedSelections.current.add(computer)
    updateDraft({ computerSelections: { ...currentDraft.current.computerSelections, [computer]: uniqueComputerSelections(selections) } })
  }

  function updateComputerIdentity(computer: string, identity: ComputerGitIdentity) {
    editedIdentities.current.add(computer)
    updateDraft({ computerIdentities: { ...currentDraft.current.computerIdentities, [computer]: identity } })
  }

  function resetComputerIdentity(computer: string) {
    if (!source.currentDeviceGitIdentity) return
    updateComputerIdentity(computer, { ...(computerValue(currentDraft.current.computerIdentities, computer) ?? { apply: false }), ...source.currentDeviceGitIdentity })
  }

  function completionRequest(): OnboardingCompletionRequest {
    // A deletion confirmation can retain this callback across settings and
    // connection changes. Read the latest inputs when the user confirms.
    const { applications, githubConnectionState } = completionInputs.current
    return {
      computerConfiguration: { schemaVersion: 1, computers: [...currentDraft.current.computers] },
      applications,
      github: {
        connectionState: githubConnectionState,
        computers: currentDraft.current.computers.map(({ name }) => {
          const access = computerValue(currentDraft.current.computerRepositoryAccess, name)
          const useGitHub = githubConnectionState === "connected" || access?.authenticationMethod === "token"
          return {
            computer: name,
            ...(useGitHub ? access : undefined),
            repositories: useGitHub ? [...(computerValue(currentDraft.current.computerSelections, name) ?? [])] : [],
            identity: { ...(computerValue(currentDraft.current.computerIdentities, name) ?? { name: "", email: "", apply: false }) },
          }
        }),
      },
    }
  }

  function continueSetup() {
    if (completed) return
    if (activeStep === "review") {
      if (viewModel.finishEnabled) submitChecked(() => actions.finishSetup(completionRequest(), ...submissionOptions()))
      return
    }
    if (activeStep === "computers" || activeStep === "github") {
      const step = activeStep
      const next = onboardingSteps[onboardingSteps.indexOf(step) + 1]
      submitChecked(() => { actions.submitStep?.(step, completionRequest(), ...submissionOptions()); setActiveStep(next) })
      return
    }
    move(1)
  }

  // Retry rebuilds the request from the current draft, so edits since the failed
  // attempt (identities, repository choices, computers) apply.
  function retrySetup() {
    if (completed) return
    submitChecked(() => actions.retryComputerSetup(completionRequest(), ...submissionOptions()))
  }

  const computerNames = computers.map(({ name }) => name)
  const oauthComputerNames = computerNames.filter((name) => computerValue(draft.computerRepositoryAccess, name)?.authenticationMethod !== "token")
  const tokenComputerCount = computerNames.length - oauthComputerNames.length
  const allComputerCount = oauthComputerNames.filter((name) => computerValue(draft.computerRepositoryAccess, name)?.repositoryMode === "all").length
  const allWriteComputerCount = oauthComputerNames.filter((name) => {
    const access = computerValue(draft.computerRepositoryAccess, name)
    return access?.repositoryMode === "all" && access.allRepositoriesAllowChanges
  }).length
  const configuredComputerCount = oauthComputerNames.filter((name) => (computerValue(computerSelections, name) ?? []).length > 0).length
  const repositoryCount = oauthComputerNames.reduce((total, name) => total + (computerValue(computerSelections, name) ?? []).length, 0)
  const pushEnabledRepositoryCount = oauthComputerNames.reduce(
    (total, name) => total + (computerValue(computerSelections, name) ?? []).filter(({ allowPushes }) => allowPushes).length,
    0,
  )
  const repositoryLabel = repositoryCount === 1 ? "repository" : "repositories"
  const pushRepositoryLabel = pushEnabledRepositoryCount === 1 ? "repository" : "repositories"
  const oauthSummary = githubConnectionState === "connected" && (oauthComputerNames.length > 0 || tokenComputerCount === 0)
    ? allComputerCount > 0 ? `All authorized repositories in ${allComputerCount} ${allComputerCount === 1 ? "computer" : "computers"} · ${allWriteComputerCount} allowing GitHub changes` : `${repositoryCount} ${repositoryLabel} across ${configuredComputerCount} of ${oauthComputerNames.length} ${oauthComputerNames.length === 1 ? "computer" : "computers"} · ${pushEnabledRepositoryCount} ${pushRepositoryLabel} allowing GitHub changes`
    : null
  const tokenSummary = tokenComputerCount > 0 ? `Personal token in ${tokenComputerCount} ${tokenComputerCount === 1 ? "computer" : "computers"}` : null
  const githubSummary = [oauthSummary, tokenSummary].filter(Boolean).join(" · ") || "GitHub not connected"
  const identitySummary = computerIdentitySummary(
    computerIdentities,
    computerNames,
  )
  const deletionNotice = pendingDeletion && !completed
    ? <DeletionConfirmation configurations={pendingDeletion.configurations} onKeep={keepExistingConfigurations} onDelete={deleteExistingConfigurations} />
    : null
  const computerComputerViews = computers.map((configuration): ComputerView => (
    viewModel.computerProgress.computers.find(({ name }) => name === configuration.name)
      ?? { name: configuration.name, status: "waiting", detail: "Waiting" }
  ))
  return (
    <OnboardingShell
      activeStep={activeStep}
      viewModel={viewModel}
      onStepChange={setActiveStep}
      onBack={() => move(-1)}
      onContinue={continueSetup}
      completed={completed}
      onOpenApp={onOpenApp}
      reduceMotion={settings.reduceMotion}
    >
      <OnboardingPanel step="dependencies" activeStep={activeStep} notice={deletionNotice}>
        <DependenciesStep
          groups={viewModel.dependencies}
          applicationPreferences={applicationPreferences}
          onApplicationPreferencesChange={(preferences) => {
            void updateSettings(applicationPreferenceChanges(applicationPreferences, preferences))
          }}
          onRetry={onRetryDependencies}
          onConnectDevice={onConnectDevice}
        />
      </OnboardingPanel>
      <OnboardingPanel step="computers" activeStep={activeStep} notice={deletionNotice}>
        <ComputersStep onConnectDevice={onConnectDevice} configurations={computers} progress={viewModel.computerProgress} onConfigurationsChange={saveConfigurations} onRetry={retrySetup} initialEditorDraft={draft.unfinishedComputerEditor} onEditorDraftChange={(unfinishedComputerEditor) => updateDraft({ unfinishedComputerEditor })} />
      </OnboardingPanel>
      <OnboardingPanel step="github" activeStep={activeStep} notice={deletionNotice}>
        <GitHubStep
          queueItems={viewModel.queueItems}
          activityEvents={source.activityEvents ?? source.progressEvents}
          computers={computerComputerViews}
          connectionState={githubConnectionState}
          tokenConnected={tokenConnected}
          notice={operationError ? <p role="alert" className="text-xs text-destructive">{operationError}</p> : undefined}
          repositoryOptions={availableRepositories}
          computerSelections={Object.fromEntries(computerNames.map((name) => [name, computerValue(computerSelections, name) ?? []]))}
          computerRepositoryAccess={Object.fromEntries(computerNames.map((name) => [name, computerValue(draft.computerRepositoryAccess, name) ?? { repositoryMode: "selected", allRepositoriesAllowChanges: false }]))}
          onComputerRepositoryAccessChange={(computer, access) => {
            if (access.authenticationMethod !== currentDraft.current.computerRepositoryAccess?.[computer]?.authenticationMethod) editedAuthenticationMethods.current.add(computer)
            editedRepositoryAccess.current.add(computer)
            updateDraft({ computerRepositoryAccess: { ...currentDraft.current.computerRepositoryAccess, [computer]: access } })
          }}
          computerIdentities={Object.fromEntries(computerNames.map((name) => [name, computerValue(computerIdentities, name) ?? { name: "", email: "", apply: false }]))}
          currentDeviceGitIdentity={source.currentDeviceGitIdentity}
          onConnect={actions.connectGitHub}
          onCancelConnection={actions.cancelGitHubConnection}
          onReopenAuthorization={actions.reopenGitHubAuthorization}
          onComputerSelectionsChange={updateComputerSelections}
          onComputerIdentityChange={updateComputerIdentity}
          onResetComputerIdentity={resetComputerIdentity}
        />
      </OnboardingPanel>
      <OnboardingPanel step="review" activeStep={activeStep} notice={deletionNotice}>
        {completed ? <SetupComplete configurations={computers} githubSummary={githubSummary} /> : <ReviewStep
          onEditStep={setActiveStep}
          computerRetryable={viewModel.computerProgress.retryable}
          queueItems={viewModel.queueItems}
          computers={viewModel.computerProgress.computers}
          configurations={computers}
          githubConnected={githubConnectionState === "connected" || tokenComputerCount > 0}
          githubSummary={githubSummary}
          identitySummary={identitySummary}
          errorMessage={viewModel.error?.message}
          errorRecovery={viewModel.error?.recovery ?? undefined}
          onRetryComputerSetup={retrySetup}
          finishBlocker={viewModel.finishBlocker}
          onStartComputer={actions.startComputer}
          onRefresh={actions.refreshSetupState}
        />}
      </OnboardingPanel>
    </OnboardingShell>
  )
}
