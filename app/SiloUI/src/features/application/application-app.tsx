import { useLifecycleToasts } from "./model/use-lifecycle-toasts"
import { useRepositoryPushToasts } from "./components/use-repository-push-toasts"
import { computerTarget } from "./model/connections"
import { useBackendNotices } from "@/features/application/model/use-backend-notices"
import { useUpdates } from "@/features/updates/update-store"
import { updateCommands } from "@/features/updates/update-commands"
import { useAppMenu } from "@/desktop/app-menu"
import { UpdateNotice } from "@/features/updates/updates"
import { createDirectoryStore } from "@/features/application/model/directory-store"
import { useCallback, useEffect, useEffectEvent, useLayoutEffect, useRef, useState } from "react"

import type { BackupController } from "@/features/application/model/backup-source"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import { ApplicationShell, type ApplicationNavigationLoading } from "@/features/application/components/application-shell"
import { ComputerEditorDraftsProvider } from "@/features/computers/model/editor-drafts"
import { ApplicationCommandMenu } from "@/features/application/components/application-command-menu"
import { PreparationToast } from "@/features/application/components/preparation-toast"
import { ComputerConfigurationToast } from "@/features/application/components/computer-configuration-toast"
import { OperationQueueToast } from "@/features/application/components/operation-queue-panel"
import { QuitRequestConfirmation, type ConnectQuitConfirmation } from "@/features/application/components/quit-request-confirmation"
import { applicationCommands, type ComputerCommandRequest } from "@/features/application/components/application-commands"
import type { ApplicationActions, ApplicationSource, RepositoryPushOperation, RepositoryPushTarget, ComputerConfigurationOperation } from "@/features/application/model/application-source"
import { useApplicationNavigation, type ApplicationInitialRoute } from "@/features/application/model/use-application-navigation"
import { defaultStartupComputerIds } from "@/features/application/model/startup-computers"
import { AlphaNotice } from "@/features/application/components/alpha-notice"
import { EditorIncludeNotice } from "@/features/application/components/editor-include-notice"
import { ConnectionsSettings } from "@/features/application/components/connections-settings"
import { GeneralPage } from "@/features/application/pages/general-page"
import { GitHubPage } from "@/features/application/pages/github-page"
import { NotificationsPage } from "@/features/application/pages/notifications-page"
import { OverviewPage, type ComputerPageRequest } from "@/features/application/pages/overview-page"
import { useComputerTransfer } from "@/features/application/components/computer-transfer"
import { SecretsPage } from "@/features/application/pages/secrets-page"
import { SystemIssuePage } from "@/features/application/pages/system-issue-page"
import { FileTransfersProvider } from "@/features/application/components/use-file-transfers"
import { ComputersPage } from "@/features/application/pages/computers-page"
import { applicationPreferenceChanges, type ApplicationPreferenceSelection } from "@/features/preferences/model/application-preferences"
import { SettingsProvider, useSettings } from "@/features/preferences/settings-store"
import { remoteMacosComputerId, useMacosComputers } from "@/features/macos-computers/model/macos-computers"

function computerAttentionCounts(source: Pick<ApplicationSource, "computers" | "computerConfigurationOperation">): { errors: number; warnings: number } {
  const attentionByComputer = new Map(source.computers.map((computer) => [
    computer.configuration.id,
    computer.state === "failed" || computer.attention?.level === "error"
      ? "error" as const
      : computer.attention?.level === "warning"
        ? "warning" as const
        : null,
  ]))
  const operation = source.computerConfigurationOperation
  if (operation?.status === "failed" && operation.error.computer) {
    const failedComputer = operation.candidate.computers.find(({ name }) => name === operation.error.computer)
      ?? source.computers.find(({ configuration }) => configuration.name === operation.error.computer)?.configuration
    if (failedComputer) attentionByComputer.set(failedComputer.id, "error")
  }

  return [...attentionByComputer.values()].reduce((counts, attention) => {
    if (attention === "error") counts.errors += 1
    else if (attention === "warning") counts.warnings += 1
    return counts
  }, { errors: 0, warnings: 0 })
}

