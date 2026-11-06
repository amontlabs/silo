import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { fixtureComputerSteps } from "@/fixtures/computer-steps"
import { StepsPanel } from "./steps-panel"

const run = vi.hoisted(() => vi.fn().mockResolvedValue(true))
vi.mock("@/features/application/model/checkpoint-operation-toast", () => ({ runCheckpointOperation: run }))

it("runs a step fork under its own operation id, not the checkpoint fork id", async () => {
  const user = userEvent.setup()
  const actions = { listComputerSteps: vi.fn().mockResolvedValue(fixtureComputerSteps), forkCheckpoint: vi.fn() }
  render(<SettingsProvider initialSettings={{ theme: "light" }}><TooltipProvider><StepsPanel computer={{ configuration: { id: "vm-dev", name: "dev" } }} target="dev" actions={actions} disabled={false} /></TooltipProvider></SettingsProvider>)
  await screen.findByText(/#3 git status/)
  await user.click(screen.getByRole("button", { name: "Fork from step 2" }))
  await user.type(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("textbox", { name: "New computer name" }), "retry")
  await user.click(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Fork" }))
  expect(run).toHaveBeenCalledWith(expect.objectContaining({ id: "step:dev:2:fork", kind: "fork" }))
})
