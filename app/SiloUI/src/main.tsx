import { isMac } from "@/lib/platform"
import { errorMessage } from "@/lib/error-message"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { invoke, isTauri } from "@tauri-apps/api/core"
import { getCurrentWindow } from "@tauri-apps/api/window"

import "./index.css"
import { SiloWindow } from "@/components/silo-window"
import { createComputerUseBridge, nativeComputerUseBackend } from "@/desktop/computer-use-bridge"
import { createPreparationStore, nativePreparationBackend, PreparationProvider } from "@/desktop/preparation"
import { ComputerUseProvider } from "@/desktop/computer-use-provider"
import { ApplicationCatalogLoader } from "@/desktop/application-catalog-loader"
import { createApplicationService } from "@/desktop/applications"
import { createNativeDependencyStore } from "@/desktop/dependencies"
import { desktopViewerRoute } from "@/desktop/linux-desktop-state"
import { loadWithRetry } from "@/desktop/lazy-load"
import { createProductionSource } from "@/desktop/production-source"
import { createDesktopSettingsStore, connectSettingsLifecycle } from "@/desktop/settings"
import { connectSystemIntegrationLifecycle, createDesktopSystemIntegrationStore } from "@/desktop/system-integrations"
import { ApplicationCatalogProvider } from "@/features/preferences/application-catalog"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { initializeTheme } from "@/features/preferences/theme"

// Each window shows one large tree. It loads on demand, in parallel with the native reads below.
const loadSurface = loadWithRetry(() => import("@/desktop/production-surface"))
const loadViewer = loadWithRetry(() => import("@/desktop/linux-desktop-viewer"))
const loadFailure = () => import("@/desktop/startup-failure")

const desktop = isTauri()
const windowLabel = desktop ? getCurrentWindow().label : ""
const statusPanel = windowLabel === "status"
document.documentElement.classList.toggle("native-status", statusPanel)
document.documentElement.classList.toggle("native-material", windowLabel === "main" && isMac())
const settings = createDesktopSettingsStore({}, !statusPanel)
const production = createProductionSource()
// One root for the session: the loading shell, a startup failure, Retry and the app
// render into it. Hot reload unmounts it, so the old tree never stays subscribed to
// a disposed source when this module runs again.
const root = createRoot(document.getElementById("root")!)
const cleanup: Array<() => void> = [() => settings.dispose(), () => production.dispose()]
let disposed = false
/** Stop `stop` on hot reload, or at once when it arrives after the module was disposed. */
function track(stop: () => void) { if (disposed) stop(); else cleanup.push(stop) }
if (import.meta.hot) import.meta.hot.dispose(() => {
  disposed = true
  root.unmount()
  for (const stop of cleanup.splice(0).reverse()) stop()
})

/** The window's code could not load: say so with a Retry that loads it again. */
async function showLoadFailure(error: unknown, retry: () => void) {
  const message = `Silo startup failed: ${error instanceof Error ? error.message : String(error)}. No computer state changed.`
  try {
    const { StartupFailure } = await loadFailure()
    if (!disposed) root.render(<StartupFailure message={message} retry={retry} />)
  } catch {
    if (!disposed) root.render(<p role="alert" className="p-6 text-sm">{message} Reopen Silo to try again.</p>)
  }
}

