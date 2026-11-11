import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import type { DeviceResources } from "@/desktop/production-schemas"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"

import { KeepFreeSection } from "./keep-free-section"

const GIB = 2 ** 30
const resources = (change: Partial<DeviceResources> = {}): DeviceResources => ({
  logicalCpus: 10, memoryTotalBytes: 16 * GIB, memoryAvailableBytes: 3.1 * GIB, diskFreeBytes: 214 * GIB, diskTotalBytes: 494 * GIB,
  memoryFloorBytes: 6.4 * GIB, diskFloorBytes: 24.7 * GIB, runningComputers: 2, runningMemoryGib: 8,
  state: "low", message: "Memory is low: Silo keeps 6.4 GiB free, and 3.1 GiB is available.", computers: [], ...change,
})

function setup(settings = {}, read = vi.fn(async () => resources())) {
  const store = createMemorySettingsStore(settings)
  render(<SettingsProvider store={store}><KeepFreeSection read={read} /></SettingsProvider>)
  return { store, read, user: userEvent.setup() }
}

it("shows the floors a device keeps, their effective defaults and the device's state in a sentence", async () => {
  setup()
  const section = await screen.findByRole("region", { name: "Keep free for this device" })
  expect(await within(section).findByText(/Now 6\.4 GiB \(the default\)\./)).toBeVisible()
  expect(within(section).getByText(/Now 24\.7 GiB \(the default\)\./)).toBeVisible()
  expect(within(section).getByRole("status")).toHaveTextContent("Memory is low: Silo keeps 6.4 GiB free, and 3.1 GiB is available.")
  expect(within(section).getByRole("textbox", { name: "Memory to keep free, in GiB" })).toHaveValue("")
  expect(within(section).getByRole("button", { name: "Use the default memory to keep free" })).toBeDisabled()
})

it("saves a floor, shows what applies, and returns to the default", async () => {
  const read = vi.fn().mockResolvedValueOnce(resources()).mockResolvedValue(resources({ memoryFloorBytes: 8 * GIB }))
  const { store, user } = setup({}, read)
  const field = await screen.findByRole("textbox", { name: "Memory to keep free, in GiB" })
  await user.type(field, "8")
  await user.tab()
  await waitFor(() => expect(store.getSnapshot().settings.memoryFloorGib).toBe(8))
  expect(await screen.findByText(/Now 8 GiB\./)).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Use the default memory to keep free" }))
  await waitFor(() => expect(store.getSnapshot().settings.memoryFloorGib).toBeNull())
})

it("commits on Enter and does not save twice when the field blurs afterwards", async () => {
  const { store, user } = setup()
  const spy = vi.spyOn(store, "updateSettings")
  const field = await screen.findByRole("textbox", { name: "Memory to keep free, in GiB" })
  await user.type(field, "8{Enter}")
  await user.tab()
  expect(spy).toHaveBeenCalledTimes(1)
  expect(store.getSnapshot().settings.memoryFloorGib).toBe(8)
})

it("refuses a number the engine would refuse, before saving", async () => {
  const { store, user } = setup()
  const field = await screen.findByRole("textbox", { name: "Disk to keep free, in GiB" })
  await user.type(field, "1")
  await user.tab()
  expect(await screen.findByRole("alert")).toHaveTextContent("Enter a whole number of GiB, at least 2.")
  await user.clear(field)
  await user.type(field, "abc")
  await user.tab()
  expect(await screen.findByRole("alert")).toHaveTextContent("Enter a whole number of GiB, at least 2.")
  expect(store.getSnapshot().settings.diskFloorGib).toBeNull()
})

it("is left out where the device's resources cannot be read", () => {
  render(<SettingsProvider store={createMemorySettingsStore()}><KeepFreeSection /></SettingsProvider>)
  expect(screen.queryByRole("region", { name: "Keep free for this device" })).not.toBeInTheDocument()
})
