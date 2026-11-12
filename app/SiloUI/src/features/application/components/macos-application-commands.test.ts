import { expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createFixtureMacosComputersStore } from "@/fixtures/macos-computers"
import { withMacosComputers } from "../model/macos-application-computers"
import type { ApplicationActions } from "../model/application-source"
import { applicationCommands } from "./application-commands"

async function labels(remote = false) {
  const store = createFixtureMacosComputersStore()
  await store.refresh()
  const source = structuredClone(applicationSourceForScenario("complete"))
  const local = store.getSnapshot().state!
  // The same computer, hosted by another device instead of this one.
  const hosted = { ...local, computers: local.computers.filter(computer => computer.id === "mac-running") }
  source.devices = remote ? [{ id: "studio", name: "Studio", address: "studio.local", connected: true }] : []
  const snapshot = remote ? { ...store.getSnapshot(), state: { ...local, computers: [] }, remote: { studio: { state: hosted, error: null, updating: false } } } : store.getSnapshot()
  const merged = withMacosComputers(source, snapshot)
  const actions = { forkCheckpoint: vi.fn() } as unknown as ApplicationActions
  return applicationCommands(merged, actions, vi.fn(), { onComputerRequest: vi.fn(), onExportComputer: vi.fn() }).map(command => command.label)
}

it("offers a macOS computer the commands its system supports", async () => {
  const commands = await labels()
  expect(commands).toEqual(expect.arrayContaining([
    "Open daily", "Open daily checkpoints", "Open daily files", "Open daily logs", "Open daily network",
    "Stop daily…", "Export daily…", "Start xcode-build", "Delete xcode-build…",
  ]))
  expect(commands.filter(label => /daily|xcode-build/.test(label) && /^(Restart|Fork)/.test(label))).toEqual([])
  expect(commands.some(label => /^Open daily in /.test(label))).toBe(true)
})

it("gives a macOS computer of another device its row's commands and no page", async () => {
  const commands = await labels(true)
  expect(commands).toContain("Open daily on Studio logs")
  expect(commands).toContain("Stop daily on Studio…")
  expect(commands).not.toContain("Open daily on Studio")
  expect(commands).not.toContain("Open daily on Studio files")
  expect(commands).not.toContain("Open daily on Studio network")
  expect(commands.some(label => /^Open daily on Studio in /.test(label))).toBe(false)
})