function navigationLoadingState(source: ApplicationSource, githubBusy: boolean, backupBusy: boolean): ApplicationNavigationLoading {
  const runningCategories = new Set(source.activities
    .filter(({ status }) => status === "running")
    .map(({ category }) => category))
  const githubSourceBusy = source.github.state === "connecting"
    || (source.github.computerOperations ?? []).some(({ status }) => status === "applying")

  return {
    tabs: {
      github: githubBusy || githubSourceBusy || runningCategories.has("github"),
      secrets: runningCategories.has("secrets"),
      system: source.runtimeRepair?.checking || runningCategories.has("system"),
    },
    computerSections: {
      overview: source.computerConfigurationOperation?.status === "applying"
        || source.computers.some(({ state }) => state === "starting")
        || backupBusy
        || runningCategories.has("computer")
        || runningCategories.has("backup"),
      files: source.repositoryPushOperations.some(({ status }) => status === "pushing")
        || runningCategories.has("git"),
    },
  }
}

// Request tokens only need to be unique: each one is consumed once by the page it opens.
let requestTokens = 0
const nextRequestToken = () => ++requestTokens

type ApplicationAppProps = {
  source: ApplicationSource
  actions: ApplicationActions
  backup: BackupController
  initialRoute?: ApplicationInitialRoute
  routeRequest?: ApplicationInitialRoute
  /** The desktop main window's Quit confirmation hook-up (`connectQuitConfirmation`). */
  connectQuitConfirmation?: ConnectQuitConfirmation
}

export function ApplicationApp(props: ApplicationAppProps) {
  return <SettingsProvider initialSettings={{
    ...props.source.preferences,
    startupComputerIds: props.source.preferences.startupComputerIds ?? defaultStartupComputerIds(props.source.computers),
  }}><ApplicationContent {...props} /></SettingsProvider>
}

