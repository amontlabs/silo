import { expect, it } from "vitest"

import type { MacosComputer } from "@/features/macos-computers/model/macos-computers"
import { logIdentity } from "./logs"
import { macosLogComputer, selectableMacosIds } from "./macos-log-computers"

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
