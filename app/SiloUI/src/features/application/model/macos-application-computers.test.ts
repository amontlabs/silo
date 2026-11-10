import { expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { MacosComputersStore } from "@/features/macos-computers/model/macos-computers"
import type { ApplicationActions } from "./application-source"
import { computerAvailability } from "./computer-availability"

import type { MacosComputer } from "@/features/macos-computers/model/macos-computers"
import { logIdentity } from "./logs"
import { macosApplicationComputer, macosApplicationComputer as macosLogComputer, macosApplicationActions, macosApplicationComputers, selectableMacosIds, withMacosComputers } from "./macos-application-computers"

const computer: MacosComputer = { id: "mac-1", name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: null, state: "running", progress: null, detail: null, displayOpen: false, installed: true, setupComplete: true }

it("reads the logs of a local macOS computer by its own id", () => {
  expect(logIdentity(macosLogComputer(computer))).toEqual({ computerId: "mac-1" })
})

it("reads the logs of another device's macOS computer from that device by the owner's id", () => {
  const listed = macosLogComputer(computer, { id: "studio", name: "Studio", address: "studio.local", connected: true })
  expect(logIdentity(listed)).toEqual({ computerId: "mac-1", deviceId: "studio" })
  expect(listed.configuration.id).toBe("silo-remote:studio:mac-1")
  expect(listed.device?.name).toBe("Studio")
})

it("keeps another device's macOS computers selectable only while that device is connected", () => {
  const state = { supported: true, unsupportedReason: null, computers: [computer], template: null, minDiskGiB: 32 }
  const snapshot = { state, error: null, warning: null, remote: { studio: { state, error: null, updating: false } } }
  expect(selectableMacosIds(snapshot, [{ id: "studio" }])).toEqual(["mac-1", "silo-remote:studio:mac-1"])
  expect(selectableMacosIds(snapshot, [])).toEqual(["mac-1"])
  expect(selectableMacosIds(snapshot, undefined)).toEqual(["mac-1"])
})

const device = { id: "studio", name: "Studio", address: "studio.local", connected: true }

it("lists a macOS computer like a Linux one, with its own record beside it", () => {
  const listed = macosApplicationComputer({ ...computer, osVersion: "26.6.2 (25G83)" })
  expect(listed).toMatchObject({ os: "macos", state: "running", stateDetail: "Running", purpose: "macOS", freshness: "fresh" })
  expect(listed.macos).toEqual({ ...computer, osVersion: "26.6.2 (25G83)" })
  expect(listed.configuration).toMatchObject({ id: "mac-1", name: "daily", cpus: 4, memoryGiB: 8, workspaceStorageGiB: 64 })
})

it.each([
  ["preparing", "starting", "Preparing"],
  ["copying", "starting", "Copying macOS"],
  ["setting-up", "starting", "Setting up macOS"],
  ["stopping", "starting", "Stopping"],
  ["stopped", "stopped", "Stopped"],
  ["failed", "failed", "Failed"],
] as const)("shows %s as %s while saying what the computer is doing", (state, expected, detail) => {
  expect(macosApplicationComputer({ ...computer, state })).toMatchObject({ state: expected, stateDetail: detail })
})

it("flags a failed computer with its reason", () => {
  expect(macosApplicationComputer({ ...computer, state: "failed", detail: "The download did not finish." }).attention).toEqual({ level: "error", message: "The download did not finish." })
  expect(macosApplicationComputer({ ...computer, state: "failed" }).attention).toBeUndefined()
})

it("carries the repositories and secrets of a computer of this device, and the lifecycle action in flight", () => {
  const listed = macosApplicationComputer(computer, undefined, {
    repositories: new Map([["daily", ["acme/app"]]]),
    secrets: new Map([["daily", ["API_KEY"]]]),
    pending: new Map([["daily", "stop"]]),
  })
  expect(listed).toMatchObject({ githubRepositories: ["acme/app"], secretNames: ["API_KEY"], lifecycleAction: "stop" })
  const remote = macosApplicationComputer(computer, device, { repositories: new Map([["daily", ["acme/app"]]]), secrets: new Map([["daily", ["API_KEY"]]]) })
  expect(remote).toMatchObject({ githubRepositories: [], secretNames: [] })
})

it("lists this device's macOS computers after the Linux ones, and a connected device's after those", () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  source.devices = [device]
  source.github.computers = [{ computer: "daily", identity: { name: "", email: "", apply: false }, repositories: [{ repository: "acme/app", allowPushes: false }] }]
  source.secrets = [{ id: "s1", name: "API_KEY", computers: ["daily"], allowedDomains: [], state: "active" }]
  const state = { supported: true, unsupportedReason: null, computers: [computer], template: null, minDiskGiB: 32 }
  const hosted = { supported: true, unsupportedReason: null, computers: [{ ...computer, id: "mac-9", name: "build" }], template: null, minDiskGiB: 32 }
  const merged = withMacosComputers(source, { state, error: null, warning: null, remote: { studio: { state: hosted, error: null, updating: false } } })
  const added = merged.computers.slice(source.computers.length)
  expect(added.map(({ configuration }) => configuration.name)).toEqual(["daily", "build"])
  expect(added[0]).toMatchObject({ githubRepositories: ["acme/app"], secretNames: ["API_KEY"] })
  expect(added[1].device?.id).toBe("studio")
  expect(merged.computers.slice(0, source.computers.length)).toEqual(source.computers)
})

