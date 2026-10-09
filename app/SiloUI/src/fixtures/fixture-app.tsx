import { useEffect, useMemo, useState } from "react"

import { LinuxDesktopPreview } from "./linux-desktop-preview"
import { ApplicationPreview } from "./application-preview"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { OnboardingPreview } from "./onboarding-preview"
import type { OnboardingCompletionRequest } from "@/features/onboarding/model/onboarding-source"
import { applicationPreviewAfterSetup } from "@/fixtures/onboarding-handoff"
import {
  applicationSourceForScenario,
  githubManagementFixtureModeFromSearch,
  repositoryPushFixtureModeFromSearch,
  computerConfigurationFixtureModeFromSearch,
  systemIssueFixtureModeFromSearch,
  computerFixtureModeFromSearch,
} from "@/fixtures/application-scenarios"
import { backupFixtureModeFromSearch } from "@/fixtures/application-backup"
import { activityFixtureModeFromSearch, activityFixtureStepCount } from "@/fixtures/application-activity"
import { githubStateFromSearch, onboardingScenarios, repositoryFixtures, scenarioFromSearch } from "@/fixtures/scenarios"
import { surfaceFromSearch } from "@/fixtures/surfaces"
import { StatusBarPreview } from "@/fixtures/status-bar-preview"
import { statusBarFixtureModeFromSearch } from "@/fixtures/status-bar-scenarios"
import type { StatusBarRoute } from "@/features/status-bar/status-bar-types"
import { SettingsProvider, type SettingsStore } from "@/features/preferences/settings-store"
import { settingsForFixture } from "./settings"
import type { DependencyRuntime } from "@/desktop/dependencies"
import { resourceFixtureModeFromSearch, withResourceFixture } from "./application-resources"
import { operationQueueFromSearch } from "./operation-queue"
import { RuntimeMigrationBoundary } from "@/desktop/runtime-migration-boundary"
import { createFixtureMigrationBackend, fixtureBackupForMode, preUpgradeBackupFixtureModeFromSearch } from "./pre-upgrade-backup"
import { createFixtureEditorInclude, editorIncludeFixtureModeFromSearch } from "./editor-include"
import { MacosComputersContext } from "@/features/macos-computers/model/macos-computers"
import { createFixtureMacosComputersStore } from "./macos-computers"
import { createComputerUseBridge } from "@/desktop/computer-use-bridge"
import { ComputerUseProvider } from "@/desktop/computer-use-provider"
import { createFixturePreparationBackend, preparationFixtureFromSearch } from "./preparation"
import { createPreparationStore, PreparationProvider } from "@/desktop/preparation"
import { chatGptFixtureFromSearch, computerUseFixtureFromSearch, createFixtureComputerUseBackend, withComputerUseFixture, withDevicesFixture } from "./computer-use"
import { createFixtureUnseenResult, unseenResultFixtureModeFromSearch } from "./transfer-result-notice"

export function FixtureApp({ nativeOnboardingComplete = false, nativeDependencies = null, nativeOperations = false, settingsStore }: { nativeOnboardingComplete?: boolean; nativeDependencies?: DependencyRuntime | null; nativeOperations?: boolean; settingsStore?: SettingsStore }) {
  const source = applicationSourceForScenario(scenarioFromSearch(window.location.search))
  return <SettingsProvider store={settingsStore} initialSettings={settingsForFixture(source)}><FixtureAppContent nativeOnboardingComplete={nativeOnboardingComplete} nativeDependencies={nativeDependencies} nativeOperations={nativeOperations} /></SettingsProvider>
}

