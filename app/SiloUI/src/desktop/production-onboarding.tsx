import { SiloWindow } from "@/components/silo-window"
import { ConnectDeviceForm } from "@/features/application/components/connections-settings"
import { useEffect, useMemo, useRef, useState } from "react"

import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"

import type { DependencyRuntime } from "@/desktop/dependencies"
import { useProductionSource, type ProductionSnapshot, type ProductionSource } from "@/desktop/production-source"
import { setupComputerConfigurationSchema, type SetupComputerConfiguration, type SetupComputerConfigurationRequest, type SiloBootstrapConfiguration } from "@/contracts/silo"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { OnboardingApp } from "@/features/onboarding/onboarding-app"
import type { OnboardingCompletionRequest, OnboardingSource, OnboardingSubmissionOptions } from "@/features/onboarding/model/onboarding-source"
import { useSettings } from "@/features/preferences/settings-store"
import { errorMessage } from "@/lib/error-message"

function bootstrapConfiguration(configurations: readonly SetupComputerConfiguration[]): SiloBootstrapConfiguration {
  return {
    schemaVersion: 1,
    computers: configurations.map((configuration) => ({
      name: configuration.name,
      cpu: configuration.cpus,
      cpuCeiling: configuration.maxCPUs,
      memoryGiB: configuration.memoryGiB,
      memoryCeilingGiB: configuration.maxMemoryGiB,
      workspaceStorageGiB: configuration.workspaceStorageGiB,
      runtimeStorageGiB: configuration.runtimeStorageGiB,
    })),
  }
}

type Submission = {
  isFinishing: boolean
  run: (request?: OnboardingCompletionRequest, options?: OnboardingSubmissionOptions) => Promise<unknown>
}

/** Computers that exist on this device, as committed configuration. */
function existingLocalConfigurations(application: ApplicationSource | null): SetupComputerConfiguration[] {
  return (application?.computers ?? []).flatMap(({ device, configuration }) => {
    if (device) return []
    const parsed = setupComputerConfigurationSchema.safeParse(configuration)
    return parsed.success ? [parsed.data] : []
  })
}

/**
 * Onboarding never deletes an existing computer unless the user confirmed deleting it.
 * Checked against the source's committed list at submission time, which is what the
 * production source derives its changes from.
 */
function assertConfirmedDeletions(committed: ApplicationSource | null, configurations: readonly SetupComputerConfiguration[], options?: OnboardingSubmissionOptions) {
  const kept = new Set(configurations.map(({ id }) => id))
  const unconfirmed = existingLocalConfigurations(committed).filter(({ id }) => !kept.has(id) && !options?.confirmedDeletions.includes(id))
  if (unconfirmed.length === 0) return
  const names = unconfirmed.map(({ name }) => name).join(", ")
  throw new Error(`Setup did not delete ${names}. Confirm deleting ${unconfirmed.length === 1 ? "it" : "them"} first, or keep ${unconfirmed.length === 1 ? "it" : "them"} in setup. No computer changed.`)
}

