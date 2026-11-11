import { expect, it } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationSource, ApplicationComputer } from "./application-source"
import { computerAvailability } from "./computer-availability"

function scenario(change: (computer: ApplicationComputer, source: ApplicationSource) => void = () => {}) {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.runtimeRepair = null
  source.computerConfigurationOperation = null
  source.activities = []
  const computer = source.computers.find(({ configuration }) => configuration.name === "dev")!
  Object.assign(computer, { state: "running", freshness: "fresh", attention: undefined, lifecycleAction: undefined, checkpointOperation: null })
  change(computer, source)
  return computerAvailability(computer, source)
}

it("allows opening, stopping and restarting a running computer", () => {
  expect(scenario()).toMatchObject({ busy: false, canOpen: true, canStart: false, canStop: true, canRestart: true })
})

it("offers Start and Restart, but not Open, for a crashed computer with its error", () => {
  const availability = scenario((computer) => {
    computer.state = "failed"
    computer.attention = { level: "error", message: "The computer runtime crashed. Restart it to retry." }
  })
  expect(availability).toMatchObject({ canOpen: false, canStart: true, canStop: false, canRestart: true })
  expect(availability.reasons.open).toBe("The computer runtime crashed. Restart it to retry.")
})

it("closes Start and Restart, with the notice as the reason, for a computer whose storage could not be moved", () => {
  const message = "Needs attention: Its files were kept untouched. Report this problem before using the computer."
  const availability = scenario((computer) => {
    computer.state = "stopped"
    computer.needsStorageUpgrade = true
    computer.attention = { level: "error", message }
  })
  expect(availability).toMatchObject({ canOpen: false, canStart: false, canRestart: false })
  expect(availability.reasons).toMatchObject({ start: message, restart: message, open: message })
})

it("keeps Stop available for a running computer with an error notice", () => {
  const availability = scenario((computer) => { computer.attention = { level: "error", message: "GitHub access could not be applied." } })
  expect(availability).toMatchObject({ canOpen: false, canStop: true, canRestart: true })
})

it.each([
  ["a lifecycle action runs", (computer: ApplicationComputer) => { computer.lifecycleAction = "stop" }, "dev is stopping."],
  ["the computer is starting", (computer: ApplicationComputer) => { computer.state = "starting" }, "Wait for dev to finish starting."],
  ["a checkpoint operation runs", (computer: ApplicationComputer) => { computer.checkpointOperation = { kind: "capture", status: "running", stage: "Saving" } }, "Wait for the checkpoint to finish."],
  ["its device refreshes", (computer: ApplicationComputer) => { computer.device = { id: "office", computerId: "vm", name: "Office", address: "office.test", connected: true, busy: true } }, "Office is updating. Wait before changing this computer."],
  ["its device is offline", (computer: ApplicationComputer) => { computer.device = { id: "office", computerId: "vm", name: "Office", address: "office.test", connected: false }; computer.freshness = "stale" }, "Office is offline. Reconnect it to manage this computer."],
  ["its status is stale", (computer: ApplicationComputer) => { computer.freshness = "stale" }, "Silo could not refresh this computer’s status."],
  ["the runtime needs repair", (_: ApplicationComputer, source: ApplicationSource) => { source.runtimeRepair = { status: "unavailable", checking: false, reason: "Missing", recovery: "" } }, "Resolve the system issue first."],
  ["computer changes apply", (_: ApplicationComputer, source: ApplicationSource) => { source.computerConfigurationOperation = { id: "x", status: "applying", candidate: { schemaVersion: 1, computers: [] }, progressEvents: [], result: null, error: null } }, "Wait for computer changes to finish."],
] as const)("disables every control with one reason while %s", (_, change, reason) => {
  const availability = scenario(change)
  expect(availability).toMatchObject({ canOpen: false, canStart: false, canStop: false, canRestart: false })
  expect(availability.reasons).toEqual({ open: reason, start: reason, stop: reason, restart: reason })
})

it("explains that a stopped computer must be started before opening it", () => {
  const availability = scenario((computer) => { computer.state = "stopped" })
  expect(availability).toMatchObject({ canOpen: false, canStart: true, canStop: false, canRestart: false })
  expect(availability.reasons.open).toBe("Start dev to open it.")
})
