import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { z } from "zod"
import { onboardingDraftSchema } from "@/features/onboarding/model/onboarding-draft"
import { createSettingsStore, type SettingsBackend, type SettingsStore } from "@/features/preferences/settings-store"
import type { SettingsPatch } from "@/features/preferences/model/settings"

const nativeSnapshotSchema = z.object({
  revision: z.number().int().nonnegative(),
  settings: z.record(z.string(), z.unknown()),
  onboardingDraft: z.unknown().transform((draft) => {
    if (draft === null) return null
    const parsed = onboardingDraftSchema.safeParse(draft)
    if (parsed.success) return parsed.data
    console.error("Silo settings: saved onboarding draft could not be restored")
    return null
  }),
  saveError: z.string().nullable(),
  writeProtected: z.boolean().optional(),
})

export function createDesktopSettingsStore(initialSettings: SettingsPatch, main: boolean) {
  let firstRead = true
  const backend: SettingsBackend = {
    async read() {
      let snapshot = nativeSnapshotSchema.parse(await invoke("read_settings"))
      if (firstRead && main) {
        // Import an actual saved choice only. Retain its old key if migration fails.
        let theme: string | null = null
        try { theme = localStorage.getItem("silo-theme") } catch { /* Browser storage can be unavailable. */ }
        if (!snapshot.saveError && !("theme" in snapshot.settings) && (theme === "system" || theme === "dark" || theme === "light")) {
          snapshot = nativeSnapshotSchema.parse(await invoke("import_legacy_theme", { theme }))
        }
        firstRead = false
      }
      return snapshot
    },
    subscribe: (receive) => listen("settings:changed", ({ payload }) => {
      const parsed = nativeSnapshotSchema.safeParse(payload)
      if (!parsed.success) {
        console.error("Silo settings: invalid native event", parsed.error.message)
        return
      }
      if (!main) {
        receive(parsed.data)
        return
      }
      // Events are public; the command returns the main window's recovery draft.
      void invoke("read_settings")
        .then(snapshot => receive(nativeSnapshotSchema.parse(snapshot)))
        .catch(error => console.error("Silo settings: native refresh failed", error))
    }),
    updateSettings: async (patch) => nativeSnapshotSchema.parse(await invoke("update_settings", { patch })),
    updateOnboardingDraft: async (draft) => nativeSnapshotSchema.parse(await invoke("update_onboarding_draft", { draft })),
    flush: () => invoke("flush_settings"),
    resetProtected: async () => nativeSnapshotSchema.parse(await invoke("reset_protected_settings")),
  }
  return createSettingsStore(backend, initialSettings)
}

/**
 * How long Quit waits for setup work (onboarding computer creation, GitHub
 * verification) before saving settings. Silo's native shutdown then waits for
 * any computer operation that is still running, and the Quit overlay names it
 * and offers to cancel it, instead of an unexplained "Stopping local computers…".
 */
export const SETUP_DRAIN_LIMIT_MS = 5_000

function withinLimit(work: () => Promise<void>, limitMs: number) {
  let timer: ReturnType<typeof setTimeout> | undefined
  const limit = new Promise<void>((resolve) => {
    timer = setTimeout(() => {
      console.warn("Silo settings shutdown: setup work is still running; saving settings without waiting for it.")
      resolve()
    }, limitMs)
  })
  return Promise.race([work(), limit]).finally(() => clearTimeout(timer))
}

export async function connectSettingsLifecycle(store: SettingsStore, main: boolean, beforeFlush: () => Promise<void> = async () => {}, beforeFlushLimitMs = SETUP_DRAIN_LIMIT_MS) {
  let stop: (() => void) | undefined
  let connecting: Promise<void> | null = null
  let disposed = false
  async function flushForQuit() {
    try { await invoke("begin_settings_flush") }
    catch (error) {
      console.error("Silo settings shutdown acknowledgment:", error)
      return
    }
    try {
      await withinLimit(beforeFlush, beforeFlushLimitMs)
      await store.flush()
      // A write-protected settings file must not block Quit: the file is
      // intentionally left unchanged and this session's changes are dropped.
      const { saveError, writeProtected } = store.getSnapshot()
      if (saveError && !writeProtected) throw new Error(saveError)
      await invoke("complete_settings_flush")
    } catch (error) {
      console.error("Silo settings shutdown:", error)
      await invoke("cancel_settings_flush").catch((failure: unknown) => console.error("Silo could not cancel shutdown:", failure))
    }
  }
  function connect() {
    if (stop || disposed) return Promise.resolve()
    connecting ??= (async () => {
      try {
        const unsubscribe = main
          ? await listen("settings:flush-request", () => { void flushForQuit() })
          : await listen("desktop:status-opened", () => { void store.refresh() })
        if (disposed) unsubscribe()
        else stop = unsubscribe
      } catch (error) { console.error("Silo settings lifecycle:", error) }
    })().finally(() => { connecting = null })
    return connecting
  }
  const refresh = () => { void store.refresh(); void connect() }
  window.addEventListener("focus", refresh)
  await connect()
  return () => { disposed = true; stop?.(); window.removeEventListener("focus", refresh) }
}

const quitRequestSchema = z.object({
  requestId: z.number().int().nonnegative(),
  /** Running local computer names; empty when their status could not be read. */
  computers: z.array(z.string()),
})
export type QuitRequest = z.infer<typeof quitRequestSchema>

/**
 * Opt the main window in to confirming Quit while local computers run (decision 7).
 * `ask` resolves true for "Quit and stop" and false for "Cancel". A repeated Quit
 * while the prompt is open re-sends the same request and is ignored here.
 */
export async function connectQuitConfirmation(ask: (request: QuitRequest) => Promise<boolean>) {
  let active: number | null = null
  const stop = await listen("silo://quit-requested", (event) => {
    const parsed = quitRequestSchema.safeParse(event.payload)
    if (!parsed.success) { console.error("Silo quit request was invalid:", parsed.error); return }
    const { requestId } = parsed.data
    if (active === requestId) return
    active = requestId
    void ask(parsed.data)
      .catch((error: unknown) => { console.error("Silo quit confirmation:", error); return false })
      .then((confirmed) => invoke("answer_quit_request", { requestId, confirmed }))
      .catch((error: unknown) => console.error("Silo quit answer:", error))
      .finally(() => { if (active === requestId) active = null })
  })
  try { await invoke("enable_quit_confirmation") }
  catch (error) { stop(); throw error }
  return stop
}
