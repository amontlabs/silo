import { expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions } from "../model/application-source"
import { computerTarget } from "../model/connections"
import { applicationCommands } from "./application-commands"

function remoteRunningSource() {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.runtimeRepair = null
  source.computerConfigurationOperation = null
  source.activities = []
  const local = source.computers.find(item => !item.device)!
  const remote = structuredClone(local)
  remote.configuration = { ...remote.configuration, id: `${local.configuration.id}-office` }
  remote.device = { id: "office", computerId: "vm-office", name: "Office", address: "office.test", connected: true }
  for (const computer of [local, remote]) {
    computer.state = "running"
    computer.freshness = "fresh"
    computer.attention = undefined
    computer.lifecycleAction = undefined
  }
  source.computers.push(remote)
  return { source, local, remote }
}

it("addresses a remote computer by its device target and names the device", () => {
  const { source, remote } = remoteRunningSource()
  const actions = { openTerminal: vi.fn(), openEditor: vi.fn(), startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn() } as unknown as ApplicationActions
  const commands = applicationCommands(source, actions, vi.fn())
  const target = computerTarget(remote)

  const terminal = commands.find(command => command.id === `${remote.configuration.id}:terminal`)!
  expect(terminal.label).toBe(`Open ${remote.configuration.name} on Office in ${source.preferences.terminal}`)
  expect(terminal.keywords).toContain("Office")
  terminal.run()
  expect(actions.openTerminal).toHaveBeenCalledWith(target)

  commands.find(command => command.id === `${remote.configuration.id}:editor`)!.run()
  expect(actions.openEditor).toHaveBeenCalledWith(target)
})

it("asks Stop in the palette without navigating away from the current page", () => {
  const { source, local } = remoteRunningSource()
  const actions = { stopComputer: vi.fn() } as unknown as ApplicationActions
  const navigate = vi.fn()
  const commands = applicationCommands(source, actions, navigate)
  const stop = commands.find(command => command.id === `${local.configuration.id}:Stop`)!
  expect(stop.confirm).toMatchObject({ confirmLabel: "Stop" })
  stop.run()
  expect(navigate).not.toHaveBeenCalled()
  expect(actions.stopComputer).toHaveBeenCalledWith(computerTarget(local))
})
