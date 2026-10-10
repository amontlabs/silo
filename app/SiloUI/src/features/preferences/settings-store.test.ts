import { describe, expect, it, vi } from "vitest"
import { createMemorySettingsStore, createSettingsStore, type SettingsBackend, type SettingsSnapshot } from "./settings-store"

const snapshot = (revision = 0, settings = {}): SettingsSnapshot => ({ revision, settings, onboardingDraft: null, saveError: null })

describe("settings synchronization", () => {
  it("clears write protection after the backend resets the settings file", async () => {
    const protectedSnapshot: SettingsSnapshot = { ...snapshot(1), saveError: "Settings use an unsupported file version.", writeProtected: true }
    const resetProtected = vi.fn().mockResolvedValue(snapshot(2))
    const store = createSettingsStore({
      subscribe: async () => () => {}, read: async () => protectedSnapshot, flush: async () => {},
      updateSettings: async () => snapshot(), updateOnboardingDraft: async () => snapshot(), resetProtected,
    })
    try {
      await store.initialize()
      expect(store.getSnapshot()).toMatchObject({ writeProtected: true })
      await store.resetProtected()
      expect(resetProtected).toHaveBeenCalledOnce()
      expect(store.getSnapshot()).toMatchObject({ revision: 2, saveError: null })
      expect(store.getSnapshot().writeProtected).toBeUndefined()
    } finally { store.dispose() }
  })

  it.each(["read", "flush"] as const)("handles a late %s failure after a newer settings event during flush", async phase => {
    let receive!: (value: SettingsSnapshot) => void
    let reject!: (error: Error) => void
    const late = new Promise<SettingsSnapshot>((_, fail) => { reject = fail })
    const read = vi.fn().mockResolvedValueOnce(snapshot()).mockImplementation(() => late)
    const flush = vi.fn().mockImplementation(async () => {
      if (phase === "flush") { receive(snapshot(2, { theme: "dark" })); throw new Error("Flush unavailable") }
    })
    const store = createSettingsStore({
      subscribe: async listener => { receive = listener; return () => {} }, read, flush,
      updateSettings: async () => snapshot(), updateOnboardingDraft: async () => snapshot(),
    })
    try {
      await store.initialize()
      const pending = store.flush()
      if (phase === "read") {
        await vi.waitFor(() => expect(read).toHaveBeenCalledTimes(2))
        receive(snapshot(2, { theme: "dark" }))
        reject(new Error("Old read unavailable"))
      }
      await pending
      expect(store.getSnapshot()).toMatchObject({ revision: 2, settings: { theme: "dark" }, saveError: phase === "read" ? null : "Flush unavailable" })
    } finally { store.dispose() }
  })

  it("delivers only the latest queued draft while preserving an in-flight write", async () => {
    const draft = { currentStep: "github" as const, computers: [], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} }
    let state = snapshot()
    let acknowledge!: () => void
    const writes: SettingsSnapshot["onboardingDraft"][] = []
    const store = createSettingsStore({
      subscribe: async () => () => {}, read: async () => state,
      updateSettings: async () => state, flush: async () => {},
      updateOnboardingDraft: async value => {
        writes.push(value)
        if (writes.length === 1) await new Promise<void>(resolve => { acknowledge = resolve })
        state = { ...state, revision: state.revision + 1, onboardingDraft: value }
        return state
      },
    })
    try {
      await store.initialize()
      const first = store.updateOnboardingDraft(draft)
      const second = store.updateOnboardingDraft({ ...draft, currentStep: "computers" })
      const third = store.updateOnboardingDraft({ ...draft, currentStep: "review" })
      const cleared = store.updateOnboardingDraft(null)
      expect(store.getSnapshot().onboardingDraft).toBeNull()
      acknowledge()
      await Promise.all([first, second, third, cleared])
      expect(writes).toEqual([draft, null])
      expect(state.onboardingDraft).toBeNull()
    } finally { store.dispose() }
  })

  it("does not replay a failed draft after setup has cleared recovery data", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {})
    let state = snapshot()
    let failing = true
    const delivered: SettingsSnapshot["onboardingDraft"][] = []
    const store = createSettingsStore({
      subscribe: async () => () => {}, read: async () => state,
      updateSettings: async () => state, flush: async () => {},
      updateOnboardingDraft: async value => {
        if (failing) throw new Error("Draft delivery unavailable")
        delivered.push(value)
        state = { ...state, revision: state.revision + 1, onboardingDraft: value }
        return state
      },
    })
    try {
      await store.initialize()
      await store.updateOnboardingDraft({ currentStep: "review", computers: [], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} })
      await store.updateOnboardingDraft(null)
      failing = false
      await store.flush()
      expect(delivered).toEqual([null])
      expect(store.getSnapshot()).toMatchObject({ onboardingDraft: null, saveError: null })
    } finally { store.dispose(); error.mockRestore() }
  })

  it.each(["initial", "refresh"])("ignores a late %s read failure after a newer settings event", async phase => {
    let receive!: (value: SettingsSnapshot) => void
    let reject!: (error: Error) => void
    const read = vi.fn().mockImplementation(() => new Promise<SettingsSnapshot>((_, fail) => { reject = fail }))
    if (phase === "refresh") read.mockResolvedValueOnce(snapshot())
    const store = createSettingsStore({
      subscribe: async listener => { receive = listener; return () => {} }, read,
      updateSettings: async () => snapshot(), updateOnboardingDraft: async () => snapshot(), flush: async () => {},
    })
    try {
      if (phase === "refresh") await store.initialize()
      const pending = store.initialize()
      await vi.waitFor(() => expect(read).toHaveBeenCalledTimes(phase === "refresh" ? 2 : 1))
      receive(snapshot(2, { theme: "dark" }))
      reject(new Error("Old settings read unavailable"))
      await pending
      expect(store.getSnapshot()).toMatchObject({ revision: 2, settings: { theme: "dark" }, saveError: null })
    } finally { store.dispose() }
  })

  it("follows system application defaults after opting in without erasing the saved custom choice", async () => {
    let state = snapshot(0, { editor: "Custom", editorPath: "/Applications/Custom.app" })
    const store = createSettingsStore({
      subscribe: async () => () => {}, read: async () => state,
      updateSettings: async (patch) => { state = snapshot(state.revision + 1, { ...state.settings, ...patch }); return state },
      updateOnboardingDraft: async () => state, flush: async () => {},
    })
    await store.initialize()
    store.updateDefaults({ editor: "Zed", editorPath: "/Applications/Zed.app" })
    expect(store.getSnapshot().settings).toMatchObject({ editor: "Custom", editorUseSystemDefault: false })
    await store.updateSettings({ editorUseSystemDefault: true })
    expect(store.getSnapshot().settings).toMatchObject({ editor: "Zed", editorPath: "/Applications/Zed.app", editorUseSystemDefault: true })
    store.updateDefaults({ editor: "Cursor", editorPath: "/Applications/Cursor.app" })
    expect(store.getSnapshot().settings.editor).toBe("Cursor")
    expect(state.settings).toEqual({ editor: "Custom", editorPath: "/Applications/Custom.app", editorUseSystemDefault: true })
    await store.updateSettings({ editorUseSystemDefault: false })
    expect(store.getSnapshot().settings).toMatchObject({ editor: "Custom", editorPath: "/Applications/Custom.app" })
  })

  it("keeps explicit false and empty selections across a new store session", async () => {
    const first = createMemorySettingsStore()
    await first.updateSettings({ launchAtLogin: false, startupComputerIds: [], notifyChanges: false, editor: "Cursor" })
    const second = createMemorySettingsStore(first.getSnapshot().settings)
    expect(second.getSnapshot().settings).toMatchObject({ launchAtLogin: false, startupComputerIds: [], notifyChanges: false, editor: "Cursor" })
  })

  it("subscribes before reading and ignores an older initial read", async () => {
    let listener: (value: SettingsSnapshot) => void = () => {}
    const backend: SettingsBackend = {
      subscribe: async (receive) => { listener = receive; return () => {} },
      read: async () => { listener(snapshot(2, { theme: "dark" })); return snapshot(1, { theme: "light" }) },
      updateSettings: async () => snapshot(), updateOnboardingDraft: async () => snapshot(), flush: async () => {},
    }
    const store = createSettingsStore(backend)
    await store.initialize()
    expect(store.getSnapshot().settings.theme).toBe("dark")
  })

  it("joins an in-flight initialization and retries a failed read without subscribing twice", async () => {
    let reject!: (error: Error) => void
    const backend: SettingsBackend = {
      subscribe: vi.fn(async () => () => {}),
      read: vi.fn().mockImplementationOnce(() => new Promise((_, fail) => { reject = fail }))
        .mockResolvedValueOnce(snapshot(1, { theme: "dark" })),
      updateSettings: async () => snapshot(), updateOnboardingDraft: async () => snapshot(), flush: async () => {},
    }
    const store = createSettingsStore(backend)
    try {
      const initial = store.initialize()
      expect(store.initialize()).toBe(initial)
      await Promise.resolve()
      expect(backend.read).toHaveBeenCalledOnce()
      reject(new Error("Read unavailable"))
      await initial
      expect(store.getSnapshot().saveError).toBe("Read unavailable")

      await store.refresh()

      expect(backend.subscribe).toHaveBeenCalledOnce()
      expect(backend.read).toHaveBeenCalledTimes(2)
      expect(store.getSnapshot()).toMatchObject({ revision: 1, settings: { theme: "dark" }, saveError: null })
    } finally { store.dispose() }
  })

  it("keeps an optimistic edit over an event and ignores its acknowledgement if the event is newer", async () => {
    let receive!: (value: SettingsSnapshot) => void
    let acknowledge!: (value: SettingsSnapshot) => void
    const store = createSettingsStore({
      subscribe: async (listener) => { receive = listener; return () => {} }, read: async () => snapshot(),
      updateSettings: () => new Promise(resolve => { acknowledge = resolve }),
      updateOnboardingDraft: async () => snapshot(), flush: async () => {},
    })
    try {
      await store.initialize()
      const writing = store.updateSettings({ terminal: "iTerm" })
      receive(snapshot(2, { terminal: "Warp", theme: "dark" }))
      expect(store.getSnapshot().settings).toMatchObject({ terminal: "iTerm", theme: "dark" })

      acknowledge(snapshot(1, { terminal: "iTerm" }))
      await writing

      expect(store.getSnapshot()).toMatchObject({ revision: 2, settings: { terminal: "Warp", theme: "dark" } })
    } finally { store.dispose() }
  })

  it("drains a change enqueued by a subscriber when the first write is acknowledged", async () => {
    let state = snapshot()
    let acknowledge!: () => void
    const writes: unknown[] = []
    const store = createSettingsStore({
      subscribe: async () => () => {}, read: async () => state,
      updateSettings: async (patch) => {
        writes.push(patch)
        if (writes.length === 1) await new Promise<void>(resolve => { acknowledge = resolve })
        state = snapshot(state.revision + 1, { ...state.settings, ...patch })
        return state
      },
      updateOnboardingDraft: async () => state, flush: async () => {},
    })
    try {
      await store.initialize()
      let later: Promise<void> | undefined
      let enqueued = false
      store.subscribe(() => {
        if (store.getSnapshot().revision === 1 && !enqueued) {
          enqueued = true
          later = store.updateSettings({ browser: "Firefox" })
        }
      })
      const first = store.updateSettings({ theme: "dark" })
      acknowledge()
      await first
      await later
      expect(writes).toEqual([{ theme: "dark" }, { browser: "Firefox" }])
      expect(state.settings).toEqual({ theme: "dark", browser: "Firefox" })
      expect(store.getSnapshot().revision).toBe(2)
    } finally { store.dispose() }
  })

  it("unsubscribes a registration that completes after disposal without starting a read", async () => {
    let register!: (stop: () => void) => void
    const stop = vi.fn()
    const backend: SettingsBackend = {
      subscribe: () => new Promise(resolve => { register = resolve }), read: vi.fn(async () => snapshot()),
      updateSettings: async () => snapshot(), updateOnboardingDraft: async () => snapshot(), flush: async () => {},
    }
    const store = createSettingsStore(backend)
    const listener = vi.fn()
    store.subscribe(listener)
    const initial = store.initialize()
    store.dispose()
    register(stop)
    await initial
    expect(stop).toHaveBeenCalledOnce()
    expect(backend.read).not.toHaveBeenCalled()
    expect(listener).not.toHaveBeenCalled()
  })

  it("keeps rapid changes visible and writes patches in order", async () => {
    let release!: () => void
    let state = snapshot()
    const patches: unknown[] = []
    const backend: SettingsBackend = {
      subscribe: async () => () => {}, read: async () => state,
      updateSettings: async (patch) => {
        patches.push(patch)
        if (patches.length === 1) await new Promise<void>((resolve) => { release = resolve })
        state = snapshot(state.revision + 1, { ...state.settings, ...patch })
        return state
      },
      updateOnboardingDraft: async () => state, flush: async () => {},
    }
    const store = createSettingsStore(backend)
    await store.initialize()
    const first = store.updateSettings({ terminal: "iTerm" })
    const second = store.updateSettings({ browser: "Firefox" })
    expect(store.getSnapshot().settings).toMatchObject({ terminal: "iTerm", browser: "Firefox" })
    release()
    await Promise.all([first, second])
    expect(patches).toEqual([{ terminal: "iTerm" }, { browser: "Firefox" }])
    expect(store.getSnapshot().settings).toMatchObject({ terminal: "iTerm", browser: "Firefox" })
  })

  it("keeps a pending explicit empty selection when session defaults change", async () => {
    let release!: () => void
    let state = snapshot()
    let writes = 0
    const store = createSettingsStore({
      subscribe: async () => () => {}, read: async () => state,
      updateSettings: async (patch) => {
        writes += 1
        await new Promise<void>((resolve) => { release = resolve })
        state = snapshot(1, patch)
        return state
      },
      updateOnboardingDraft: async () => state, flush: async () => {},
    }, { startupComputerIds: ["initial-dev"] })
    await store.initialize()
    const pending = store.updateSettings({ startupComputerIds: [] })
    store.updateDefaults({ startupComputerIds: ["new-dev"] })
    expect(store.getSnapshot().settings.startupComputerIds).toEqual([])
    release()
    await pending
    store.updateDefaults({ startupComputerIds: ["another-dev"] })
    expect(store.getSnapshot().settings.startupComputerIds).toEqual([])
    expect(writes).toBe(1)
  })

  it("retries an unsent change during flush without losing the session selection", async () => {
    let attempts = 0
    const backend: SettingsBackend = {
      subscribe: async () => () => {}, read: async () => snapshot(),
      updateSettings: async (patch) => { if (++attempts === 1) throw new Error("IPC unavailable"); return snapshot(1, patch) },
      updateOnboardingDraft: async () => snapshot(), flush: async () => {},
    }
    const store = createSettingsStore(backend)
    await store.initialize()
    await store.updateSettings({ theme: "dark" })
    expect(store.getSnapshot().settings.theme).toBe("dark")
    expect(store.getSnapshot().saveError).toBe("IPC unavailable")
    await store.flush()
    expect(store.getSnapshot().saveError).toBeNull()
    expect(attempts).toBe(2)
  })

  it("rolls back a change the backend rejects so later changes and flushes still save", async () => {
    let state = snapshot()
    const writes: unknown[] = []
    let release!: () => void
    const store = createSettingsStore({
      subscribe: async () => () => {}, read: async () => state,
      updateSettings: async (patch) => {
        writes.push(patch)
        if ("terminal" in patch) {
          await new Promise<void>((resolve) => { release = resolve })
          // Native commands reject with the command's error string.
          throw "Invalid settings change"
        }
        state = snapshot(state.revision + 1, { ...state.settings, ...patch })
        return state
      },
      updateOnboardingDraft: async () => state, flush: async () => {},
    })
    await store.initialize()
    const saved = store.getSnapshot().settings.terminal
    const rejected = store.updateSettings({ terminal: "iTerm" })
    const later = store.updateSettings({ browser: "Firefox" })
    expect(store.getSnapshot().settings).toMatchObject({ terminal: "iTerm", browser: "Firefox" })
    release()
    await Promise.all([rejected, later])
    expect(writes).toEqual([{ terminal: "iTerm" }, { browser: "Firefox" }])
    expect(store.getSnapshot().settings.terminal).toBe(saved)
    expect(store.getSnapshot().settings.browser).toBe("Firefox")
    expect(store.getSnapshot().saveError).toBe("Invalid settings change")
    await store.flush()
    expect(store.getSnapshot().saveError).toBeNull()
    await store.updateSettings({ theme: "dark" })
    expect(state.settings).toEqual({ browser: "Firefox", theme: "dark" })
    expect(writes).toHaveLength(3)
  })
})
