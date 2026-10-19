import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { Toaster } from "@/components/ui/sonner"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions } from "../model/application-source"
import { OverviewPage } from "./overview-page"
import { remoteComputerTarget } from "../model/connections"

function setup(connected = true) {
  const source = applicationSourceForScenario("running")
  const device = { id: "office", name: "Office Mac", address: "user@office", connected }
  const remote = structuredClone(source.computers[0])
  remote.device = { ...device, computerId: remote.configuration.id }
  remote.configuration.id = remoteComputerTarget(device.id, remote.configuration.id)
  remote.freshness = connected ? "fresh" : "stale"
  source.computers.push(remote)
  source.devices = [device]
  const actions = { saveRemoteComputer: vi.fn().mockResolvedValue(undefined), deleteRemoteComputer: vi.fn().mockResolvedValue(undefined), startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn(), connectDevice: vi.fn() } as unknown as ApplicationActions
  const onConfigurationsChange = vi.fn()
  const view = render(<OverviewPage source={source} actions={actions} onConfigurationsChange={onConfigurationsChange} />)
  return { source, remote, actions, onConfigurationsChange, view, user: userEvent.setup() }
}

it("keeps the existing computer list and shows remote ownership through a focusable badge", async () => {
  const { remote, actions, user, source, view, onConfigurationsChange } = setup()
  const badge = screen.getByLabelText(/Computer on Office Mac/)
  expect(badge).toHaveAttribute("tabindex", "0")
  const row = within(badge.closest("li")!)
  expect(row.queryByText("Restart required")).not.toBeInTheDocument()
  await user.click(row.getByRole("button", { name: `Stop ${remote.configuration.name}` }))
  // Stopping a running computer confirms first, naming its device (decision 8).
  const stop = within((await screen.findByText(`Stop ${remote.configuration.name} on Office Mac?`)).closest<HTMLElement>("[data-slot=popover-content]")!)
  expect(actions.stopComputer).not.toHaveBeenCalled()
  await user.click(stop.getByRole("button", { name: "Stop" }))
  await waitFor(() => expect(actions.stopComputer).toHaveBeenCalledWith(remote.configuration.id))
  await user.click(row.getByRole("button", { name: `More actions for ${remote.configuration.name}` }))
  expect(screen.getByRole("menuitem", { name: `Delete ${remote.configuration.name} on Office Mac` })).toHaveAttribute("aria-disabled", "true")
  await user.keyboard("{Escape}")
  remote.state = "stopped"
  view.rerender(<OverviewPage source={{ ...source }} actions={actions} onConfigurationsChange={onConfigurationsChange} />)
  await user.click(row.getByRole("button", { name: `More actions for ${remote.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${remote.configuration.name} on Office Mac` }))
  expect(await screen.findByText(`Delete ${remote.configuration.name} on Office Mac permanently?`)).toBeVisible()
  await user.click(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Delete permanently" }))
  expect(actions.deleteRemoteComputer).toHaveBeenCalledWith("office", remote.configuration)
})

it("disables remote lifecycle operations while preserving last-known rows when the device is unavailable", () => {
  const { remote } = setup(false)
  expect(screen.getByText("4 computers · 3 on this device · 1 on other devices")).toBeVisible()
  const row = within(screen.getByLabelText(/Computer on Office Mac/).closest("li")!)
  expect(row.getByText("Offline · last known status")).toBeVisible()
  expect(row.getByRole("button", { name: `Stop ${remote.configuration.name}` })).toBeDisabled()
  expect(row.queryByRole("button", { name: `Delete ${remote.configuration.name} on Office Mac` })).not.toBeInTheDocument()
})

it("creates a computer on the selected device without rewriting the local inventory", async () => {
  const { actions, onConfigurationsChange, user } = setup()
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(screen.getByRole("menuitem", { name: "New computer" }))
  await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "office")
  await user.click(screen.getByRole("button", { name: "Create" }))
  expect(actions.saveRemoteComputer).toHaveBeenCalledWith("office", expect.objectContaining({ name: expect.any(String) }), undefined)
  expect(onConfigurationsChange).not.toHaveBeenCalled()
})

it("edits a remote computer with the same name as a local computer using its original configuration", async () => {
  const { actions, remote, user } = setup()
  const row = within(screen.getByLabelText(/Computer on Office Mac/).closest("li")!)
  await user.click(row.getByRole("button", { name: `More actions for ${remote.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${remote.configuration.name}` }))
  expect(screen.getByRole("combobox", { name: "Run on" })).toBeDisabled()
  await user.click(screen.getByRole("button", { name: "Stop and save…" }))
  await user.click(screen.getByRole("button", { name: "Stop and save" }))
  expect(actions.saveRemoteComputer).toHaveBeenCalledWith("office", remote.configuration, remote.configuration)
})