function ApplicationContent({ source, actions, backup, initialRoute, routeRequest, connectQuitConfirmation }: ApplicationAppProps) {
  useBackendNotices()
  const updates = useUpdates()
  const installingUpdate = updates?.snapshot?.phase === "installing"
    || Boolean(updates?.pending && (updates.snapshot?.phase === "ready" || updates.snapshot?.retryAction === "install"))
  const [newComputerRequest, setNewComputerRequest] = useState(0)
  // A palette command that opens something on a computer's page (folder picker, Fork, Delete).
  const [computerRequest, setComputerRequest] = useState<ComputerPageRequest>()
  const [searchRequest, setSearchRequest] = useState(0)
  const [sidebarRequest, setSidebarRequest] = useState(0)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [directoryStore] = useState(() => createDirectoryStore(actions.listComputerDirectory))
  useLayoutEffect(() => {
    directoryStore.setLoader(actions.listComputerDirectory)
  }, [actions.listComputerDirectory, directoryStore])
  const previousFileStates = useRef(new Map<string, string>())
  useLayoutEffect(() => {
    const current = new Map(source.computers.map((computer) => [
      computerTarget(computer), `${computer.configuration.id}:${computer.state}:${computer.freshness}`,
    ]))
    for (const [name, state] of previousFileStates.current) {
      if (current.get(name) !== state) directoryStore.invalidateComputer(name)
    }
    previousFileStates.current = current
  }, [source.computers, directoryStore])

  const { settings, updateSettings } = useSettings()
  const { reduceMotion } = settings
  const applicationPreferences: ApplicationPreferenceSelection = {
    terminal: settings.terminal,
    editor: settings.editor,
    browser: settings.browser,
    terminalUseSystemDefault: settings.terminalUseSystemDefault,
    editorUseSystemDefault: settings.editorUseSystemDefault,
    browserUseSystemDefault: settings.browserUseSystemDefault,
    ...(settings.terminalPath && { terminalPath: settings.terminalPath }),
    ...(settings.editorPath && { editorPath: settings.editorPath }),
    ...(settings.browserPath && { browserPath: settings.browserPath }),
  }
  const activeRuntimeRepair = source.runtimeRepair
  const initialComputerId = initialRoute?.computer ? resolveComputerId(initialRoute.computer) : undefined
  const navigation = useApplicationNavigation(Boolean(activeRuntimeRepair), initialRoute && { ...initialRoute, computer: initialComputerId })
  const { tab: activeTab, computerSection, settingsSection } = navigation
  const computers = source.computers
  const [selectedComputerIds, setSelectedComputerIds] = useState<Set<string>>(() => new Set(
    source.computers
      .filter(({ configuration }) => configuration.id === initialComputerId)
      .map(({ configuration }) => configuration.id),
  ))
  const [logQuery, setLogQuery] = useState("")
  const [computerConfigurationOperation, setComputerConfigurationOperation] = useState<ComputerConfigurationOperation | null>(source.computerConfigurationOperation)
  const [repositoryPushOperations, setRepositoryPushOperations] = useState<RepositoryPushOperation[]>(source.repositoryPushOperations)
  const backupBusy = backup.state.operation?.kind === "running"
  const [githubBusy, setGitHubBusy] = useState(
    source.github.state === "connecting"
      || (source.github.computerOperations ?? []).some(({ status }) => status === "applying"),
  )
  const visibleTab = activeTab
  const visibleComputerSection = computerSection
  const applicationSource = {
    ...source,
    computers,
    computerConfigurationOperation,
    repositoryPushOperations,
    preferences: { ...source.preferences, ...settings },
  }
  const transfer = useComputerTransfer(backup, { source: applicationSource, openComputer: (id) => navigation.openComputer(id) })

  // macOS computers can be filtered on the Logs page, so their ids stay selected too.
  const macosSnapshot = useMacosComputers()?.snapshot
  const macosIds = [
    ...(macosSnapshot?.state?.computers.map(({ id }) => id) ?? []),
    ...Object.entries(macosSnapshot?.remote ?? {}).flatMap(([deviceId, remote]) => remote.state?.computers.map(({ id }) => remoteMacosComputerId(deviceId, id)) ?? []),
  ].join("\n")
  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    setSelectedComputerIds((current) => {
      const availableIds = new Set([...source.computers.map(({ configuration }) => configuration.id), ...macosIds.split("\n")])
      const next = new Set([...current].filter((id) => availableIds.has(id)))
      return next.size === current.size ? current : next
    })
  }, [source.computers, macosIds])

  // The latest native snapshot, for saves that settle after later snapshots arrived.
  const latestSource = useRef(source)
  useLayoutEffect(() => { latestSource.current = source }, [source])

  // Each optimistic state yields only to its own authoritative field, so a configuration
  // snapshot never discards a push in flight, or the reverse.
  useEffect(() => {
    // The native bridge clears or replaces the pending operation alongside its authoritative snapshot.
    // oxlint-disable-next-line react/set-state-in-effect
    setComputerConfigurationOperation(source.computerConfigurationOperation)
  }, [source.computerConfigurationOperation])
  useEffect(() => {
    // The native bridge replaces local push progress with its authoritative operation result.
    // oxlint-disable-next-line react/set-state-in-effect
    setRepositoryPushOperations(source.repositoryPushOperations)
  }, [source.repositoryPushOperations])

  function changeApplicationPreferences(next: ApplicationPreferenceSelection) {
    void updateSettings(applicationPreferenceChanges(applicationPreferences, next))
  }

  function updateConfigurations(configurations: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[]) {
    const candidate = { schemaVersion: 1 as const, computers: configurations }
    setComputerConfigurationOperation({
      id: "local-computer-configuration",
      status: "applying",
      candidate,
      progressEvents: [],
      result: null,
      error: null,
    })
    const outcome = actions.saveComputerConfiguration(candidate, baseline)
    // Without a promise, only the next snapshot reports the change; keep the optimistic state until then.
    if (!outcome || typeof outcome.then !== "function") return Promise.resolve()
    // Once the save settles the native snapshot is authoritative: adopt the latest one. A no-op
    // save publishes nothing, and a late stale-baseline rejection must not restore the operation
    // from when the save began. Re-raise a rejection so the editor can react.
    const settle = () => setComputerConfigurationOperation(latestSource.current.computerConfigurationOperation)
    return outcome.then(settle, (cause: unknown) => {
      settle()
      throw cause
    })
  }

  function pushRepository(computer: string, repositoryPath: string, commitCount: number, target: RepositoryPushTarget) {
    setRepositoryPushOperations((current) => [
      ...current.filter((operation) => operation.computer !== computer || operation.repositoryPath !== repositoryPath),
      { computer, repositoryPath, commitCount, target, status: "pushing" },
    ])
    actions.pushRepository(computer, repositoryPath, target)
  }

  const dismissRepositoryPush = useCallback((computer: string, repositoryPath: string) => {
    if (actions.dismissRepositoryPush) { actions.dismissRepositoryPush(computer, repositoryPath); return }
    setRepositoryPushOperations((current) => current.filter((operation) => operation.computer !== computer || operation.repositoryPath !== repositoryPath))
  }, [actions])

  useLifecycleToasts(applicationSource, actions)
  useRepositoryPushToasts(repositoryPushOperations, {
    onPush: pushRepository,
    onDismiss: dismissRepositoryPush,
    queue: source.operationQueue,
    onCancel: actions.cancelOperation,
    resolveComputer: target => {
      const configuration = computers.find(computer => computerTarget(computer) === target)?.configuration
      return configuration ? { id: configuration.id, name: configuration.name } : undefined
    },
  })

  function resolveComputerId(value: string) {
    return (source.computers.find((computer) => computer.configuration.id === value)
      ?? source.computers.find((computer) => computerTarget(computer) === value))?.configuration.id ?? value
  }

  // History never keeps a page for a computer that no longer exists (deleted, or gone after a
  // refresh): its entries become the Computers list in place, so Back cannot land on it.
  const { forgetComputers } = navigation
  useEffect(() => {
    const known = new Set<string>()
    for (const computer of source.computers) known.add(computer.configuration.id)
    for (const configuration of computerConfigurationOperation?.candidate.computers ?? []) known.add(configuration.id)
    forgetComputers((computer) => known.has(computer))
  }, [source.computers, computerConfigurationOperation, forgetComputers])

  function navigateCommand(route: ApplicationInitialRoute) {
    const wantsSection = Boolean(route.computerSection && route.computerSection !== "overview")
    // A computer without a detail section deep-links into that computer's overview page.
    if (route.computer && !wantsSection) { navigation.openComputer(resolveComputerId(route.computer), route.computerTab); return }
    if (route.computer || wantsSection) {
      setSelectedComputerIds(new Set(source.computers
        .filter(({ configuration }) => route.computer !== undefined && configuration.id === resolveComputerId(route.computer))
        .map(({ configuration }) => configuration.id)))
    }
    if (route.computerSection || route.computer) navigation.selectComputerSection(route.computerSection ?? "overview")
    else if (route.settingsSection) navigation.selectSettingsSection(route.settingsSection)
    else if (route.tab) navigation.selectTab(route.tab)
  }

  const navigateRequested = useEffectEvent(navigateCommand)
  useEffect(() => {
    // Apply an external status-panel navigation request to the existing window.
    // oxlint-disable-next-line react/set-state-in-effect
    if (routeRequest) navigateRequested(routeRequest)
  }, [routeRequest])

  const canCreateComputer = computerConfigurationOperation === null
  const canImport = !backupBusy
  const canCheckUpdates = Boolean(updates && !updates.pending && !["checking", "downloading", "installing"].includes(updates.snapshot?.phase ?? ""))
  function requestNewComputer() {
    navigation.selectComputerSection("overview")
    setNewComputerRequest(nextRequestToken())
  }
  function requestOnComputerPage(computerId: string, request: ComputerCommandRequest) {
    navigation.openComputer(computerId)
    setComputerRequest({ token: nextRequestToken(), computerId, request })
  }

  // The review popover anchors to the computer list's Add button, so show the list first.
  const openImport = () => { navigation.selectComputerSection("overview"); navigation.closeComputer(); void transfer.beginImport() }
  const nativeMenu = useAppMenu({ ready: true, busy: installingUpdate,
    canGoBack: navigation.canGoBack, canGoForward: navigation.canGoForward,
    canCreateComputer, canImport, canCheckUpdates, sidebarCollapsed,
  }, (command) => {
    if (installingUpdate) return
    switch (command) {
      case "settings": navigation.selectSettingsSection("general"); break
      case "check-updates":
        navigation.selectSettingsSection("general")
        if (canCheckUpdates) updates?.check()
        break
      case "new-computer":
        if (canCreateComputer) requestNewComputer()
        break
      case "import-computer":
        if (canImport) openImport()
        break
      case "search": setSearchRequest(value => value + 1); break
      case "toggle-sidebar": setSidebarRequest(value => value + 1); break
      case "go-back": navigation.goBack(); break
      case "go-forward": navigation.goForward(); break
      case "go-computers": navigation.selectComputerSection("overview"); break
      case "go-files": navigation.selectComputerSection("files"); break
      case "go-logs": navigation.selectComputerSection("logs"); break
      case "go-network": navigation.selectComputerSection("network"); break
      case "go-activity": navigation.selectComputerSection("activity"); break
      case "go-github": navigation.selectTab("github"); break
      case "go-secrets": navigation.selectTab("secrets"); break
    }
  })

  return (
    // Keeps unsaved computer edits while navigating between sections (I-37).
    <ComputerEditorDraftsProvider>
    <FileTransfersProvider api={actions.fileTransfers}>
    <ApplicationShell
      toggleSidebarRequest={sidebarRequest}
      onSidebarCollapsedChange={setSidebarCollapsed}
      navigationDisabled={installingUpdate}
      notice={<UpdateNotice onOpen={() => navigation.selectSettingsSection("general")} />}
      banner={<><AlphaNotice /><EditorIncludeNotice /></>}
      activeTab={visibleTab}
      computerSection={visibleComputerSection}
      settingsSection={settingsSection}
      systemIssueStatus={activeRuntimeRepair?.status ?? null}
      computerAttention={computerAttentionCounts(applicationSource)}
      navigationLoading={navigationLoadingState(applicationSource, githubBusy, backupBusy)}
      onTabChange={navigation.selectTab}
      onComputerSectionChange={navigation.selectComputerSection}
      onSettingsSectionChange={navigation.selectSettingsSection}
      canGoBack={navigation.canGoBack}
      canGoForward={navigation.canGoForward}
      onGoBack={navigation.goBack}
      onGoForward={navigation.goForward}
      reduceMotion={reduceMotion}
      commandMenu={<ApplicationCommandMenu nativeShortcuts={nativeMenu} openRequest={searchRequest} disabled={installingUpdate} commands={[...applicationCommands(applicationSource, actions, navigateCommand, {
        onImportComputer: canImport ? openImport : undefined,
        onNewComputer: canCreateComputer ? requestNewComputer : undefined,
        onExportComputer: canImport ? (name) => { void transfer.exportComputer(name) } : undefined,
        onComputerRequest: requestOnComputerPage,
      }), ...updateCommands(updates, () => navigation.selectSettingsSection("general"))]} />}
    >
      {/* One toast reflects computer-changing operations wherever the user is, so progress and
          Cancel never vanish while the work continues. It renders nothing inline. */}
      <OperationQueueToast queue={source.operationQueue} onCancel={actions.cancelOperation} />
      <ComputerConfigurationToast operation={source.computerConfigurationOperation} computers={source.computers} onOpen={(id) => navigation.openComputer(id)} />
      <PreparationToast />
      <QuitRequestConfirmation connect={connectQuitConfirmation} />
      <section id="application-panel-computers" role="region" aria-labelledby="application-nav-computers" hidden={visibleTab !== "computers"} className="h-full min-h-0 overflow-hidden">
        {visibleComputerSection === "overview" ? (
          <OverviewPage notifyOperations={false} active={visibleTab === "computers"} newComputerRequest={newComputerRequest} onNewComputerRequestHandled={(id) => setNewComputerRequest(current => current === id ? 0 : current)}
            computerRequest={computerRequest} onComputerRequestHandled={(token) => setComputerRequest(current => current?.token === token ? undefined : current)} onExportComputer={transfer.exportComputer} onImportComputer={openImport} importPopover={transfer.importPopover} backup={backup} source={applicationSource}
            selectedComputerId={navigation.computer ? resolveComputerId(navigation.computer) : null}
            computerTab={navigation.computerTab}
            onOpenComputer={(id, tab) => navigation.openComputer(id, tab)}
            onCloseComputer={() => navigation.closeComputer()}
            onSelectComputerTab={(tab) => navigation.selectComputerTab(tab)}
            onNavigate={navigateCommand}
            actions={{ ...actions, dismissComputerConfigurationError: () => {
            if (computerConfigurationOperation?.status !== "failed") return
            actions.dismissComputerConfigurationError()
            setComputerConfigurationOperation(null)
          } }} onConfigurationsChange={updateConfigurations} />
        ) : (
          <ComputersPage
            notifyOperations={false}
            source={applicationSource}
            onSectionChange={navigation.selectComputerSection}
            network={source.network}
            networkError={source.networkError}
            networkActions={actions}
            onOpenEditor={actions.openEditor}
            editor={applicationPreferences.editor}
            directoryStore={directoryStore}
            active={visibleTab === "computers"}
            computers={computers}
            activities={source.activities}
            selectedComputerIds={selectedComputerIds}
            section={visibleComputerSection}
            logQuery={logQuery}
            repositoryPushOperations={repositoryPushOperations}
            browser={applicationPreferences.browser}
            onComputerFilterChange={setSelectedComputerIds}
            onLogQueryChange={setLogQuery}
            onPushRepository={pushRepository}
            operationQueue={source.operationQueue}
            onDismissRepositoryPush={dismissRepositoryPush}
            onCreateComputer={canCreateComputer && !installingUpdate ? requestNewComputer : undefined}
          />
        )}
      </section>
      <section id="application-panel-github" role="region" aria-labelledby="application-nav-github" hidden={visibleTab !== "github"} className="h-full min-h-0 overflow-hidden">
        <GitHubPage source={applicationSource} actions={actions} onBusyChange={setGitHubBusy} />
      </section>
      <section id="application-panel-secrets" role="region" aria-labelledby="application-nav-secrets" hidden={visibleTab !== "secrets"}><SecretsPage source={applicationSource} onSaveSecret={actions.saveSecret} onRemoveSecret={actions.removeSecret} onRetrySecret={actions.retrySecret} /></section>
      {activeRuntimeRepair && (
        <section id="application-panel-system" role="region" aria-labelledby="application-nav-system" hidden={visibleTab !== "system"}>
          <SystemIssuePage issue={activeRuntimeRepair} actions={actions} />
        </section>
      )}
      <section id="application-panel-settings" role="region" aria-labelledby="application-nav-settings" hidden={visibleTab !== "settings"}>
        <div hidden={settingsSection !== "general"}>
          <GeneralPage source={source} applicationPreferences={applicationPreferences} onApplicationPreferencesChange={changeApplicationPreferences} reduceMotion={reduceMotion} onReduceMotionChange={(enabled) => { void updateSettings({ reduceMotion: enabled }) }} />
        </div>
        <div hidden={settingsSection !== "connections"} className="mx-auto w-full max-w-4xl px-4 py-5 sm:px-6 sm:py-6"><ConnectionsSettings source={source} actions={actions} active={visibleTab === "settings" && settingsSection === "connections"} /></div>
        <div hidden={settingsSection !== "notifications"}><NotificationsPage /></div>
      </section>
    </ApplicationShell>
    </FileTransfersProvider>
    </ComputerEditorDraftsProvider>
  )
}
