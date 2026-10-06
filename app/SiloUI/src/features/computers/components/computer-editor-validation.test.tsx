import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { ComputerConfigurationList } from "./computer-configuration-list"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { ComputerEditor } from "./computer-editor"

async function openNewComputer() {
  const onConfigurationsChange = vi.fn()
  render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={onConfigurationsChange} /></TooltipProvider>)
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(screen.getByRole("menuitem", { name: "New computer" }))
  return { user, onConfigurationsChange }
}

describe("configuration editor validation", () => {
  it("shows a form error when retained configuration fields cannot be submitted", async () => {
    const configuration = { ...productionComputerDefaults[0], futurePolicy: { enabled: true } }
    const onSave = vi.fn()
    render(<ComputerEditor editor={{ draft: configuration, originalID: configuration.id, insertAt: 0 }} configurations={[configuration]} focusRequest={0} created={true} running={false} onSave={onSave} onCancel={vi.fn()} onDraftChange={vi.fn()} />)
    await userEvent.setup().click(screen.getByRole("button", { name: "Save" }))
    expect(onSave).not.toHaveBeenCalled()
    expect(screen.getByRole("alert")).toHaveTextContent("futurePolicy")
  })

  it("links each error to its field and moves focus to the first invalid field", async () => {
    const { user, onConfigurationsChange } = await openNewComputer()
    const name = screen.getByRole("textbox", { name: "Computer name" })
    await user.clear(name)
    await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "16")
    await user.click(screen.getByRole("button", { name: "Create" }))

    expect(onConfigurationsChange).not.toHaveBeenCalled()
    expect(name).toHaveFocus()
    expect(name).toHaveAttribute("aria-invalid", "true")
    expect(name).toHaveAccessibleDescription("Use 1–32 lowercase letters, numbers, or hyphens, starting with a letter.")
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveAccessibleDescription("CPUs at start cannot exceed the maximum.")
    // Valid fields carry no stale description.
    expect(screen.getByRole("combobox", { name: "Memory at start" })).not.toHaveAttribute("aria-describedby")

    await user.type(name, "dev")
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveFocus()
  })
})
