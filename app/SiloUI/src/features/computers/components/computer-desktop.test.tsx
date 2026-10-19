import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { Toaster } from "@/components/ui/sonner"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import { setupComputerConfigurationRequestSchema } from "@/contracts/silo"
import { ComputerConfigurationList } from "./computer-configuration-list"

const configuration = productionComputerDefaults[0]
function editor(draft: SetupComputerConfiguration, created: boolean) {
  const save = vi.fn()
  render(<TooltipProvider><ComputerConfigurationList configurations={created ? [draft] : []} onConfigurationsChange={save}
    isComputerCreated={() => created} isComputerRunning={() => created}
    initialEditorDraft={{ draft, originalID: created ? draft.id : undefined, insertAt: 0 }} /></TooltipProvider>)
  return save
}

describe("optional Linux desktop", () => {
  it("installs from the computer menu while preserving its configuration", async () => {
    const save = vi.fn()
    render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={save}
      isComputerCreated={() => true} isComputerRunning={() => true}
      getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
    await user.click(screen.getByRole("menuitem", { name: "Add Linux desktop" }))
    expect(save).toHaveBeenCalledWith([{ ...configuration, desktop: { startWithComputer: true } }], [configuration])
  })
  it("routes menu installation to the owning device and surfaces failures", async () => {
    const save = vi.fn().mockRejectedValue(new Error("Device disconnected"))
    const localSave = vi.fn()
    render(<TooltipProvider><Toaster /><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={localSave}
      onCommitComputer={save} getDeviceId={() => "remote-device"}
      isComputerCreated={() => true} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
    await user.click(screen.getByRole("menuitem", { name: "Add Linux desktop" }))
    expect(save).toHaveBeenCalledWith({ ...configuration, desktop: { startWithComputer: true } }, configuration, "remote-device", [configuration])
    expect(await screen.findByText("Device disconnected")).toBeVisible()
    expect(localSave).not.toHaveBeenCalled()
  })
  it("does not offer installation when the desktop is already configured", async () => {
    render(<TooltipProvider><ComputerConfigurationList configurations={[{ ...configuration, desktop: { startWithComputer: true } }]}
      onConfigurationsChange={vi.fn()} isComputerCreated={() => true}
      getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
    await userEvent.setup().click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
    expect(screen.queryByRole("menuitem", { name: "Add Linux desktop" })).not.toBeInTheDocument()
  })
  it("respects configuration locks", async () => {
    render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()}
      interactionDisabled isComputerCreated={() => true}
      getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
    await userEvent.setup().click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
    expect(screen.getByRole("menuitem", { name: "Add Linux desktop" })).toHaveAttribute("aria-disabled", "true")
  })
  it("checks operation eligibility before installing", async () => {
    const save = vi.fn()
    const validate = vi.fn().mockReturnValue("This device is unavailable.")
    render(<TooltipProvider><Toaster /><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={save}
      validateOperation={validate} isComputerCreated={() => true}
      getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
    await user.click(screen.getByRole("menuitem", { name: "Add Linux desktop" }))
    expect(validate).toHaveBeenCalledWith({ ...configuration, desktop: { startWithComputer: true } }, false, "")
    expect(await screen.findByText("This device is unavailable.")).toBeVisible()
    expect(save).not.toHaveBeenCalled()
  })
  it("keeps legacy configurations desktop-free and retains an explicit startup policy", () => {
    expect(setupComputerConfigurationRequestSchema.parse({ schemaVersion: 1, computers: [configuration] }).computers[0]).not.toHaveProperty("desktop")
    expect(setupComputerConfigurationRequestSchema.parse({ schemaVersion: 1, computers: [{ ...configuration, desktop: { startWithComputer: false } }] }).computers[0]).toMatchObject({ desktop: { startWithComputer: false } })
  })
  it("opts in during creation with automatic startup", async () => {
    const user = userEvent.setup()
    const save = editor(configuration, false)
    expect(screen.getByRole("checkbox", { name: "Linux desktop" })).not.toBeChecked()
    await user.click(screen.getByRole("checkbox", { name: "Linux desktop" }))
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(save).toHaveBeenCalledWith([expect.objectContaining({ desktop: { startWithComputer: true } })])
  })
  it("adds a desktop to a running computer without asking to stop it", async () => {
    const user = userEvent.setup()
    const save = editor(configuration, true)
    await user.click(screen.getByRole("button", { name: "Add Linux desktop" }))
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).toHaveBeenCalledWith([expect.objectContaining({ desktop: { startWithComputer: true } })])
  })
  it("changes startup policy without offering desktop removal or stopping the computer", async () => {
    const user = userEvent.setup()
    const save = editor({ ...configuration, desktop: { startWithComputer: true } }, true)
    expect(screen.queryByRole("button", { name: "Add Linux desktop" })).not.toBeInTheDocument()
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument()
    await user.click(screen.getByRole("switch", { name: "Start desktop with computer" }))
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).toHaveBeenCalledWith([expect.objectContaining({ desktop: { startWithComputer: false } })])
  })
  it("still explains the computer stop required by a resource change", async () => {
    const user = userEvent.setup()
    editor({ ...configuration, desktop: { startWithComputer: true } }, true)
    await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
    expect(screen.getByRole("button", { name: "Stop and save…" })).toBeVisible()
  })
})