async function start() {
  if (!desktop) {
    try {
      const { Unavailable } = await loadSurface()
      if (!disposed) root.render(<Unavailable message="Open Silo in the desktop app." />)
    } catch (error) { await showLoadFailure(error, () => void start()) }
    return
  }

  const viewer = desktopViewerRoute()
  if (windowLabel.startsWith("desktop-") && viewer) {
    try {
      const { NativeLinuxDesktopViewer } = await loadViewer()
      if (!disposed) root.render(<NativeLinuxDesktopViewer {...viewer} />)
    } catch (error) { await showLoadFailure(error, () => void start()) }
    return
  }

  let surface = loadSurface()
  surface.catch(() => {})
  let rendered = false
  // Rust has already shown the window: paint before any native call. The status panel's
  // skeleton ships with its tree; the main window's shell is small enough to paint at once.
  function showLoading() {
    rendered = false
    if (!statusPanel) {
      root.render(<SiloWindow title="Silo" label="Silo"><span role="status" className="sr-only">Opening Silo…</span></SiloWindow>)
      return
    }
    void surface.then(({ StartupLoading }) => { if (!disposed && !rendered) root.render(<StartupLoading statusPanel />) }, () => {})
  }
  showLoading()
  const dependencies = !statusPanel ? createNativeDependencyStore() : null
  dependencies?.retry()
  if (dependencies) track(() => dependencies.dispose())
  // Independent of the reads below. Quit must reach the settings flush as early as possible.
  void connectSettingsLifecycle(settings, !statusPanel, () => production.drainSetup()).then(track)
  // Only feeds the loading skeleton; its failure leaves the list empty.
  void production.loadConfiguration()
  // Live state does not depend on the settings, so it loads while they do.
  void production.initialize().catch((error: unknown) => console.error("Silo live updates:", error))
  // Both windows list open sites in their computer menus, which come from network services.
  // The watch starts reading once live updates are running, however that start turns out.
  track(production.watchNetwork({ ambient: !statusPanel }))
  const systemIntegrations = createDesktopSystemIntegrationStore(settings)
  track(() => systemIntegrations.dispose())
  if (!statusPanel) {
    // Reports its own failures; the switches stay disabled until it has read the OS state.
    void systemIntegrations.initialize()
    track(connectSystemIntegrationLifecycle(systemIntegrations))
  }
  // Resolved defaults are local to each webview's store, not saved settings.
  // Both windows must discover them; the provider refreshes them on focus.
  const applicationService = createApplicationService(settings)
  const computerUse = statusPanel ? null : createComputerUseBridge(nativeComputerUseBackend)
  const preparation = statusPanel ? null : createPreparationStore(nativePreparationBackend)
  let started = false

  // The only steps a Retry repeats: settings must be ready before the app renders.
  async function boot() {
    const [{ ProductionSurface }, initialCatalog] = await Promise.all([surface, (async () => {
      if (!statusPanel) await invoke("initialize_settings")
      await settings.initialize()
      // Defaults resolve with the catalog; wait for it briefly so early actions carry the application path.
      return Promise.race([
        applicationService.read().catch((error: unknown) => { console.error("Silo applications:", error); return null }),
        new Promise<null>(resolve => setTimeout(resolve, 1000, null)),
      ])
    })()])
    if (disposed) return
    if (!started) {
      started = true
      track(initializeTheme(settings))
    }
    const content = <ProductionSurface source={production} dependencyStore={dependencies} statusPanel={statusPanel} />
    rendered = true
    root.render(
      <StrictMode>
        <SettingsProvider store={settings}>
          <ApplicationCatalogProvider service={applicationService} initialCatalog={initialCatalog ?? undefined}>
            {!initialCatalog && <ApplicationCatalogLoader />}
            {statusPanel || !computerUse || !preparation ? content : (
              <SystemIntegrationProvider store={systemIntegrations}>
                <ComputerUseProvider bridge={computerUse}>
                  <PreparationProvider store={preparation}>{content}</PreparationProvider>
                </ComputerUseProvider>
              </SystemIntegrationProvider>
            )}
          </ApplicationCatalogProvider>
        </SettingsProvider>
      </StrictMode>,
    )
  }

  function run() {
    void boot().catch(async (error: unknown) => {
      if (disposed) return
      const message = `Silo startup failed: ${errorMessage(error)}. No computer state changed.`
      const retry = () => { surface = loadSurface(); surface.catch(() => {}); showLoading(); run() }
      const loaded = await surface.catch(() => null)
      if (disposed) return
      if (!loaded) { await showLoadFailure(error, retry); return }
      const { Unavailable } = loaded
      if (!statusPanel) { root.render(<Unavailable message={message} retry={retry} retryLabel="Retry" />); return }
      const { StatusPanelUnavailable } = await import("@/desktop/application-loading")
      if (!disposed) root.render(<StatusPanelUnavailable message={message} retry={retry} />)
    })
  }
  run()
}

void start()
