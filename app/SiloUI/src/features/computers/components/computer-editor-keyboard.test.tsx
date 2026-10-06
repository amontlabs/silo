import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { ComputerConfigurationList } from "./computer-configuration-list"

const configuration = productionComputerDefaults[0]!

function renderEditor({ running = false, created = true } = {}) {
  const onConfigurationsChange = vi.fn()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={onConfigurationsChange}
    isComputerCreated={() => created} isComputerRunning={() => running}
    initialEditorDraft={{ draft: configuration, originalID: configuration.id, insertAt: 0 }} /></TooltipProvider>)
  return { onConfigurationsChange, user: userEvent.setup() }
}

describe("computer editor keyboard", () => {
  it("saves with Enter in a field", async () => {
    const { onConfigurationsChange, user } = renderEditor({ created: false })
    await user.click(screen.getByRole("textbox", { name: "Computer name" }))
    await user.keyboard("{Enter}")
    expect(onConfigurationsChange).toHaveBeenCalledOnce()
  })

  it("cancels with Escape", async () => {
    const { onConfigurationsChange, user } = renderEditor()
    await user.click(screen.getByRole("textbox", { name: "Computer name" }))
    await user.keyboard("{Escape}")
    expect(screen.queryByRole("textbox", { name: "Computer name" })).not.toBeInTheDocument()
    expect(onConfigurationsChange).not.toHaveBeenCalled()
  })

  it("lets Escape dismiss the stop confirmation without closing the editor", async () => {
    const { user } = renderEditor({ running: true })
    await user.click(screen.getByRole("button", { name: "Stop and save…" }))
    await user.keyboard("{Escape}")
    expect(screen.queryByRole("group", { name: /and save\?/ })).not.toBeInTheDocument()
    expect(screen.getByRole("textbox", { name: "Computer name" })).toBeInTheDocument()
  })

  it("shows the name rule before any failed save", () => {
    renderEditor({ created: false })
    expect(screen.getByRole("textbox", { name: "Computer name" })).toHaveAccessibleDescription("1–32 lowercase letters, numbers, or hyphens, starting with a letter.")
  })
})
