import { createContext, useContext, useEffect } from "react"

import type { NetworkMode } from "@/desktop/production-schemas"
import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"
import { showActionFailure } from "@/lib/operation-toast"

/** Whether the app can set a network policy, so a create form offers the choice. */
export const NetworkPolicySupport = createContext(false)

export function useNetworkPolicySupport() {
  return useContext(NetworkPolicySupport)
}

/** The network a new computer starts with: the default, or one of the three modes. */
export type NetworkChoice = "default" | NetworkMode

/**
 * Modes chosen in a create form, by computer name. A new computer has no policy to set until it
 * exists, so the choice waits here until the computer is listed and settled.
 */
const planned = new Map<string, NetworkMode>()

export function planNetworkMode(computerName: string, choice: NetworkChoice | null | undefined) {
  if (choice && choice !== "default") planned.set(computerName, choice)
  else planned.delete(computerName)
}

export function clearPlannedNetworkModes() {
  planned.clear()
}

/** Sets each planned mode on its computer once the computer exists on this device. */
export function usePlannedNetworkModes(source: ApplicationSource | null | undefined, save: ApplicationActions["saveNetworkPolicy"]) {
  useEffect(() => {
    if (!save || planned.size === 0 || !source) return
    for (const computer of source.computers) {
      const name = computer.configuration.name
      const mode = planned.get(name)
      if (!mode || computer.device || computer.freshness === "stale" || computer.lifecycleAction) continue
      if (computer.macos && computer.macos.setupComplete === false) continue
      planned.delete(name)
      save(computerTarget(computer), { mode, hosts: [], presets: [] }).catch((error: unknown) => showActionFailure(`Could not set the network of ${name}`, error, undefined, { native: false }))
    }
  }, [source, save])
}
