import { ShutdownBoundary } from "@/desktop/shutdown-boundary"
import { RuntimeMigrationBoundary } from "@/desktop/runtime-migration-boundary"
import { desktopUpdateBackend } from "@/desktop/updates"
import { desktopPreUpgradeBackupBackend } from "@/desktop/pre-upgrade-backup"
import { PreUpgradeBackupProvider } from "@/features/storage/pre-upgrade-backup"
import { desktopEditorIncludeBackend } from "@/desktop/editor-include"
import { EditorIncludeProvider } from "@/features/application/model/editor-include"
import { UpdatesProvider, useUpdates } from "@/features/updates/update-store"
import { useMainRoute } from "@/desktop/use-main-route"
import { useEffect, useMemo, useState, type ReactNode } from "react"
import { useStableCallback } from "@/lib/use-stable-callback"
import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import type { BackupController } from "@/features/application/model/backup-source"
import type { SiloPreflightCheck } from "@/contracts/silo"
import { SiloWindow } from "@/components/silo-window"
import { useDependencyStore, type DependencyStore } from "@/desktop/dependencies"
import { ProductionOnboarding } from "@/desktop/production-onboarding"
import { localUpdatingNotice, useProductionSource, type ProductionSource } from "@/desktop/production-source"
import { StatusPanel } from "@/desktop/status-panel"
import { ApplicationLoading, StatusPanelUnavailable } from "@/desktop/application-loading"
import { ApplicationApp } from "@/features/application/application-app"
import { connectQuitConfirmation } from "@/desktop/settings"
import { useSettingsStore } from "@/features/preferences/settings-store"

/** Painted before any native call at startup, so the window Rust has shown is never blank. */
export function StartupLoading({ statusPanel = false }: { statusPanel?: boolean }) {
  if (statusPanel) return <ApplicationLoading configurations={[]} statusPanel />
  return <SiloWindow title="Silo" label="Silo"><span role="status" className="sr-only">Opening Silo…</span></SiloWindow>
}

export function Unavailable({ message, retry, retryLabel = "Retry checks", checks = [], checking = false }: { message: string; retry?: () => void; retryLabel?: string; checks?: SiloPreflightCheck[]; checking?: boolean }) {
  return (
    <SiloWindow title="Silo" label="Silo unavailable">
      <div className="grid flex-1 place-items-center p-6">
        <div className="max-w-lg rounded-lg border border-destructive/25 bg-destructive/[.06] p-4" role="alert">
          <h1 className="text-sm font-semibold">Silo could not load</h1>
          {checks.length ? checks.map((check) => (
            <div key={check.id} className="mt-2 text-xs">
              <p>{check.title}: {check.detail}</p>
              <p className="mt-1 whitespace-pre-wrap text-muted-foreground">{check.remediation}</p>
            </div>
          )) : <p className="mt-1 whitespace-pre-wrap text-xs text-muted-foreground">{message}</p>}
          {retry && <button type="button" disabled={checking} className="mt-3 rounded-md border px-3 py-1.5 text-xs disabled:opacity-50" onClick={retry}>{checking ? "Checking…" : retryLabel}</button>}
        </div>
      </div>
    </SiloWindow>
  )
}

