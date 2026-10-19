import { errorMessage } from "@/lib/error-message"
import { createContext, createElement, useContext, useSyncExternalStore, type ReactNode } from "react"
import { z } from "zod"

import type { SettingsStore } from "./settings-store"

const loginStateSchema = z.enum(["enabled", "notRegistered", "requiresApproval", "notFound", "unavailable", "error"])
const notificationStateSchema = z.enum(["notDetermined", "denied", "authorized", "provisional", "unavailable", "error"])
const status = <T extends z.ZodType>(state: T) => z.object({ state, error: z.string().nullable() })

export const systemIntegrationsSchema = z.object({
  platform: z.enum(["macos", "linux"]),
  loginItem: status(loginStateSchema),
  notifications: status(notificationStateSchema),
})

export type SystemIntegrations = z.infer<typeof systemIntegrationsSchema>
export type LoginItemStatus = SystemIntegrations["loginItem"]
export type NotificationStatus = SystemIntegrations["notifications"]

export interface SystemIntegrationService {
  read: () => Promise<SystemIntegrations>
  setLoginItem: (enabled: boolean) => Promise<SystemIntegrations["loginItem"]>
  requestNotifications: () => Promise<SystemIntegrations["notifications"]>
  openSettings: (integration: "loginItem" | "notifications") => Promise<void>
  showError: (message: string) => Promise<void>
}

export interface SystemIntegrationView extends SystemIntegrations {
  initialized: boolean
  loginPending: boolean
  notificationsPending: boolean
}

const unknown: SystemIntegrations = {
  platform: "macos",
  loginItem: { state: "notRegistered", error: null },
  notifications: { state: "notDetermined", error: null },
}

export function createSystemIntegrationStore(
  service: SystemIntegrationService,
  settings: SettingsStore,
  initial?: SystemIntegrations,
) {
  let current: SystemIntegrationView = {
    ...(initial ?? unknown),
    initialized: initial !== undefined,
    loginPending: false,
    notificationsPending: false,
  }
  const listeners = new Set<() => void>()
  let initialization: Promise<void> | null = null
  let refreshSequence = 0

  function publish(changes: Partial<SystemIntegrationView>) {
    current = { ...current, ...changes }
    listeners.forEach((listener) => listener())
  }

  async function report(error: unknown) {
    const text = errorMessage(error)
    console.error("Silo system integrations:", text)
    try { await service.showError(text) }
    catch (dialogError) { console.error("Silo system integration dialog:", dialogError) }
  }

  async function saveVerified(patch: { launchAtLogin: boolean } | { notificationsEnabled: boolean }) {
    await settings.updateSettings(patch)
    const saveError = settings.getSnapshot().saveError
    if (saveError) await report(saveError)
  }

  async function refresh(reportInitialFailure = false) {
    const sequence = ++refreshSequence
    try {
      const snapshot = systemIntegrationsSchema.parse(await service.read())
      if (sequence !== refreshSequence) return
      publish({ ...snapshot, initialized: true })
      if (reportInitialFailure) {
        const errors = [snapshot.loginItem.error, snapshot.notifications.error].filter(Boolean)
        if (errors.length) await report(errors.join("\n"))
      }
    } catch (error) {
      if (sequence !== refreshSequence) return
      // A transport or schema failure does not establish either authority.
      // Keep both switches disabled until a later refresh supplies a snapshot.
      publish({ initialized: false })
      if (reportInitialFailure) await report(error)
      else console.error("Silo system integrations:", errorMessage(error))
    }
  }

  function initialize() {
    initialization ??= refresh(true).finally(() => { initialization = null })
    return initialization
  }

  async function setLaunchAtLogin(enabled: boolean) {
    if (current.loginPending) return
    // Invalidate a focus read that started before this explicit mutation.
    ++refreshSequence
    publish({ loginPending: true })
    try {
      const verified = status(loginStateSchema).parse(await service.setLoginItem(enabled))
      // Invalidate any focus read that started while the OS action was pending.
      ++refreshSequence
      publish({ loginItem: verified })
      await saveVerified({ launchAtLogin: verified.state === "enabled" })
      if (enabled && !["enabled", "requiresApproval"].includes(verified.state)) {
        await report(verified.error ?? "The operating system did not enable Silo at login")
      }
    } catch (error) {
      await refresh(false)
      await report(error)
    } finally {
      publish({ loginPending: false })
    }
  }

  async function setNotificationsEnabled(enabled: boolean) {
    if (current.notificationsPending) return
    ++refreshSequence
    publish({ notificationsPending: true })
    try {
      if (!enabled) {
        await saveVerified({ notificationsEnabled: false })
        return
      }
      // Authorization can change outside Silo. Every explicit enable starts
      // from a fresh native read instead of trusting the rendered snapshot.
      let fresh: SystemIntegrations
      for (;;) {
        const sequence = ++refreshSequence
        try {
          fresh = systemIntegrationsSchema.parse(await service.read())
          if (sequence === refreshSequence) break
          // A focus read or another OS action superseded this preflight. Read
          // again so the explicit enable uses current permission authority.
        } catch (error) {
          if (sequence !== refreshSequence) continue
          // A current failed preflight cannot establish authorization.
          publish({ initialized: false })
          await report(error)
          return
        }
      }
      let verified = fresh.notifications
      ++refreshSequence
      publish({ ...fresh, initialized: true })
      if (verified.state === "notDetermined") {
        verified = status(notificationStateSchema).parse(await service.requestNotifications())
        ++refreshSequence
        publish({ notifications: verified })
      }
      const authorized = verified.state === "authorized" || verified.state === "provisional"
      await saveVerified({ notificationsEnabled: authorized })
      if (!authorized && !["denied"].includes(verified.state)) {
        await report(verified.error ?? "Desktop notifications are unavailable")
      }
    } catch (error) {
      await refresh(false)
      await report(error)
    } finally {
      publish({ notificationsPending: false })
    }
  }

  async function openSettings(integration: "loginItem" | "notifications") {
    try { await service.openSettings(integration) }
    catch (error) { await report(error) }
  }

  return {
    getSnapshot: () => current,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener) } },
    initialize,
    refresh: () => refresh(false),
    setLaunchAtLogin,
    setNotificationsEnabled,
    openSettings,
    dispose() { listeners.clear() },
  }
}

export type SystemIntegrationStore = ReturnType<typeof createSystemIntegrationStore>

const SystemIntegrationContext = createContext<SystemIntegrationStore | null>(null)

export function SystemIntegrationProvider({ store, children }: { store: SystemIntegrationStore; children: ReactNode }) {
  return createElement(SystemIntegrationContext.Provider, { value: store }, children)
}

export function useSystemIntegrations() {
  const inherited = useContext(SystemIntegrationContext)
  if (!inherited) throw new Error("SystemIntegrationProvider is required")
  const store = inherited
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot)
  return {
    ...snapshot,
    loginEnabled: snapshot.loginItem.state === "enabled",
    notificationsAuthorized: snapshot.notifications.state === "authorized" || snapshot.notifications.state === "provisional",
    setLaunchAtLogin: store.setLaunchAtLogin,
    setNotificationsEnabled: store.setNotificationsEnabled,
    openIntegrationSettings: store.openSettings,
  }
}
