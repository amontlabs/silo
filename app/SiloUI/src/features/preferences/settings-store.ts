import { errorMessage } from "@/lib/error-message"
import { createContext, createElement, useContext, useRef, useState, useSyncExternalStore, type ReactNode } from "react"
import { onboardingDraftSchema, type OnboardingDraft } from "@/features/onboarding/model/onboarding-draft"
import { defaultSettings, readSettingsOverrides, settingsPatchSchema, type Settings, type SettingsPatch } from "./model/settings"

export interface SettingsSnapshot {
  revision: number
  settings: Record<string, unknown>
  onboardingDraft: OnboardingDraft | null
  saveError: string | null
  /** The settings file is protected from writes; changes last for this session only. */
  writeProtected?: boolean
}

export interface SettingsBackend {
  read: () => Promise<SettingsSnapshot>
  subscribe: (receive: (snapshot: SettingsSnapshot) => void) => Promise<() => void>
  updateSettings: (patch: SettingsPatch) => Promise<SettingsSnapshot>
  updateOnboardingDraft: (draft: OnboardingDraft | null) => Promise<SettingsSnapshot>
  flush: () => Promise<void>
  /** Set a write-protected settings file aside and start from defaults. */
  resetProtected?: () => Promise<SettingsSnapshot>
}

type Change = { kind: "settings"; patch: SettingsPatch } | { kind: "draft"; draft: OnboardingDraft | null }
export interface SettingsView extends Omit<SettingsSnapshot, "settings"> { settings: Settings }

/**
 * The native store validated and refused the change (`settings.rs` update and
 * update_draft). Resending it can never succeed, unlike a failed delivery.
 */
function isRejection(error: unknown) { return /^Invalid (settings change|onboarding draft)$/.test(errorMessage(error)) }

export function createSettingsStore(backend: SettingsBackend, initialSettings: SettingsPatch = {}, initialSnapshot?: SettingsSnapshot) {
  const defaults = { ...defaultSettings, ...initialSettings }
  const listeners = new Set<() => void>()
  const pending: Change[] = []
  let confirmed: SettingsSnapshot = initialSnapshot ?? { revision: -1, settings: {}, onboardingDraft: null, saveError: null }
  let transportError: string | null = null
  /** The last rejected change, shown until the next change or flush. */
  let rejection: string | null = null
  function resolveSettings(overrides: SettingsPatch) {
    const settings = { ...defaults, ...overrides }
    for (const kind of ["terminal", "editor", "browser"] as const) {
      const path = `${kind}Path` as const
      const mode = `${kind}UseSystemDefault` as const
      settings[mode] = overrides[mode] ?? !(kind in overrides || path in overrides)
      if (settings[mode]) {
        settings[kind] = defaults[kind]
        settings[path] = defaults[path]
        continue
      }
      // A legacy saved label must not inherit a different installed default's path.
      if (kind in overrides && !(path in overrides)) settings[path] = null
    }
    return settings
  }
  let current: SettingsView = { ...confirmed, settings: resolveSettings(readSettingsOverrides(confirmed.settings)) }
  let draining: Promise<void> | null = null
  let activeChange: Change | null = null
  let initialization: Promise<void> | null = null
  let unsubscribe: (() => void) | undefined
  let disposed = false

  function publish() {
    const overrides = readSettingsOverrides(confirmed.settings)
    for (const change of pending) {
      if (change.kind === "settings") Object.assign(overrides, change.patch)
    }
    const next: SettingsView = {
      ...confirmed,
      settings: resolveSettings(overrides),
      saveError: transportError ?? rejection ?? confirmed.saveError,
      writeProtected: transportError === null && rejection === null && confirmed.writeProtected ? true : undefined,
    }
    for (const change of pending) {
      if (change.kind === "draft") next.onboardingDraft = change.draft
    }
    if (JSON.stringify(next) === JSON.stringify(current)) return
    current = next
    listeners.forEach((listener) => listener())
  }

  function receive(snapshot: SettingsSnapshot) {
    if (disposed || snapshot.revision < confirmed.revision) return
    confirmed = snapshot
    publish()
  }

  function failed(error: unknown) {
    transportError = errorMessage(error)
    console.error("Silo settings:", transportError)
    publish()
  }

  function drain(): Promise<void> {
    if (draining) return draining
    let writeFailed = false
    draining = (async () => {
      while (pending.length) {
        const change = pending[0]
        activeChange = change
        try {
          const snapshot = change.kind === "settings"
            ? await backend.updateSettings(change.patch)
            : await backend.updateOnboardingDraft(change.draft)
          pending.shift()
          transportError = null
          receive(snapshot)
          publish()
        } catch (error) {
          if (isRejection(error)) {
            // Roll back the refused change instead of resending it forever, which
            // would also block every later change behind it.
            pending.shift()
            rejection = errorMessage(error)
            console.error("Silo settings: change rejected:", rejection)
            publish()
            continue
          }
          writeFailed = true
          failed(error)
          // Keep an unsent edit visible and retry it on the next edit or flush.
          return
        } finally { activeChange = null }
      }
    })().finally(() => {
      draining = null
      // A subscriber can enqueue during the microtask between the loop finishing
      // and this cleanup. Hand that work on instead of leaving it stranded.
      if (!writeFailed && pending.length) return drain()
    })
    return draining
  }

  function enqueue(change: Change) {
    rejection = null
    const previous = pending.at(-1)
    // Drafts are complete snapshots; only the latest unsent one needs delivery.
    if (change.kind === "draft" && previous?.kind === "draft" && previous !== activeChange) pending[pending.length - 1] = change
    else pending.push(change)
    publish()
    return drain()
  }

  function initialize() {
    initialization ??= (async () => {
      let before = confirmed.revision
      try {
        if (!unsubscribe) {
          const stop = await backend.subscribe(receive)
          if (disposed) { stop(); return }
          unsubscribe = stop
        }
        before = confirmed.revision
        const snapshot = await backend.read()
        if (!pending.length) transportError = null
        receive(snapshot)
        publish()
      } catch (error) {
        if (!disposed && (!unsubscribe || confirmed.revision === before)) failed(error)
      }
    })().finally(() => { initialization = null })
    return initialization
  }

  return {
    getSnapshot: () => current,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener) } },
    updateDefaults(patch: SettingsPatch) {
      Object.assign(defaults, patch)
      publish()
    },
    initialize,
    refresh: initialize,
    async updateSettings(patch: SettingsPatch) {
      try {
        const value = settingsPatchSchema.parse(patch)
        if (Object.keys(value).length) await enqueue({ kind: "settings", patch: value })
      } catch (error) { failed(error) }
    },
    async updateOnboardingDraft(draft: OnboardingDraft | null) {
      try {
        const parsed = draft === null ? null : onboardingDraftSchema.parse(draft)
        if (new TextEncoder().encode(JSON.stringify(parsed)).length > 256 * 1024) throw new Error("Onboarding draft is too large to save")
        await enqueue({ kind: "draft", draft: parsed })
      }
      catch (error) { failed(error) }
    },
    async flush() {
      // An earlier rejected change is already rolled back; nothing of it remains
      // unsaved, so it must not fail this flush (for example, Quit).
      rejection = null
      publish()
      try {
        do {
          await drain()
          if (pending.length && transportError) return
          await backend.flush()
          const before = confirmed.revision
          let snapshot: SettingsSnapshot
          try { snapshot = await backend.read() }
          catch (error) {
            if (disposed) return
            if (confirmed.revision === before) throw error
            snapshot = confirmed
          }
          if (!pending.length) transportError = null
          receive(snapshot)
        } while (pending.length || draining)
      } catch (error) { failed(error) }
    },
    canResetProtected: backend.resetProtected !== undefined,
    async resetProtected() {
      if (!backend.resetProtected) return
      try {
        const snapshot = await backend.resetProtected()
        transportError = null
        rejection = null
        receive(snapshot)
        publish()
      } catch (error) { failed(error) }
    },
    dispose() { disposed = true; unsubscribe?.(); listeners.clear() },
  }
}

