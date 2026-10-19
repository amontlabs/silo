import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { Toaster } from "@/components/ui/sonner"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import { ComputerConfigurationList } from "./computer-configuration-list"

const configuration = productionComputerDefaults[0]
const staleError = new Error("This computer changed while your edit was waiting. Review it and try again.")

async function openEditor(configurations: readonly SetupComputerConfiguration[], props: Record<string, unknown> = {}) {
  const view = render(<TooltipProvider><ComputerConfigurationList configurations={configurations} onConfigurationsChange={vi.fn()}
    isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} {...props} /></TooltipProvider>)
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  return { user, view }
}

it("keeps the editor open with the user's edits when a save is rejected as stale", async () => {
  const onCommitComputer = vi.fn().mockRejectedValue(staleError)
  const { user } = await openEditor([configuration], { onCommitComputer, getDeviceId: () => "" })
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  await user.click(screen.getByRole("button", { name: "Save" }))

  expect(await screen.findByRole("alert")).toHaveTextContent("This computer changed since you opened it.")
  // The edited value is preserved rather than discarded.
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
  expect(screen.getByRole("button", { name: "Review changes" })).toBeInTheDocument()

  // Review changes clears the conflict but keeps the user's edit on the latest settings.
  await user.click(screen.getByRole("button", { name: "Review changes" }))
  expect(screen.queryByText("This computer changed since you opened it.")).not.toBeInTheDocument()
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
  expect(screen.getByRole("status", { name: "Review changes" })).toHaveTextContent("Your edits are kept on top of the latest settings.")
})

it("shows fields changed on both sides and saves the user's edits on top of the latest settings", async () => {
  const onCommitComputer = vi.fn().mockRejectedValueOnce(staleError).mockResolvedValue(undefined)
  const props = { onCommitComputer, getDeviceId: () => "" }
  const { user, view } = await openEditor([configuration], props)
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  const latest = { ...configuration, cpus: 6, maxMemoryGiB: 64 }
  view.rerender(<TooltipProvider><ComputerConfigurationList configurations={[latest]} onConfigurationsChange={vi.fn()}
    isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} {...props} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: "Save" }))
  await user.click(await screen.findByRole("button", { name: "Review changes" }))

  const review = screen.getByRole("status", { name: "Review changes" })
  expect(review).toHaveTextContent("CPUs at start: yours 4 CPUs, elsewhere 6 CPUs")
  expect(review).toHaveTextContent("Updated from elsewhere: Maximum memory.")
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
  expect(screen.getByRole("combobox", { name: "Maximum memory" })).toHaveValue("64")

  await user.click(screen.getByRole("button", { name: "Save" }))
  expect(onCommitComputer).toHaveBeenLastCalledWith({ ...latest, cpus: 4 }, latest, "", [latest])
})

it("discards edits and closes the editor from the conflict prompt", async () => {
  const onCommitComputer = vi.fn().mockRejectedValue(staleError)
  const { user } = await openEditor([configuration], { onCommitComputer, getDeviceId: () => "" })
  await user.click(screen.getByRole("button", { name: "Save" }))
  await screen.findByRole("button", { name: "Discard my edits" })
  await user.click(screen.getByRole("button", { name: "Discard my edits" }))
  expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument()
})

it("notices when the computer is changed elsewhere while the editor is open", async () => {
  const { view } = await openEditor([configuration])
  view.rerender(<TooltipProvider><ComputerConfigurationList configurations={[{ ...configuration, maxCPUs: 4 }]} onConfigurationsChange={vi.fn()}
    isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
  expect(await screen.findByRole("status")).toHaveTextContent("This computer was changed elsewhere.")
  expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
})

it("keeps the edit's original baseline when another row receives a reorder key", async () => {
  const second = { ...configuration, id: "00000000-0000-4000-8000-000000000002", name: "second" }
  const props = { onCommitComputer: vi.fn().mockResolvedValue(undefined), onReorder: vi.fn(), getDeviceId: () => "" }
  const { user, view } = await openEditor([configuration, second], props)
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  const latest = { ...configuration, maxMemoryGiB: 64 }
  view.rerender(<TooltipProvider><ComputerConfigurationList configurations={[latest, second]} onConfigurationsChange={vi.fn()}
    isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} {...props} /></TooltipProvider>)
  const reorder = screen.getByRole("button", { name: "Reorder second" })
  expect(reorder).toHaveAttribute("aria-disabled", "true")
  expect(reorder).toHaveAttribute("tabindex", "-1")
  reorder.focus()
  // This is a no-op at the end of the list, so the editor stays open.
  await user.keyboard("{ArrowDown}")
  await user.click(screen.getByRole("button", { name: "Save" }))
  expect(props.onReorder).not.toHaveBeenCalled()
  expect(props.onCommitComputer).toHaveBeenCalledExactlyOnceWith({ ...configuration, cpus: 4 }, configuration, "", [configuration, second])
})

it("keeps the edit's original baseline when another computer is deleted", async () => {
  const second = { ...configuration, id: "00000000-0000-4000-8000-000000000002", name: "second" }
  const props = { onCommitComputer: vi.fn().mockResolvedValue(undefined), onDeleteComputer: vi.fn().mockResolvedValue(undefined), getDeviceId: () => "" }
  const { user, view } = await openEditor([configuration, second], props)
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  const latest = { ...configuration, maxMemoryGiB: 64 }
  const renderConfigurations = (configurations: SetupComputerConfiguration[]) => <TooltipProvider><ComputerConfigurationList configurations={configurations} onConfigurationsChange={vi.fn()}
    isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} {...props} /></TooltipProvider>
  view.rerender(renderConfigurations([latest, second]))
  await user.click(screen.getByRole("button", { name: "More actions for second" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete second" }))
  const popover = within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!)
  await user.click(popover.getByRole("button", { name: "Delete permanently" }))
  await waitFor(() => expect(props.onDeleteComputer).toHaveBeenCalledExactlyOnceWith(second, [latest, second]))
  view.rerender(renderConfigurations([latest]))
  await user.click(screen.getByRole("button", { name: "Save" }))
  expect(props.onCommitComputer).toHaveBeenCalledExactlyOnceWith({ ...configuration, cpus: 4 }, configuration, "", [configuration, second])
})

it("reports a stale rejection of Add Linux desktop, which has no editor to show it", async () => {
  const onCommitComputer = vi.fn().mockRejectedValue(staleError)
  render(<TooltipProvider><Toaster /><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()} onCommitComputer={onCommitComputer}
    getDeviceId={() => "office"} isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: "Add Linux desktop" }))
  expect(await screen.findByText(`Could not save ${configuration.name}`)).toBeVisible()
  expect(screen.getByText(staleError.message)).toBeVisible()
})

it("reports a stale rejection after the editor closed on a local save", async () => {
  let reject!: (cause: unknown) => void
  const onConfigurationsChange = vi.fn(() => new Promise<void>((_resolve, fail) => { reject = fail }))
  render(<TooltipProvider><Toaster /><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={onConfigurationsChange}
    isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  await user.click(screen.getByRole("button", { name: "Save" }))
  expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument()
  await act(async () => reject(staleError))
  expect(await screen.findByText(`Could not save ${configuration.name}`)).toBeVisible()
  expect(screen.getByText(staleError.message)).toBeVisible()
})

it("blocks saving when the computer was deleted elsewhere while the editor is open", async () => {
  const { view } = await openEditor([configuration])
  view.rerender(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={vi.fn()}
    isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
  expect(await screen.findByText("This computer no longer exists.")).toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
})
