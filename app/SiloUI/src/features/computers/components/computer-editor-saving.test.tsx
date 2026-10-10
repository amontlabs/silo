import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { ComputerConfigurationList } from "./computer-configuration-list"

const configuration = productionComputerDefaults[0]!

describe("configuration editor while saving", () => {
  it("locks every field until the save settles", async () => {
    const pending = () => new Promise<void>(() => {})
    render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={pending} onCommitComputer={pending}
      isComputerCreated={() => true} isComputerRunning={() => false}
      initialEditorDraft={{ draft: configuration, originalID: configuration.id, insertAt: 0 }} /></TooltipProvider>)
    const user = userEvent.setup()
    expect(screen.queryByRole("status")).not.toBeInTheDocument()
    await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "2")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(await screen.findByRole("button", { name: "Saving…" })).toBeDisabled()
    expect(screen.getByRole("status")).toHaveTextContent(`Saving ${configuration.name}…`)
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toBeDisabled()
    expect(screen.getByRole("combobox", { name: "Memory at start" })).toBeDisabled()
  })

  it("disables Save with a reason when another change locks editing after the editor opened", async () => {
    const onConfigurationsChange = vi.fn()
    const view = (interactionDisabled: boolean) => <TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={onConfigurationsChange}
      isComputerCreated={() => true} isComputerRunning={() => false} interactionDisabled={interactionDisabled}
      initialEditorDraft={{ draft: configuration, originalID: configuration.id, insertAt: 0 }} /></TooltipProvider>
    const { rerender } = render(view(false))
    const user = userEvent.setup()
    await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "2")
    rerender(view(true))
    const save = screen.getByRole("button", { name: "Save" })
    expect(save).toBeDisabled()
    expect(save).toHaveAccessibleDescription("Saving is paused while another computer change is in progress or needs review.")
    // The draft is kept, so Save works again once the lock clears.
    rerender(view(false))
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("2")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(onConfigurationsChange.mock.lastCall?.[0]).toEqual([expect.objectContaining({ cpus: 2 })])
  })
})
