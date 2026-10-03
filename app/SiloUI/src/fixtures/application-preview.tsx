import { computerTarget } from "@/features/application/model/connections"
import { fixtureLogPage, type LogLoader } from "@/features/application/model/logs"
import { useMemo, useState } from "react"
import { fixtureDirectoryLoader } from "./directory-loader"
import { fixtureFileTransfers } from "./file-transfers"
import { useApplicationFixture } from "@/fixtures/application-state"
import { ApplicationApp } from "@/features/application/application-app"
import type { ApplicationActions, ApplicationSource, SshAccessRequest, SshAccessComputer } from "@/features/application/model/application-source"
import type { ApplicationInitialRoute } from "@/features/application/model/use-application-navigation"
import { useBackupFixture, useUnavailableBackup, type BackupFixtureMode } from "@/fixtures/application-backup"
import { ApplicationCatalogProvider } from "@/features/preferences/application-catalog"
import { fixtureApplicationCatalog } from "@/fixtures/application-catalog"
import { SettingsProvider, useSettings } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"
import { PreUpgradeBackupProvider, type PreUpgradeBackupBackend } from "@/features/storage/pre-upgrade-backup"
import { EditorIncludeProvider, type EditorIncludeBackend } from "@/features/application/model/editor-include"
import type { FixtureUnseenResult } from "@/fixtures/transfer-result-notice"

const inactiveApplicationActions: ApplicationActions = {
  openNetworkPort: async () => undefined,
  saveSecret: () => undefined,
  removeSecret: () => undefined,
  retryRuntimeChecks: () => undefined,
  saveComputerConfiguration: () => undefined,
  dismissComputerConfigurationError: () => undefined,
  retryComputerConfiguration: () => undefined,
  pushRepository: () => undefined,
  startComputer: () => undefined,
  stopComputer: () => undefined,
  restartComputer: () => undefined,
  dismissComputerError: () => undefined,
  openTerminal: () => undefined,
  openEditor: () => undefined,
  disconnectGitHub: () => undefined,
}

export function ApplicationPreview({ source, actions, backupPreviewMode, initialRoute, nativeOperations = false, preUpgradeBackup, editorInclude, unseenResult }: {
  source: ApplicationSource
  actions?: Partial<ApplicationActions>
  backupPreviewMode?: BackupFixtureMode
  initialRoute?: ApplicationInitialRoute
  nativeOperations?: boolean
  /** The previous storage an upgrade kept; Settings, General shows it when given. */
  preUpgradeBackup?: PreUpgradeBackupBackend
  /** The SSH `Include` line Silo could not add; the application shows a notice for it when given. */
  editorInclude?: EditorIncludeBackend
  /** An export or import result that was not shown yet; the application starts with it, as after an upgrade. */
  unseenResult?: FixtureUnseenResult
}) {
  const { store } = useSettings(source.preferences)
  const [systemIntegrations] = useState(() => createFixtureSystemIntegrationStore(store))
  const application = nativeOperations
    ? <UnavailableApplicationPreview source={source} actions={actions} initialRoute={initialRoute} />
    : <FixtureApplicationPreview source={source} actions={actions} backupPreviewMode={backupPreviewMode} initialRoute={initialRoute} unseenResult={unseenResult} />
  const withBackup = preUpgradeBackup ? <PreUpgradeBackupProvider backend={preUpgradeBackup}>{application}</PreUpgradeBackupProvider> : application
  const withInclude = editorInclude ? <EditorIncludeProvider backend={editorInclude}>{withBackup}</EditorIncludeProvider> : withBackup
  return <SettingsProvider store={store}><SystemIntegrationProvider store={systemIntegrations}>{withInclude}</SystemIntegrationProvider></SettingsProvider>
}

