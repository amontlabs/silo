import { expect, it } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { defaultStartupComputerIds, startupComputerCandidates } from "./startup-computers"

const office = { id: "office", name: "Office Mac", address: "office.local", connected: true, computerId: "remote-dev" }

it("never picks a computer on another device as the default startup computer", () => {
  const [dev, playgrounds, personal] = structuredClone(applicationSourceForScenario("running").computers)
  const remoteDev = { ...dev, configuration: { ...dev.configuration, id: "remote-dev" }, device: office }
  expect(defaultStartupComputerIds([remoteDev, playgrounds, dev, personal])).toEqual([dev.configuration.id])
  expect(defaultStartupComputerIds([remoteDev, playgrounds, personal])).toEqual([playgrounds.configuration.id])
  expect(defaultStartupComputerIds([remoteDev])).toEqual([])
  expect(startupComputerCandidates([remoteDev, playgrounds]).map(({ configuration }) => configuration.id)).toEqual([playgrounds.configuration.id])
})

it("uses the first installed local macOS computer when there is no local Linux computer", () => {
  expect(defaultStartupComputerIds([], [
    { id: "preparing", installed: false, state: "downloading" },
    { id: "ready", installed: true, state: "stopped" },
  ])).toEqual(["ready"])
})
