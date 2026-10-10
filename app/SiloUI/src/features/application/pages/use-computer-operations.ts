import { parseRemoteComputerTarget } from "../model/connections"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import type { ApplicationActions, ApplicationComputer, ApplicationSource } from "@/features/application/model/application-source"
import { computerBusyReason } from "@/features/computers/model/computer-presentation"
import { nextComputerOrder, computerOrderKey, computerOrderRanks } from "@/features/computers/model/computer-order"
import { useSettingsSelector, useSettingsStore } from "@/features/preferences/settings-store"
import { showActionFailure } from "@/lib/operation-toast"

/** The computer list's commit, delete, reorder and validation callbacks, shared by the list and
 * the detail page so both go through the same paths. */
export function useComputerOperations({ source, actions, computers, committedComputers, configurations, onConfigurationsChange }: {
  source: ApplicationSource
  actions: ApplicationActions
  computers: ReadonlyMap<string, ApplicationComputer>
  committedComputers: ReadonlyMap<string, ApplicationComputer>
  configurations: SetupComputerConfiguration[]
  onConfigurationsChange: (configurations: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[]) => Promise<void> | void
}) {
  const getComputerDeviceId = (configuration: SetupComputerConfiguration) => parseRemoteComputerTarget(configuration.id)?.deviceId ?? computers.get(configuration.id)?.device?.id
  const localOnly = (list: readonly SetupComputerConfiguration[]) => list.filter(configuration => !getComputerDeviceId(configuration))
  const localConfigurations = localOnly(configurations)

  // Build the save from the baseline the editor started from (falling back to the live
  // local list) so the change carries the right `expected` state and does not drag other
  // computers' concurrent edits into this one.
  function updateLocal(configuration: SetupComputerConfiguration, original?: SetupComputerConfiguration, baseline?: SetupComputerConfiguration[]) {
    const base = baseline ? localOnly(baseline) : localConfigurations
    const next = original ? base.map(item => item.id === original.id ? configuration : item) : [...base, configuration]
    return onConfigurationsChange(next, baseline ? base : undefined)
  }

  const { updateSettings } = useSettingsStore()
  // This device's own order of the list, local and remote computers alike.
  const computerOrder = useSettingsSelector((view) => view.settings.computerOrder)
  const orderRanks = computerOrderRanks(computerOrder)
  const orderRank = (configuration: SetupComputerConfiguration) => {
    const computer = computers.get(configuration.id)
    return computer ? orderRanks.get(computerOrderKey(computer)) : undefined
  }
  const reorderComputers = (ids: string[]) => {
    const shown = ids.flatMap(id => { const computer = computers.get(id); return computer ? [computerOrderKey(computer)] : [] })
    void updateSettings({ computerOrder: nextComputerOrder(computerOrder, shown) })
  }
  const commitComputer = actions.saveRemoteComputer ? async (configuration: SetupComputerConfiguration, original: SetupComputerConfiguration | undefined, deviceId: string, baseline?: SetupComputerConfiguration[]) => {
    if (deviceId) await actions.saveRemoteComputer!(deviceId, configuration, original)
    else await updateLocal(configuration, original, baseline)
  } : undefined
  const deleteComputer = actions.deleteRemoteComputer ? async (configuration: SetupComputerConfiguration, baseline?: SetupComputerConfiguration[]) => {
    const device = computers.get(configuration.id)?.device
    if (device) {
      if (!device.connected) throw new Error(`${device.name} is offline. Reconnect to it before deleting ${configuration.name}.`)
      await actions.deleteRemoteComputer!(device.id, configuration)
    } else {
      const base = baseline ? localOnly(baseline) : localConfigurations
      await onConfigurationsChange(base.filter(item => item.id !== configuration.id), baseline ? base : undefined)
    }
  } : undefined
  const changeConfigurations = (next: SetupComputerConfiguration[], baseline?: SetupComputerConfiguration[]) => {
    if (source.computerOperationsUnavailable) { notifyOperationUnavailable(); return }
    return onConfigurationsChange(localOnly(next), baseline ? localOnly(baseline) : undefined)
  }
  const validateComputerOperation = (configuration: SetupComputerConfiguration, isNew: boolean, deviceId?: string) => {
    const device = computers.get(configuration.id)?.device ?? source.devices?.find(device => device.id === deviceId)
    if (deviceId && !device) return "The selected device was removed. Choose another device before saving."
    if (device) return device.busy ? `${device.name} is updating. Wait before changing ${configuration.name}.` : device.connected ? undefined : `${device.name} is offline. Reconnect to it before changing ${configuration.name}.`
    if (source.computerOperationsUnavailable) return source.computerOperationsUnavailable
    const notice = source.resourceNotice
    if (!isNew || notice?.kind !== "create-storage" || configuration.name !== notice.computer) return undefined
    return `Not enough storage to create ${configuration.name}. About ${notice.requiredGB} GiB is needed on ${notice.volume}; ${notice.availableGB} GiB is available. No computer was created.`
  }
  const isComputerCreated = (configuration: SetupComputerConfiguration) => committedComputers.has(configuration.id)
  const isComputerRunning = (configuration: SetupComputerConfiguration) => computers.get(configuration.id)?.state === "running"
  const configurationBusyReason = (configuration: SetupComputerConfiguration) => computerBusyReason(computers.get(configuration.id))

  function notifyOperationUnavailable() {
    showActionFailure("Computer operation unavailable", source.computerOperationsUnavailable ?? "Computer operations are unavailable.", undefined, { native: false })
  }

  return { getComputerDeviceId, orderRank, reorderComputers, commitComputer, deleteComputer, changeConfigurations, validateComputerOperation, isComputerCreated, isComputerRunning, configurationBusyReason }
}
