import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { invoke, isTauri } from "@tauri-apps/api/core"
import { getCurrentWindow } from "@tauri-apps/api/window"

import "./index.css"
import { createComputerUseBridge, nativeComputerUseBackend } from "@/desktop/computer-use-bridge"
import { createPreparationStore, nativePreparationBackend, PreparationProvider } from "@/desktop/preparation"
import { ComputerUseProvider } from "@/desktop/computer-use-provider"
import { createApplicationService, emptyApplicationCatalog } from "@/desktop/applications"
import { StatusPanelUnavailable } from "@/desktop/application-loading"
import { createNativeDependencyStore } from "@/desktop/dependencies"
import { desktopViewerRoute } from "@/desktop/linux-desktop-state"
import { NativeLinuxDesktopViewer } from "@/desktop/linux-desktop-viewer"
import { macosDisplayRoute } from "@/desktop/macos-remote-viewer-state"
import { NativeMacosRemoteViewer } from "@/desktop/macos-remote-viewer"
import { ProductionSurface, StartupLoading, Unavailable } from "@/desktop/production-surface"
import { createProductionSource } from "@/desktop/production-source"
import { createDesktopSettingsStore, connectSettingsLifecycle } from "@/desktop/settings"
import { connectSystemIntegrationLifecycle, createDesktopSystemIntegrationStore } from "@/desktop/system-integrations"
import { ApplicationCatalogProvider } from "@/features/preferences/application-catalog"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { initializeTheme } from "@/features/preferences/theme"

const desktop = isTauri()
const windowLabel = desktop ? getCurrentWindow().label : ""
const statusPanel = windowLabel === "status"
document.documentElement.classList.toggle("native-status", statusPanel)
document.documentElement.classList.toggle("native-material", windowLabel === "main" && /Mac/.test(navigator.platform))
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

function start() {
  if (!desktop) {
    root.render(<Unavailable message="Open Silo in the desktop app." />)
    return
  }

  const macosDisplay = windowLabel.startsWith("desktop-shell-") ? macosDisplayRoute() : null
  if (macosDisplay) {
    root.render(<NativeMacosRemoteViewer name={macosDisplay.name} device={macosDisplay.device} />)
    return
  }

  const viewer = desktopViewerRoute()
  if (windowLabel.startsWith("desktop-") && viewer) {
    root.render(<NativeLinuxDesktopViewer {...viewer} />)
    return
  }

  // Rust has already shown the window: paint before any native call.
  root.render(<StartupLoading statusPanel={statusPanel} />)
  const dependencies = !statusPanel ? createNativeDependencyStore() : null
  dependencies?.retry()
  if (dependencies) track(() => dependencies.dispose())
  // Independent of the reads below. Quit must reach the settings flush as early as possible.
  void connectSettingsLifecycle(settings, !statusPanel, () => production.drainSetup()).then(track)
  // Only feeds the loading skeleton; its failure leaves the list empty.
  void production.loadConfiguration()
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
  const computerUse = createComputerUseBridge(nativeComputerUseBackend)
  const preparation = createPreparationStore(nativePreparationBackend)
  let started = false

  // The only steps a Retry repeats: settings must be ready before the app renders.
  async function boot() {
    if (!statusPanel) await invoke("initialize_settings")
    await settings.initialize()
    const applicationCatalog = await applicationService.read().catch((error: unknown) => {
      console.error("Silo applications:", error)
      return emptyApplicationCatalog
    })
    if (disposed) return
    if (!started) {
      started = true
      track(initializeTheme(settings))
      void production.initialize().catch((error: unknown) => console.error("Silo live updates:", error))
    }
    root.render(
      <StrictMode>
        <SettingsProvider store={settings}>
          <SystemIntegrationProvider store={systemIntegrations}>
            <ApplicationCatalogProvider initialCatalog={applicationCatalog} service={applicationService}>
              <ComputerUseProvider bridge={computerUse}>
                <PreparationProvider store={preparation}>
                  <ProductionSurface source={production} dependencyStore={dependencies} statusPanel={statusPanel} />
                </PreparationProvider>
              </ComputerUseProvider>
            </ApplicationCatalogProvider>
          </SystemIntegrationProvider>
        </SettingsProvider>
      </StrictMode>,
    )
  }

  function run() {
    void boot().catch((error: unknown) => {
      if (disposed) return
      const message = `Silo startup failed: ${error instanceof Error ? error.message : String(error)}. No computer state changed.`
      const retry = () => { root.render(<StartupLoading statusPanel={statusPanel} />); run() }
      root.render(statusPanel ? <StatusPanelUnavailable message={message} retry={retry} /> : <Unavailable message={message} retry={retry} retryLabel="Retry" />)
    })
  }
  run()
}

start()