// oxlint-disable-next-line react/only-export-components
export function productionOnboardingSource(application: ApplicationSource | null, dependencies: DependencyRuntime, applicationPreferences: OnboardingSource["applicationPreferences"], setup?: ProductionSnapshot): OnboardingSource {
  // Setup on this device must never adopt another device's computer identities.
  if (application) application = { ...application, computers: application.computers.filter(computer => !computer.device) }
  const operation = application?.computerConfigurationOperation
  const existingConfigurations = existingLocalConfigurations(application)
  // Only a real read of this device's state says which computers exist. Before it, or
  // while it is replaced by a shell (local state updating, or unreadable), the saved list
  // or defaults are a placeholder seed that is replaced once the real state loads.
  const configurationsAuthoritative = application !== null && !setup?.localUpdating && !(setup?.error && existingConfigurations.length === 0)
  const fallback = !configurationsAuthoritative && setup?.savedConfigurations?.length ? setup.savedConfigurations : productionComputerDefaults
  const configurations = setup?.setupCandidate?.computers ?? operation?.candidate.computers ?? (existingConfigurations.length ? existingConfigurations : fallback)
  const emptyConfigurationVerified = setup?.setupCandidate?.computers.length === 0
    && ["computerRun", "computerVerify"].every((id) => setup.setupQueue.some((item) => item.id === id && item.status === "succeeded"))
  const configured = !!application && (application.computers.length > 0 || emptyConfigurationVerified) && application.computers.every(({ freshness, state }) => freshness === "fresh" && state !== "failed" && state !== "starting") && operation?.status !== "applying" && operation?.status !== "failed"
  const completedPhases = configured ? ["preflight", "toolchain", "deviceIntegration", "computers"] as const : []
  const computerSetupPending = setup?.setupQueue.some(({ id, status }) => ["computerRun", "computerVerify"].includes(id) && (status === "running" || status === "queued" || status === "failed")) ?? false
  // Computer setup itself explains running, queued and failed work; otherwise say which
  // computer keeps Finish unavailable and how to resolve it.
  const settled = !!application && !configured && !computerSetupPending && operation?.status !== "applying" && operation?.status !== "failed"
  const failedComputer = settled ? application?.computers.find(({ state }) => state === "failed") : undefined
  const staleComputer = settled ? application?.computers.find(({ freshness }) => freshness === "stale") : undefined
  const startingComputer = settled ? application?.computers.find(({ state }) => state === "starting") : undefined
  const finishBlocker: OnboardingSource["finishBlocker"] = failedComputer
    ? { computer: failedComputer.configuration.name, action: "start", message: `${failedComputer.configuration.name} is not running: ${failedComputer.lifecycleFailure ?? failedComputer.stateDetail}. Start it to finish setup.` }
    : staleComputer
      ? { computer: staleComputer.configuration.name, action: "refresh", message: `${staleComputer.configuration.name}'s status could not be confirmed. Check again to finish setup.` }
      : startingComputer
        ? { computer: startingComputer.configuration.name, action: null, message: `Waiting for ${startingComputer.configuration.name} to start…` }
        : null
  return {
    ...(setup && { setupQueue: setup.setupQueue.map((item) => configured && item.status === "idle" && ["computerRun", "computerVerify"].includes(item.id) ? { ...item, status: "succeeded" as const } : item) }),
    readyToFinish: configured && !computerSetupPending,
    finishBlocker,
    configurationsAuthoritative: configurationsAuthoritative || Boolean(setup?.setupCandidate ?? operation),
    existingConfigurations,
    computerConfigurations: [...configurations],
    bootstrapConfiguration: bootstrapConfiguration(configurations),
    bootstrapState: {
      phase: "computers",
      updatedAt: setup?.setupFinishedAt ?? Math.floor(Date.now() / 1000),
      ...(setup?.setupStartedAt && { startedAt: setup.setupStartedAt }),
      completedPhases: [...completedPhases],
      phaseDurations: {},
      ...(operation?.status === "failed" && { lastError: operation.error.message }),
    },
    preflightChecks: dependencies.checks,
    progressEvents: setup ? setup.setupEvents : operation?.progressEvents ? [...operation.progressEvents] : [],
    activityEvents: setup?.setupActivity,
    activityError: setup?.setupActivityError,
    githubPolicies: [],
    currentDeviceGitIdentity: application?.github.deviceIdentity ?? null,
    applicationPreferences,
    bootstrapResult: configured ? { resumed: false, phase: "complete", requiresApproval: false, vmsStarted: false, message: "Computer configuration verified." } : operation?.status === "awaiting-approval" ? operation.result : null,
    error: operation?.status === "failed" ? operation.error : null,
  }
}