export type SettingsStore = ReturnType<typeof createSettingsStore>

// Each fixture/test owns its own backend. Nothing here reads browser or native storage.
export function createMemorySettingsStore(initialSettings: SettingsPatch = {}, initialDraft: OnboardingDraft | null = null): SettingsStore {
  let snapshot: SettingsSnapshot = { revision: 0, settings: structuredClone(initialSettings), onboardingDraft: initialDraft, saveError: null }
  const listeners = new Set<(snapshot: SettingsSnapshot) => void>()
  function update(change: Partial<SettingsSnapshot>) {
    snapshot = { ...snapshot, ...change, revision: snapshot.revision + 1 }
    listeners.forEach((listener) => listener(snapshot))
    return Promise.resolve(snapshot)
  }
  return createSettingsStore({
    read: async () => snapshot,
    subscribe: async (listener) => { listeners.add(listener); return () => { listeners.delete(listener) } },
    updateSettings: (patch) => update({ settings: { ...snapshot.settings, ...patch } }),
    updateOnboardingDraft: (draft) => update({ onboardingDraft: draft }),
    flush: async () => {},
  }, initialSettings, snapshot)
}

const SettingsContext = createContext<SettingsStore | null>(null)

export function SettingsProvider({ store, initialSettings, children }: { store?: SettingsStore; initialSettings?: SettingsPatch; children: ReactNode }) {
  const inherited = useContext(SettingsContext)
  const [local] = useState(() => {
    const value = createMemorySettingsStore()
    value.updateDefaults(initialSettings ?? {})
    return value
  })
  return createElement(SettingsContext.Provider, { value: store ?? inherited ?? local }, children)
}

export function useSettings(initialSettings?: SettingsPatch) {
  const store = useSettingsStore(initialSettings)
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot)
  return { ...snapshot, store, updateSettings: store.updateSettings, updateOnboardingDraft: store.updateOnboardingDraft, flush: store.flush, canResetProtected: store.canResetProtected, resetProtected: store.resetProtected }
}

/** The settings store in context, or a private in-memory one when no provider exists. */
export function useSettingsStore(initialSettings?: SettingsPatch) {
  const inherited = useContext(SettingsContext)
  const [local] = useState(() => {
    const value = createMemorySettingsStore()
    value.updateDefaults(initialSettings ?? {})
    return value
  })
  return inherited ?? local
}

/**
 * Subscribes to a derived value of the settings view. The component re-renders only when
 * the selection changes under `isEqual` (reference equality by default).
 */
export function useSettingsSelector<T>(select: (view: SettingsView) => T, isEqual: (a: T, b: T) => boolean = Object.is, initialSettings?: SettingsPatch): T {
  const store = useSettingsStore(initialSettings)
  const memo = useRef<{ view: SettingsView; select: typeof select; value: T } | null>(null)
  const getSelection = () => {
    const view = store.getSnapshot()
    const previous = memo.current
    if (previous && previous.view === view && previous.select === select) return previous.value
    const value = select(view)
    if (previous && isEqual(previous.value, value)) {
      memo.current = { view, select, value: previous.value }
      return previous.value
    }
    memo.current = { view, select, value }
    return value
  }
  return useSyncExternalStore(store.subscribe, getSelection)
}

export function shallowEqual<T extends Record<string, unknown>>(a: T, b: T) {
  const keys = Object.keys(a)
  return keys.length === Object.keys(b).length && keys.every((key) => Object.is(a[key], b[key]))
}
