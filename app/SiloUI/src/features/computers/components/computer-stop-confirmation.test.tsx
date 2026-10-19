import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { ComputerConfigurationList } from "./computer-configuration-list"

const configuration = productionComputerDefaults[0]!

function renderRunningEditor(running = true) {
  const onConfigurationsChange = vi.fn()
  const view = (isRunning: boolean, interactionDisabled = false) => <TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={onConfigurationsChange}
    isComputerCreated={() => true} isComputerRunning={() => isRunning} interactionDisabled={interactionDisabled}
    initialEditorDraft={{ draft: configuration, originalID: configuration.id, insertAt: 0 }} /></TooltipProvider>
  const result = render(view(running))
  return { onConfigurationsChange, user: userEvent.setup(), rerender: (isRunning: boolean, interactionDisabled = false) => result.rerender(view(isRunning, interactionDisabled)) }
}

describe("saving changes that stop a running computer", () => {
  it("confirms the stop inline before saving", async () => {
    const { onConfigurationsChange, user } = renderRunningEditor()
    await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
    await user.click(screen.getByRole("button", { name: "Stop and save…" }))
    expect(onConfigurationsChange).not.toHaveBeenCalled()
    const confirmation = screen.getByRole("group", { name: `Stop ${configuration.name} and save?` })
    expect(confirmation).toHaveTextContent("Running processes will be interrupted.")
    expect(within(confirmation).getByRole("button", { name: "Cancel" })).toHaveFocus()

    await user.click(within(confirmation).getByRole("button", { name: "Cancel" }))
    expect(screen.queryByRole("group", { name: `Stop ${configuration.name} and save?` })).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Stop and save…" })).toHaveFocus()
    expect(onConfigurationsChange).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Stop and save…" }))
    await user.click(screen.getByRole("button", { name: "Stop and save" }))
    expect(onConfigurationsChange).toHaveBeenCalledExactlyOnceWith([{ ...configuration, cpus: 4 }])
  })

  it("dismisses the confirmation with Escape without saving", async () => {
    const { onConfigurationsChange, user } = renderRunningEditor()
    await user.click(screen.getByRole("button", { name: "Stop and save…" }))
    await user.keyboard("{Escape}")
    expect(screen.queryByRole("group", { name: `Stop ${configuration.name} and save?` })).not.toBeInTheDocument()
    expect(onConfigurationsChange).not.toHaveBeenCalled()
  })

  it("saves without asking once the computer has stopped", async () => {
    const { onConfigurationsChange, user, rerender } = renderRunningEditor()
    await user.click(screen.getByRole("button", { name: "Stop and save…" }))
    rerender(false)
    expect(screen.queryByRole("group", { name: `Stop ${configuration.name} and save?` })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(onConfigurationsChange).toHaveBeenCalledOnce()
  })
})


it("revalidates newly reported host capacity before confirming Stop and save", async () => {
  const onConfigurationsChange = vi.fn()
  const view = (capacity?: { logicalCPUs: number; memoryGiB: number }) => <TooltipProvider><ComputerConfigurationList
    configurations={[configuration]} onConfigurationsChange={onConfigurationsChange} getDeviceCapacity={() => capacity}
    isComputerCreated={() => true} isComputerRunning={() => true}
    initialEditorDraft={{ draft: configuration, originalID: configuration.id, insertAt: 0 }} /></TooltipProvider>
  const result = render(view())
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Stop and save…" }))
  result.rerender(view({ logicalCPUs: 8, memoryGiB: 16 }))
  await user.click(screen.getByRole("button", { name: "Stop and save" }))
  expect(onConfigurationsChange).not.toHaveBeenCalled()
  expect(screen.getByRole("spinbutton", { name: "Maximum memory custom (GiB)" }))
    .toHaveAccessibleDescription("This device has 16 GiB of memory. Choose 16 GiB or fewer.")
  expect(screen.queryByRole("group", { name: `Stop ${configuration.name} and save?` })).not.toBeInTheDocument()
})


it.each(["stopped", "locked"])("does not revive an old stop confirmation after the computer was %s", async interruption => {
  const { user, rerender, onConfigurationsChange } = renderRunningEditor()
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  await user.click(screen.getByRole("button", { name: "Stop and save…" }))
  rerender(interruption !== "stopped", interruption === "locked")
  expect(screen.queryByRole("group", { name: `Stop ${configuration.name} and save?` })).not.toBeInTheDocument()
  rerender(true)
  expect(screen.queryByRole("group", { name: `Stop ${configuration.name} and save?` })).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Stop and save…" })).toBeEnabled()
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
  expect(onConfigurationsChange).not.toHaveBeenCalled()
})