function FixtureApplicationPreview({ source, actions, backupPreviewMode, initialRoute, unseenResult }: Parameters<typeof ApplicationPreview>[0]) {
  const fixture = useApplicationFixture(source)
  const [sshSettings, setSshSettings] = useState(() => new Map<string, SshAccessRequest>())
  const sshAccess = { computers: fixture.source.computers.map((w, index): SshAccessComputer => {
    const target = computerTarget(w)
    const seeded = fixture.source.sshAccess?.computers.find(access => access.computer === target)
    const settings = sshSettings.get(target) ?? seeded ?? { computer: target, enabled: index === 0, port: 2222 + index, bindAddress: "127.0.0.1", keys: [] }
    return { ...settings, keys: settings.keys ?? [], state: !settings.enabled ? "disabled" : w.state === "running" ? "listening" : "waiting", message: null, fingerprint: settings.enabled ? "SHA256:fixtureHostKeyForVisualPreviewOnly" : null, deviceName: seeded?.deviceName ?? w.device?.name ?? "Ada’s Mac mini", addresses: seeded?.addresses ?? ["127.0.0.1", "192.168.1.42"] }
  }) }

  const queryLogs = useMemo<LogLoader>(() => async request => {
    const computer = fixture.source.computers.find(item => (item.device?.computerId ?? item.configuration.id) === request.computerId && item.device?.id === request.deviceId)
    if (!computer) throw new Error("Computer unavailable")
    return fixtureLogPage(computer, request)
  }, [fixture.source.computers])
  const listComputerDirectory = useMemo(() => fixtureDirectoryLoader(source.computers), [source.computers])
  const fileTransfers = useMemo(() => fixtureFileTransfers(source.computers), [source.computers])
  // Read once, when the application opens: a result acknowledged on the screen about the backup is already seen.
  const [initialResult] = useState(() => unseenResult?.current())
  const backup = useBackupFixture({
    source: fixture.source,
    previewMode: backupPreviewMode,
    onRestoreComplete: fixture.onRestoreComplete,
    initialResult,
  })

  return <ApplicationCatalogProvider initialCatalog={fixtureApplicationCatalog}><ApplicationApp
    source={{ ...fixture.source, sshAccess, network: fixture.source.network ?? { computers: fixture.source.computers.map(w => ({ computer:w.configuration.name,error:null,ports:w.ports.map(p => ({port:p.port,hostPort:p.port,scheme:"http" as const,state:p.listening === true ? "reachable" as const : p.listening === false ? "waiting" as const : "unknown" as const,configured:true})) })) } }}
    initialRoute={initialRoute}
    routeRequest={initialRoute}
    backup={backup}
    actions={{
      ...inactiveApplicationActions,
      saveSshAccess: async request => { setSshSettings(current => new Map(current).set(request.computer, { ...request, keys: request.keys ?? current.get(request.computer)?.keys ?? sshAccess.computers.find(access => access.computer === request.computer)?.keys ?? [] })) },
      createCheckpoint: fixture.createCheckpoint,
      forkCheckpoint: fixture.forkCheckpoint,
      restoreCheckpoint: fixture.restoreCheckpoint,
      deleteCheckpoint: fixture.deleteCheckpoint,
      listComputerDirectory,
      fileTransfers,
      queryLogs,
      ...actions,
      saveSecret: (request) => {
        fixture.saveSecret(request)
        actions?.saveSecret?.(request)
      },
      removeSecret: (id) => {
        fixture.removeSecret(id)
        actions?.removeSecret?.(id)
      },
    }}
  /></ApplicationCatalogProvider>
}

function UnavailableApplicationPreview({ source, actions, initialRoute }: Parameters<typeof ApplicationPreview>[0]) {
  const backup = useUnavailableBackup(source)
  return <ApplicationCatalogProvider initialCatalog={fixtureApplicationCatalog}><ApplicationApp source={{ ...source, computerOperationsUnavailable: "Computer operations are not available in this Silo build. No computer state was changed." }} initialRoute={initialRoute} routeRequest={initialRoute} backup={backup} actions={{ ...inactiveApplicationActions, ...actions }} /></ApplicationCatalogProvider>
}
