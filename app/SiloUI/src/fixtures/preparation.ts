import type { PreparationBackend, PreparationStatus, PreparationTask } from "@/desktop/preparation"

// Deterministic fixtures for the background preparation toast. Select with `?preparation=<name>`
// in the browser preview; nothing here reaches Silo services.
export const preparationFixtureNames = ["downloading-image", "importing", "failed", "ready"] as const
export type PreparationFixtureName = typeof preparationFixtureNames[number]

export function preparationFixtureFromSearch(search: string): PreparationFixtureName | undefined {
  const requested = new URLSearchParams(search).get("preparation")
  return preparationFixtureNames.find(name => name === requested)
}

const task = (state: PreparationTask["state"], extra: Partial<PreparationTask> = {}): PreparationTask => ({ state, fraction: null, message: null, retryable: false, ...extra })

export function fixturePreparationStatus(name: PreparationFixtureName): PreparationStatus {
  switch (name) {
    case "downloading-image": return { image: task("running", { fraction: 42 }) }
    case "importing": return { image: task("running") }
    case "failed": return { image: task("failed", { message: "Silo could not download the computer image. Check your network connection, then retry.", retryable: true }) }
    case "ready": return { image: task("ready") }
  }
}

/** Retry in a failed fixture finishes the work, so the toast can be seen going away. */
export function createFixturePreparationBackend(name: PreparationFixtureName): PreparationBackend {
  let status = fixturePreparationStatus(name)
  const listeners = new Set<(value: unknown) => void>()
  const publish = () => listeners.forEach(listener => listener(status))
  return {
    read: async () => status,
    retry: async () => {
      status = { image: task("running", { fraction: 60 }) }
      window.setTimeout(() => { status = fixturePreparationStatus("ready"); publish() }, 2500)
      return status
    },
    listen: async handler => { listeners.add(handler); return () => { listeners.delete(handler) } },
  }
}
