import type { PreparationBackend, PreparationStatus, PreparationTask } from "@/desktop/preparation"

// Deterministic fixtures for the background preparation toast. Select with `?preparation=<name>`
// in the browser preview (combine with `&chatgpt=downloading`); nothing here reaches Silo services.
export const preparationFixtureNames = ["downloading-image", "importing", "downloading-lcu", "both", "failed", "ready"] as const
export type PreparationFixtureName = typeof preparationFixtureNames[number]

export function preparationFixtureFromSearch(search: string): PreparationFixtureName | undefined {
  const requested = new URLSearchParams(search).get("preparation")
  return preparationFixtureNames.find(name => name === requested)
}

const task = (state: PreparationTask["state"], extra: Partial<PreparationTask> = {}): PreparationTask => ({ state, fraction: null, message: null, retryable: false, ...extra })

export function fixturePreparationStatus(name: PreparationFixtureName): PreparationStatus {
  switch (name) {
    case "downloading-image": return { image: task("running", { fraction: 42 }), lcu: task("pending") }
    case "importing": return { image: task("running"), lcu: task("pending") }
    case "downloading-lcu": return { image: task("ready"), lcu: task("running") }
    case "both": return { image: task("running"), lcu: task("running") }
    case "failed": return { image: task("ready"), lcu: task("failed", { message: "Silo could not download LCU. Check your network connection, then retry.", retryable: true }) }
    case "ready": return { image: task("ready"), lcu: task("ready") }
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
      status = { image: task("ready"), lcu: task("running") }
      window.setTimeout(() => { status = fixturePreparationStatus("ready"); publish() }, 2500)
      return status
    },
    listen: async handler => { listeners.add(handler); return () => { listeners.delete(handler) } },
  }
}