function FixtureAppContent({ nativeOnboardingComplete, nativeDependencies, nativeOperations }: { nativeOnboardingComplete: boolean; nativeDependencies: DependencyRuntime | null; nativeOperations: boolean }) {
  const [surface, setSurface] = useState(() => surfaceFromSearch(window.location.search))
  const [completedSetup, setCompletedSetup] = useState<OnboardingCompletionRequest | null>(null)
  const [statusBarHandoff, setStatusBarHandoff] = useState<{ source: ApplicationSource; route?: StatusBarRoute } | null>(null)
  const scenario = scenarioFromSearch(window.location.search)
  const githubState = githubStateFromSearch(window.location.search)
  const computerMode = computerFixtureModeFromSearch(window.location.search)
  const computerConfigurationMode = computerConfigurationFixtureModeFromSearch(window.location.search)
  const systemIssueMode = systemIssueFixtureModeFromSearch(window.location.search)
  const repositoryPushMode = repositoryPushFixtureModeFromSearch(window.location.search)
  const githubManagementMode = githubManagementFixtureModeFromSearch(window.location.search)
  const activityMode = activityFixtureModeFromSearch(window.location.search)
  const backupMode = backupFixtureModeFromSearch(window.location.search)
  const resourceMode = resourceFixtureModeFromSearch(window.location.search)
  const statusBarMode = statusBarFixtureModeFromSearch(window.location.search)
  const [activityStep, setActivityStep] = useState(0)
  const [dependencyFixtureRecovered, setDependencyFixtureRecovered] = useState(false)
  const operationQueue = operationQueueFromSearch(window.location.search)
  // One backup per preview, so the migration screen and Settings, General agree about it.
  // The migration view always has one; elsewhere it appears only when asked for.
  const backupFixtureMode = preUpgradeBackupFixtureModeFromSearch(window.location.search) ?? (surface === "migration" ? "present" : undefined)
  const preUpgradeBackup = useMemo(() => backupFixtureMode ? fixtureBackupForMode(backupFixtureMode, 1_500) : undefined, [backupFixtureMode])
  // An export or import result Silo has not shown yet. The screen about the backup shows and acknowledges it;
  // the application shows it as a notification when that screen does not appear.
  const unseenResultMode = unseenResultFixtureModeFromSearch(window.location.search)
  const unseenResult = useMemo(() => unseenResultMode ? createFixtureUnseenResult(unseenResultMode) : undefined, [unseenResultMode])
  const migrationBackend = useMemo(() => preUpgradeBackup ? createFixtureMigrationBackend(preUpgradeBackup, unseenResult) : undefined, [preUpgradeBackup, unseenResult])
  const editorIncludeMode = editorIncludeFixtureModeFromSearch(window.location.search)
  const editorInclude = useMemo(() => editorIncludeMode ? createFixtureEditorInclude() : undefined, [editorIncludeMode])
  const baseSource = withResourceFixture(completedSetup ? applicationPreviewAfterSetup(completedSetup) : applicationSourceForScenario(scenario, githubState, computerMode, computerConfigurationMode, systemIssueMode, repositoryPushMode, activityMode, activityStep, githubManagementMode), resourceMode)
  const computerUseMode = computerUseFixtureFromSearch(window.location.search)
  const chatGptMode = chatGptFixtureFromSearch(window.location.search)
  const chatGptRemoteMode = chatGptFixtureFromSearch(window.location.search, "chatgpt-remote")
  const computerUseBridge = useMemo(() => computerUseMode || chatGptMode || chatGptRemoteMode ? createComputerUseBridge(createFixtureComputerUseBackend(computerUseMode ?? "ready", chatGptMode ?? "ready", chatGptRemoteMode ?? chatGptMode ?? "ready")) : null, [computerUseMode, chatGptMode, chatGptRemoteMode])
  const macosComputersStore = useMemo(() => createFixtureMacosComputersStore(), [])
  const preparationMode = preparationFixtureFromSearch(window.location.search)
  const preparationStore = useMemo(() => preparationMode ? createPreparationStore(createFixturePreparationBackend(preparationMode)) : null, [preparationMode])
  const queuedSource = operationQueue ? { ...baseSource, operationQueue } : baseSource
  const sourceWithComputerUse = computerUseMode ? withComputerUseFixture(queuedSource, computerUseMode) : queuedSource
  const fixtureSource = computerUseBridge ? withDevicesFixture(sourceWithComputerUse) : sourceWithComputerUse

  useEffect(() => {
    const stepCount = activityFixtureStepCount(activityMode)
    if (stepCount <= 1) return
    const timer = window.setInterval(() => {
      setActivityStep((current) => {
        if (current >= stepCount - 1) {
          window.clearInterval(timer)
          return current
        }
        return current + 1
      })
    }, 1_600)
    return () => window.clearInterval(timer)
  }, [activityMode])
  const content = (
    <>
      {surface === "desktop" ? <LinuxDesktopPreview /> : surface === "migration" && migrationBackend ? (
        <RuntimeMigrationBoundary backend={migrationBackend}>
          <ApplicationPreview
            key={scenario}
            source={fixtureSource}
            backupPreviewMode={backupMode}
            nativeOperations={nativeOperations}
            preUpgradeBackup={preUpgradeBackup}
            editorInclude={editorInclude}
            unseenResult={unseenResult}
          />
        </RuntimeMigrationBoundary>
      ) : surface === "app" ? (
        <ApplicationPreview
          key={`${scenario}:${githubState ?? "source"}:${computerMode ?? "source"}:${computerConfigurationMode ?? "source"}:${systemIssueMode ?? "source"}:${repositoryPushMode ?? "source"}:${activityMode ?? "source"}:${githubManagementMode ?? "source"}`}
          backupPreviewMode={backupMode}
          initialRoute={statusBarHandoff?.route}
          source={statusBarHandoff?.source ?? fixtureSource}
          nativeOperations={nativeOperations}
          preUpgradeBackup={preUpgradeBackup}
          editorInclude={editorInclude}
          unseenResult={unseenResult}
        />
      ) : surface === "status-bar" ? (
        <StatusBarPreview
          fixtureKey={`${scenario}:${githubState ?? "source"}:${computerMode ?? "source"}:${computerConfigurationMode ?? "source"}:${systemIssueMode ?? "source"}:${repositoryPushMode ?? "source"}:${activityMode ?? "source"}:${githubManagementMode ?? "source"}`}
          source={applicationSourceForScenario(scenario, githubState, computerMode, computerConfigurationMode, systemIssueMode, repositoryPushMode, activityMode, activityStep, githubManagementMode)}
          mode={statusBarMode}
          onOpenSilo={(snapshot, route) => {
            setStatusBarHandoff({ source: snapshot, route })
            const url = new URL(window.location.href)
            url.searchParams.set("view", "app")
            window.history.replaceState(null, "", url)
            setSurface("app")
          }}
        />
      ) : (
        <OnboardingPreview
          key={`${scenario}:${githubState ?? "source"}`}
          source={nativeDependencies ? {
            ...onboardingScenarios[scenarioFromSearch(window.location.search, "complete")],
            preflightChecks: nativeDependencies.checks,
          } : dependencyFixtureRecovered ? {
            ...onboardingScenarios[scenarioFromSearch(window.location.search, "complete")],
            preflightChecks: onboardingScenarios.complete.preflightChecks,
          } : onboardingScenarios[scenarioFromSearch(window.location.search, "complete")]}
          onRetryDependencies={nativeDependencies?.retry ?? (scenario === "dependency-failure" ? () => setDependencyFixtureRecovered(true) : undefined)}
          initialGitHubConnectionState={githubState}
          repositoryOptions={repositoryFixtures}
          initialCompleted={nativeOnboardingComplete}
          onOpenApp={() => {
            const url = new URL(window.location.href)
            url.searchParams.set("view", "app")
            window.history.replaceState(null, "", url)
            setSurface("app")
          }}
          actions={{
            finishSetup: setCompletedSetup,
          }}
        />
      )}
    </>
  )
  const withPreparation = preparationStore ? <PreparationProvider store={preparationStore}>{content}</PreparationProvider> : content
  const withMacos = <MacosComputersContext.Provider value={macosComputersStore}>{withPreparation}</MacosComputersContext.Provider>
  return computerUseBridge ? <ComputerUseProvider bridge={computerUseBridge}>{withMacos}</ComputerUseProvider> : withMacos
}