export function ProductionOnboarding({ application, dependencies, source, onOpenApp }: { application: ApplicationSource | null; dependencies: DependencyRuntime; source: ProductionSource; onOpenApp?: () => void }) {
  const setup = useProductionSource(source)
  const [, tick] = useState(0)
  const setupRunning = setup.setupQueue.some(({ status }) => status === "running")
  useEffect(() => {
    if (!setupRunning) return
    const timer = window.setInterval(() => tick((value) => value + 1), 1000)
    return () => window.clearInterval(timer)
  }, [setupRunning])
  const { settings, onboardingDraft, updateSettings, store } = useSettings()
  const lastSubmission = useRef<Submission | null>(null)
  const [completed, setCompleted] = useState(false)
  const [connectingDevice, setConnectingDevice] = useState(false)
  const submissionSequence = useRef(0)
  const [finishing, setFinishing] = useState(false)
  const [operationError, setOperationError] = useState<string | null>(null)
  const preferences = useMemo(() => ({
    terminal: settings.terminal,
    editor: settings.editor,
    browser: settings.browser,
    terminalPath: settings.terminalPath,
    editorPath: settings.editorPath,
    browserPath: settings.browserPath,
    terminalUseSystemDefault: settings.terminalUseSystemDefault,
    editorUseSystemDefault: settings.editorUseSystemDefault,
    browserUseSystemDefault: settings.browserUseSystemDefault,
  }), [settings])
  const onboarding = useMemo(
    () => {
      const current = { ...productionOnboardingSource(application, dependencies, preferences, { ...setup, backup: setup.backup.state }), ...(finishing && { readyToFinish: false }) }
      return operationError ? { ...current, error: { code: "native_operation_failed", message: operationError, recovery: "Review the configuration and retry.", computer: current.error?.computer ?? (application === null ? setup.setupEvents.at(-1)?.computer ?? null : null), retryable: true } } : current
    },
    [application, dependencies, preferences, operationError, finishing, setup],
  )
  useEffect(() => {
    if (!onboardingDraft || completed) return
    void source.verifySetupIdentities({
      computerConfiguration: { schemaVersion: 1, computers: onboardingDraft.computers },
      github: {
        connectionState: application?.github.state ?? "disconnected",
        computers: onboardingDraft.computers.map(({ name }) => ({
          computer: name,
          repositories: [],
          identity: Object.hasOwn(onboardingDraft.computerIdentities, name)
            ? onboardingDraft.computerIdentities[name] : { name: "", email: "", apply: false },
        })),
      },
    })
  }, [source, onboardingDraft, application?.github.state, completed])

  // The committed list the production source derives changes from, read when submitting.
  const committed = () => source.getSnapshot?.().source ?? application
  const configurationSubmission = (request: SetupComputerConfigurationRequest, options?: OnboardingSubmissionOptions): Submission => ({
    isFinishing: false,
    run: (current, confirmed = options) => {
      const configuration = current?.computerConfiguration ?? request
      assertConfirmedDeletions(committed(), configuration.computers, confirmed)
      return source.configureConfigurations(configuration)
    },
  })

  // A submission remembers how to run again: Retry passes the current draft (and the
  // deletions confirmed so far) so later edits apply instead of the failed request.
  function submit(submission: Submission, request?: OnboardingCompletionRequest, options?: OnboardingSubmissionOptions) {
    lastSubmission.current = submission
    const sequence = ++submissionSequence.current
    setOperationError(null)
    setFinishing(submission.isFinishing)
    let operation: Promise<unknown>
    try { operation = submission.run(request, options) } catch (error) { operation = Promise.reject(error) }
    void operation.then(() => {
      if (sequence === submissionSequence.current) setOperationError(null)
    }).catch((error: unknown) => {
      if (sequence === submissionSequence.current) setOperationError(errorMessage(error))
    }).finally(() => {
      if (sequence === submissionSequence.current) setFinishing(false)
    })
  }

  if (connectingDevice && source.applicationActions.connectDevice) {
    return <SiloWindow title="Silo" label="Connect another device">
      <div className="mx-auto w-full max-w-lg p-6">
        <h1 className="mb-3 text-sm font-semibold">Connect another device</h1>
        <ConnectDeviceForm authorize={source.applicationActions.authorizeDevice} onClose={() => setConnectingDevice(false)} connect={async (address, options) => {
          await (options ? source.applicationActions.connectDevice!(address, options) : source.applicationActions.connectDevice!(address))
          await updateSettings({ onboardingComplete: true })
          await store.flush()
          const error = store.getSnapshot().saveError
          if (error) throw new Error(error)
          onOpenApp?.()
        }} />
      </div>
    </SiloWindow>
  }
  return <OnboardingApp
    onConnectDevice={source.applicationActions.connectDevice ? () => setConnectingDevice(true) : undefined}
    source={onboarding}
    completed={completed}
    onOpenApp={onOpenApp}
    githubConnectionState={application?.github.state ?? "disconnected"}
    operationError={setup.error}
    repositoryOptions={application?.github.repositoryCatalog}
    repositoryPolicies={application?.github.computers}
    tokenConnected={application?.github.personalToken?.state === "connected"}
    onRetryDependencies={dependencies.retry}
    actions={{
      submitStep: (step, request, options) => {
        submit({ isFinishing: false, run: (current = request, confirmed = options) => {
          assertConfirmedDeletions(committed(), current.computerConfiguration.computers, confirmed)
          return source.submitSetupStep(step, current)
        } })
      },
      startComputer: (computer) => source.applicationActions.startComputer(computer),
      refreshSetupState: () => { void source.refresh() },
      connectGitHub: () => source.applicationActions.connectGitHub?.(),
      cancelGitHubConnection: () => source.applicationActions.cancelGitHubConnection?.(),
      reopenGitHubAuthorization: () => source.applicationActions.reopenGitHubAuthorization?.(),
      saveComputerConfiguration: (request, options) => {
        submit(configurationSubmission(request, options))
      },
      retryComputerSetup: (request, options) => {
        if (finishing) return
        const previous = lastSubmission.current
        if (previous) submit(previous, request, options)
        else if (application?.computerConfigurationOperation?.status === "failed") {
          submit(configurationSubmission(application.computerConfigurationOperation.candidate), request, options)
        }
      },
      finishSetup: (request, options) => {
        if (finishing) return
        submit({ isFinishing: true, run: async (current = request, confirmed = options) => {
          assertConfirmedDeletions(committed(), current.computerConfiguration.computers, confirmed)
          await source.finishSetup(current, async () => {
            await updateSettings({ ...current.applications, onboardingComplete: true })
            const error = store.getSnapshot().saveError
            if (error) throw new Error(error)
          })
          setCompleted(true)
        } })
      },
    }}
  />
}