type ProductionSurfaceProps = { source: ProductionSource; dependencyStore: DependencyStore | null; statusPanel?: boolean }
export function ProductionSurface(props: ProductionSurfaceProps) {
  return props.statusPanel ? <ProductionContent {...props} /> : <MainSurface {...props} />
}
function MainSurface(props: ProductionSurfaceProps) {
  // Quit drains accepted setup first; name that work while the overlay waits for it.
  const { setupDrain } = useProductionSource(props.source)
  return <ShutdownBoundary pendingWork={setupDrain}><RuntimeMigrationBoundary><PreUpgradeBackupProvider backend={desktopPreUpgradeBackupBackend}><EditorIncludeProvider backend={desktopEditorIncludeBackend}><ProductionContent {...props} /></EditorIncludeProvider></PreUpgradeBackupProvider></RuntimeMigrationBoundary></ShutdownBoundary>
}
function ProductionContent({ source, dependencyStore, statusPanel = false }: ProductionSurfaceProps) {
  const current = useProductionSource(source)
  const routeRequest = useMainRoute(!statusPanel)
  const dependencies = useDependencyStore(dependencyStore)
  const settingsStore = useSettingsStore()
  const [preparingUpdate, setPreparingUpdate] = useState(false)
  const updateBackend = useMemo(() => ({ ...desktopUpdateBackend, install: async (stopComputers: boolean) => {
    setPreparingUpdate(true)
    try {
      await settingsStore.flush()
      const { saveError, writeProtected } = settingsStore.getSnapshot()
      if (saveError || writeProtected) throw new Error(saveError ?? "Settings are protected from writes")
      return await desktopUpdateBackend.install(stopComputers)
    } finally {
      setPreparingUpdate(false)
    }
  } }), [settingsStore])
  const checks = dependencies?.checks
  const [previousFailures, setPreviousFailures] = useState<SiloPreflightCheck[]>([])
  useEffect(() => {
    const failures = checks?.filter(({ status }) => ["failed", "unavailable", "timeout"].includes(status)) ?? []
    // Retain actionable guidance while the read-only checks run again.
    // oxlint-disable-next-line react/set-state-in-effect
    if (checks && !checks.some(({ status }) => status === "pending")) setPreviousFailures(failures)
  }, [checks])
  const checking = checks?.some(({ status }) => status === "pending") ?? false
  const failures = checking ? previousFailures : checks?.filter(({ status }) => ["failed", "unavailable", "timeout"].includes(status)) ?? []
  // Initialize again rather than refresh: after a failed start it also restores live
  // events, polling and refresh-on-focus; once live it only refreshes.
  const retryChecks = useStableCallback(() => { dependencies?.retry(); void source.initialize().catch((error: unknown) => console.error("Silo live updates:", error)) })
  // Finish persists completion; keep this session on its preferences screen until Open Silo.
  // Settings that could not be read (or a damaged, write-protected file) report defaults,
  // so a missing completion flag is unknown, not "new user": never route to onboarding
  // then. Onboarding could not save completion in that state anyway.
  const [onboardingActive, setOnboardingActive] = useState(() => {
    const { revision, writeProtected, settings } = settingsStore.getSnapshot()
    return revision >= 0 && !writeProtected && !settings.onboardingComplete
  })
  if (!statusPanel && onboardingActive && dependencies) {
    return <UpdatesProvider backend={updateBackend}><UpdateInstallationBoundary preparing={preparingUpdate}><ProductionOnboarding application={current.source} dependencies={dependencies} source={source} onOpenApp={() => setOnboardingActive(false)} /></UpdateInstallationBoundary></UpdatesProvider>
  }
  if (!current.source) {
    if (current.loading && !current.error && !failures.length) return <ApplicationLoading configurations={current.savedConfigurations ?? []} statusPanel={statusPanel} />
    const message = current.error ?? "The native application state is unavailable. No computer state changed."
    if (statusPanel) return <StatusPanelUnavailable message={message} retry={current.loading ? undefined : retryChecks} />
    return <Unavailable message={message} checks={failures} checking={checking} retry={current.loading ? undefined : retryChecks} />
  }
  const remoteOnly = Boolean(current.source.devices?.length)
    && !current.source.computers.some(computer => !computer.device)
  const localRuntimeFailures = remoteOnly ? [] : failures
  // Connected devices stay usable while this device's computers update; say why
  // the local ones are missing.
  const notice = current.localUpdating ? localUpdatingNotice : undefined
  return statusPanel
    ? <StatusPanel source={current.source} actions={source.statusActions} notice={notice} />
    : <UpdatesProvider backend={updateBackend}><UpdateInstallationBoundary preparing={preparingUpdate}>{notice && <p role="status" className="border-b bg-muted px-4 py-2 text-xs">{notice}</p>}<MainApplication routeRequest={routeRequest} source={current.source} applicationActions={source.applicationActions} retryChecks={retryChecks} backup={current.backup}
      repair={localRuntimeFailures.length ? {
        status: "unavailable", checking,
        reason: failures.map(({ title, detail }) => `${title}: ${detail}`).join("\n"),
        recovery: [...new Set(failures.map(({ remediation }) => remediation).filter(Boolean))].join("\n"),
      } : undefined} /></UpdateInstallationBoundary></UpdatesProvider>
}

type RuntimeRepair = NonNullable<ApplicationSource["runtimeRepair"]>
/** Keeps the source and actions handed to the application stable across unrelated renders. */
function MainApplication({ source, applicationActions, retryChecks, backup, routeRequest, repair }: {
  source: ApplicationSource; applicationActions: ApplicationActions; retryChecks: () => void
  backup: BackupController; routeRequest: ReturnType<typeof useMainRoute>; repair: RuntimeRepair | undefined
}) {
  const repairStatus = repair?.status, repairChecking = repair?.checking, repairReason = repair?.reason, repairRecovery = repair?.recovery
  const applicationSource = useMemo(() => repairStatus
    ? { ...source, runtimeRepair: { status: repairStatus, checking: repairChecking, reason: repairReason, recovery: repairRecovery } as RuntimeRepair }
    : source, [source, repairStatus, repairChecking, repairReason, repairRecovery])
  const actions = useMemo(() => ({ ...applicationActions, retryRuntimeChecks: retryChecks }), [applicationActions, retryChecks])
  return <ApplicationApp routeRequest={routeRequest} connectQuitConfirmation={connectQuitConfirmation} source={applicationSource} actions={actions} backup={backup} />
}

function UpdateInstallationBoundary({ preparing, children }: { preparing: boolean; children: ReactNode }) {
  const updates = useUpdates()
  const installing = updates?.snapshot?.phase === "installing"
  const blocked = preparing || installing
  return <div className="flex h-full min-h-0 flex-col">
    {blocked && <div role="status" className="border-b bg-muted px-4 py-2 text-xs">{installing ? updates?.snapshot?.installStatus ?? "Installing update. Silo will restart…" : "Preparing update…"}</div>}
    <div className="flex min-h-0 flex-1 flex-col" inert={blocked} aria-busy={blocked}>{children}</div>
  </div>
}