it("removes the last computer from a remote device and can create from an empty list", async () => {
  const source = applicationSourceForScenario("running")
  const original = source.computers[0]
  original.state = "stopped"
  const device = { id: "office", name: "Office Mac", address: "user@office", connected: true }
  const remote = { ...original, configuration: { ...original.configuration, id: remoteComputerTarget("office", original.configuration.id) }, device: { ...device, computerId: original.configuration.id } }
  source.computers = [remote]
  source.devices = [device]
  const actions = { saveRemoteComputer: vi.fn().mockResolvedValue(undefined), deleteRemoteComputer: vi.fn().mockResolvedValue(undefined), stopComputer: vi.fn(), restartComputer: vi.fn() } as unknown as ApplicationActions
  const onConfigurationsChange = vi.fn()
  const user = userEvent.setup()
  const view = render(<OverviewPage source={source} actions={actions} onConfigurationsChange={onConfigurationsChange} />)
  await user.click(screen.getByRole("button", { name: `More actions for ${remote.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${remote.configuration.name} on Office Mac` }))
  await user.click(within((await screen.findByText(`Delete ${remote.configuration.name} on Office Mac permanently?`)).closest<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Delete permanently" }))
  await waitFor(() => expect(actions.deleteRemoteComputer).toHaveBeenCalledWith("office", remote.configuration))
  view.rerender(<OverviewPage source={{ ...source, computers: [] }} actions={actions} onConfigurationsChange={onConfigurationsChange} />)
  expect(screen.getByText("0 computers · 0 on this device · 0 on other devices")).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(screen.getByRole("menuitem", { name: "New computer" }))
  await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "office")
  await user.click(screen.getByRole("button", { name: "Create" }))
  expect(actions.saveRemoteComputer).toHaveBeenCalledWith("office", expect.objectContaining({ name: expect.any(String) }), undefined)
})

it("permits removing the last local computer without affecting connected devices", async () => {
  const source = applicationSourceForScenario("running")
  source.computers = [source.computers[0]]
  source.computers[0].state = "stopped"
  const configuration = source.computers[0].configuration
  const actions = { saveRemoteComputer: vi.fn(), deleteRemoteComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn() } as unknown as ApplicationActions
  const onConfigurationsChange = vi.fn()
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={actions} onConfigurationsChange={onConfigurationsChange} />)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${configuration.name}` }))
  await user.click(within((await screen.findByText(`Delete ${configuration.name} permanently?`)).closest<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Delete permanently" }))
  await waitFor(() => expect(onConfigurationsChange).toHaveBeenCalledWith([], [configuration]))
  expect(actions.deleteRemoteComputer).not.toHaveBeenCalled()
})

it("keeps a local edit scoped to local configurations when a connected device is removed", async () => {
  const { source, actions, onConfigurationsChange, user, view } = setup()
  source.computers.forEach(computer => { computer.state = "stopped" })
  view.rerender(<OverviewPage source={{ ...source }} actions={actions} onConfigurationsChange={onConfigurationsChange} />)
  const local = source.computers.filter(computer => !computer.device).map(computer => computer.configuration)
  const configuration = local[0]!
  const row = within(document.querySelector<HTMLElement>(`li[data-computer-id="${configuration.id}"]`)!)
  await user.click(row.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  view.rerender(<OverviewPage source={{ ...source, computers: source.computers.filter(computer => !computer.device), devices: [] }}
    actions={actions} onConfigurationsChange={onConfigurationsChange} />)
  await user.click(screen.getByRole("button", { name: "Save" }))
  expect(onConfigurationsChange).toHaveBeenCalledExactlyOnceWith(local.map(item => item.id === configuration.id ? { ...item, cpus: 4 } : item), local)
  expect(actions.saveRemoteComputer).not.toHaveBeenCalled()
})


it("keeps a new computer draft when its selected device is removed before Create", async () => {
  const { source, actions, onConfigurationsChange, user, view } = setup()
  render(<Toaster />)
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(screen.getByRole("menuitem", { name: "New computer" }))
  await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "office")
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "2")
  view.rerender(<OverviewPage source={{ ...source, computers: source.computers.filter(computer => !computer.device), devices: [] }}
    actions={actions} onConfigurationsChange={onConfigurationsChange} />)
  await user.click(screen.getByRole("button", { name: "Create" }))
  expect(actions.saveRemoteComputer).not.toHaveBeenCalled()
  expect(onConfigurationsChange).not.toHaveBeenCalled()
  expect(await screen.findByText("The selected device was removed. Choose another device before saving.")).toBeVisible()
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("2")
  await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "")
  await user.click(screen.getByRole("button", { name: "Create" }))
  expect(onConfigurationsChange).toHaveBeenCalledWith(expect.arrayContaining([expect.objectContaining({ cpus: 2 })]), expect.any(Array))
})
