import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { ComputerConfigurationList } from "./computer-configuration-list"

function popoverButton(name: string) { return within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name }) }

it("identifies the remote device and describes both destructive choices", async () => {
  const configuration = productionComputerDefaults[0]
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()}
    devices={[{ id: "office", name: "Office", connected: true }]} getDeviceId={() => "office"}
    getRowPresentation={() => ({ menuActions: [], deleteDetails: { checkpoints: 2, exportFirst: vi.fn().mockResolvedValue(true) } })} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${configuration.name} on Office` }))
  const dialog = await screen.findByRole("dialog", { name: `Delete ${configuration.name} on Office` })
  for (const action of ["Delete permanently", "Export, then delete"]) {
    expect(within(dialog).getByRole("button", { name: action })).toHaveAccessibleDescription(`Delete ${configuration.name} on Office permanently? Its files and 2 checkpoints will be deleted. This can't be undone.`)
  }
})

it("confirms a row deletion in a popover anchored to the ⋯ menu", async () => {
  const configuration = productionComputerDefaults[0]
  const save = vi.fn()
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={save} isComputerRunning={() => false} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${configuration.name}` }))
  expect(await screen.findByText(`Delete ${configuration.name} permanently?`)).toBeVisible()
  expect(screen.getByRole("dialog", { name: `Delete ${configuration.name}` })).toBeVisible()
  expect(screen.getByText("Its files and checkpoints will be deleted. This can't be undone.")).toBeVisible()
  expect(popoverButton("Delete permanently")).toHaveAccessibleDescription(`Delete ${configuration.name} permanently? Its files and checkpoints will be deleted. This can't be undone.`)
  expect(save).not.toHaveBeenCalled()
  await user.click(popoverButton("Delete permanently"))
  await waitFor(() => expect(save).toHaveBeenCalledWith([], [configuration]))
})

it("cancelling the popover deletes nothing", async () => {
  const configuration = productionComputerDefaults[0]
  const save = vi.fn()
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={save} isComputerRunning={() => false} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${configuration.name}` }))
  await user.click(await screen.findByRole("button", { name: "Cancel" }))
  expect(save).not.toHaveBeenCalled()
  expect(screen.queryByText(`Delete ${configuration.name} permanently?`)).not.toBeInTheDocument()
})

it("blocks a deletion if the computer starts before confirmation", async () => {
  const configuration = productionComputerDefaults[0]
  const save = vi.fn()
  const view = (running: boolean) => <TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={save} isComputerRunning={() => running} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>
  const { rerender } = render(view(false))
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${configuration.name}` }))
  await screen.findByText(`Delete ${configuration.name} permanently?`)
  rerender(view(true))
  await user.click(popoverButton("Delete permanently"))
  await new Promise(resolve => setTimeout(resolve, 20))
  expect(save).not.toHaveBeenCalled()
})

it.each(["configuration lock", "offline device"])("blocks deletion when a %s appears before confirmation", async reason => {
  const configuration = productionComputerDefaults[0]
  const save = vi.fn()
  const view = (blocked: boolean) => <TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()} onDeleteComputer={save}
    interactionDisabled={reason === "configuration lock" && blocked}
    getDeviceId={() => "office"}
    validateOperation={() => reason === "offline device" && blocked ? "Office is offline." : undefined}
    isComputerRunning={() => false} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>
  const { rerender } = render(view(false))
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${configuration.name}` }))
  await screen.findByText(`Delete ${configuration.name} permanently?`)
  rerender(view(true))
  await user.click(popoverButton("Delete permanently"))
  expect(save).not.toHaveBeenCalled()
})

it("closes the delete confirmation on one Escape after hovering its menu trigger", async () => {
  const configuration = productionComputerDefaults[0]
  const save = vi.fn()
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={save} isComputerRunning={() => false} /></TooltipProvider>)
  const trigger = screen.getByRole("button", { name: `More actions for ${configuration.name}` })
  await user.hover(trigger)
  await user.click(trigger)
  await user.click(screen.getByRole("menuitem", { name: `Delete ${configuration.name}` }))
  expect(await screen.findByText(`Delete ${configuration.name} permanently?`)).toBeVisible()
  await user.keyboard("{Escape}")
  await waitFor(() => expect(screen.queryByText(`Delete ${configuration.name} permanently?`)).not.toBeInTheDocument())
  expect(trigger).toHaveFocus()
  expect(save).not.toHaveBeenCalled()
})
