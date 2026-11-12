import { render, renderHook, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"
import { NetworkPolicySupport, clearPlannedNetworkModes, planNetworkMode, usePlannedNetworkModes } from "@/features/network-policy/model/planned-network"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { ComputerConfigurationList } from "./computer-configuration-list"

beforeEach(clearPlannedNetworkModes)
afterEach(clearPlannedNetworkModes)

function renderNewComputerForm(supported = true) {
  const onConfigurationsChange = vi.fn()
  render(<SettingsProvider store={createMemorySettingsStore()}><NetworkPolicySupport value={supported}><TooltipProvider>
    <ComputerConfigurationList configurations={[productionComputerDefaults[0]!]} onConfigurationsChange={onConfigurationsChange} />
  </TooltipProvider></NetworkPolicySupport></SettingsProvider>)
  return onConfigurationsChange
}

async function openNewComputerForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Add" }))
  const item = screen.queryByRole("menuitem", { name: "New computer" })
  if (item) await user.click(item)
  return screen.findByTestId(/^computer-editor-/)
}

function sourceWith(state: "running" | "stopped") {
  const source: ApplicationSource = structuredClone(applicationSourceForScenario("running"))
  const computer = source.computers.find(item => !item.device)!
  computer.state = state
  computer.freshness = "fresh"
  return { source, computer }
}

describe("the network choice of the create form", () => {
  it("offers the default and the three modes, with the default chosen", async () => {
    const user = userEvent.setup()
    renderNewComputerForm()
    await openNewComputerForm(user)
    const field = await screen.findByRole("combobox", { name: "Network" })
    expect(field).toHaveValue("default")
    expect([...(field as HTMLSelectElement).options].map(option => option.textContent)).toEqual(["Default", "Open", "Allowlist", "Closed"])
    await user.selectOptions(field, "closed")
    expect(screen.getByText(/Nothing out\./)).toBeVisible()
  })

  it("is not offered where the app cannot set a network", async () => {
    const user = userEvent.setup()
    renderNewComputerForm(false)
    await openNewComputerForm(user)
    expect(screen.queryByRole("combobox", { name: "Network" })).not.toBeInTheDocument()
  })

  it("sets the chosen mode on the new computer once it exists, and nothing for the default", async () => {
    const user = userEvent.setup()
    const onConfigurationsChange = renderNewComputerForm()
    await openNewComputerForm(user)
    const name = await screen.findByRole("textbox", { name: "Computer name" })
    await user.clear(name)
    await user.type(name, "second")
    await user.selectOptions(screen.getByRole("combobox", { name: "Network" }), "closed")
    await user.click(screen.getByRole("button", { name: "Create" }))
    await waitFor(() => expect(onConfigurationsChange).toHaveBeenCalled())
    const { source, computer } = sourceWith("stopped")
    computer.configuration = { ...computer.configuration, name: "second" }
    const save = vi.fn<NonNullable<ApplicationActions["saveNetworkPolicy"]>>(async () => { throw new Error("unused") })
    save.mockResolvedValue({} as never)
    renderHook(() => usePlannedNetworkModes(source, save))
    await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith(computerTarget(computer), { mode: "closed", hosts: [], presets: [] }))
  })
})

describe("setting a planned network", () => {
  it("waits while the computer is still being created, then sets it once", async () => {
    const { source, computer } = sourceWith("stopped")
    const save = vi.fn<NonNullable<ApplicationActions["saveNetworkPolicy"]>>(async () => ({}) as never)
    planNetworkMode(computer.configuration.name, "allowlist")
    const busy = { ...source, computers: source.computers.map(item => item === computer ? { ...item, lifecycleAction: "creating" as never } : item) }
    const { rerender } = renderHook(({ current }) => usePlannedNetworkModes(current, save), { initialProps: { current: busy } })
    expect(save).not.toHaveBeenCalled()
    rerender({ current: source })
    await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith(computerTarget(computer), { mode: "allowlist", hosts: [], presets: [] }))
    rerender({ current: { ...source } })
    expect(save).toHaveBeenCalledTimes(1)
  })

  it("plans nothing for the default", () => {
    const { source, computer } = sourceWith("running")
    const save = vi.fn<NonNullable<ApplicationActions["saveNetworkPolicy"]>>(async () => ({}) as never)
    planNetworkMode(computer.configuration.name, "default")
    renderHook(() => usePlannedNetworkModes(source, save))
    expect(save).not.toHaveBeenCalled()
  })
})