it("lists no macOS computers where macOS is unsupported", () => {
  const source = applicationSourceForScenario("running")
  const state = { supported: false, unsupportedReason: "Needs Apple silicon.", computers: [computer], template: null, minDiskGiB: 32 }
  expect(withMacosComputers(source, { state, error: null, warning: null, remote: {} })).toBe(source)
  expect(macosApplicationComputers(undefined, source)).toEqual([])
})

it("sends Start, Stop and Show screen of a macOS computer to the macOS store, and everything else on", async () => {
  const listed = macosApplicationComputer(computer)
  const store = { action: vi.fn(async () => {}), openDisplay: vi.fn(async () => {}) } as unknown as MacosComputersStore
  const actions = { startComputer: vi.fn(), stopComputer: vi.fn(), openDesktop: vi.fn(), openTerminal: vi.fn() } as unknown as ApplicationActions
  const run = vi.fn((_target: string, _kind: string, task: () => Promise<void>) => { void task() })
  const wired = macosApplicationActions(actions, [listed, applicationSourceForScenario("running").computers[0]], store, run)
  wired.startComputer("daily")
  wired.stopComputer("daily")
  await wired.openDesktop?.("daily")
  expect(store.action).toHaveBeenNthCalledWith(1, "mac-1", "start")
  expect(store.action).toHaveBeenNthCalledWith(2, "mac-1", "stop")
  expect(store.openDisplay).toHaveBeenCalledWith("mac-1")
  wired.startComputer("dev")
  await wired.openDesktop?.("dev")
  expect(actions.startComputer).toHaveBeenCalledWith("dev")
  expect(actions.openDesktop).toHaveBeenCalledWith("dev")
  expect(wired.openTerminal).toBe(actions.openTerminal)
})

it("addresses a computer of another device by its target everywhere, including inside its macOS record", () => {
  const listed = macosApplicationComputer(computer, device)
  expect(listed.configuration.id).toBe("silo-remote:studio:mac-1")
  expect(listed.macos?.id).toBe("silo-remote:studio:mac-1")
  expect(listed.device?.computerId).toBe("mac-1")
})

it("does not hold a macOS computer while Linux computer changes are applied", () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  source.computerConfigurationOperation = { id: "op", status: "applying", candidate: { schemaVersion: 1, computers: [] }, progressEvents: [], result: null, error: null }
  const mac = macosApplicationComputer({ ...computer, state: "stopped" })
  expect(computerAvailability(mac, source)).toMatchObject({ canStart: true })
  expect(computerAvailability(source.computers[0], source).canStart).toBe(false)
  expect(computerAvailability(macosApplicationComputer(computer), source).reasons.restart).toMatch(/can’t be restarted/)
})
