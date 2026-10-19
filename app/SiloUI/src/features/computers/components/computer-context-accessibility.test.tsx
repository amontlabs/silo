import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { DeviceBadge } from "./device-badge"
import { ComputerConfigurationList } from "./computer-configuration-list"
import { ComputerListRow } from "./computer-list"

it("exposes the computer's management and runtime controls as named groups", () => {
  render(<ComputerListRow name="dev" detail="Stopped"
    hoverActions={<button type="button">Edit dev</button>}
    actions={<button type="button">Start dev</button>} />)
  expect(within(screen.getByRole("group", { name: "Manage dev" })).getByRole("button", { name: "Edit dev" })).toBeVisible()
  expect(within(screen.getByRole("group", { name: "Controls for dev" })).getByRole("button", { name: "Start dev" })).toBeVisible()
})

it("names each computer list group with its own heading", () => {
  render(<TooltipProvider>
    <ComputerConfigurationList configurations={[]} onConfigurationsChange={vi.fn()} />
    <ComputerConfigurationList configurations={[]} onConfigurationsChange={vi.fn()} />
  </TooltipProvider>)
  const groups = screen.getAllByRole("group", { name: "Computers" })
  expect(groups).toHaveLength(2)
  expect(new Set(groups.map(group => group.getAttribute("aria-labelledby"))).size).toBe(2)
  for (const group of groups) {
    const heading = within(group).getByRole("heading", { name: "Computers" })
    expect(group).toHaveAttribute("aria-labelledby", heading.id)
    expect(within(group).getByRole("list", { name: "Configured computers" })).toBeVisible()
  }
})

it("exposes the focusable device badge as a named note with connection context", async () => {
  const user = userEvent.setup()
  render(<DeviceBadge device={{ id: "office", name: "Office", address: "office.example", connected: false, computerId: "dev" }} />)
  const note = screen.getByRole("note", { name: "Computer on Office · Offline · last known status · office.example" })
  await user.tab()
  expect(note).toHaveFocus()
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Office · Offline · last known status · office.example")
})

it("names the focusable read-only disk groups and keeps their values available", async () => {
  const configuration = productionComputerDefaults[0]
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()} isComputerCreated={() => true} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Computer name" })).toHaveFocus())
  for (const label of ["CPUs at start", "Maximum CPUs", "Memory at start", "Maximum memory"]) {
    await user.tab()
    expect(screen.getByRole("combobox", { name: label })).toHaveFocus()
  }
  for (const [label, value] of [["Workspace disk", configuration.workspaceStorageGiB], ["Runtime disk", configuration.runtimeStorageGiB]] as const) {
    const group = screen.getByRole("group", { name: `${label}: ${value} GiB, read-only` })
    expect(within(group).getByRole("combobox", { name: label })).toBeDisabled()
    await user.tab()
    expect(group).toHaveFocus()
  }
})
