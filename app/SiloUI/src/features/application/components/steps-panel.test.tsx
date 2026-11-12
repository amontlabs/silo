import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { toast } from "sonner"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { fixtureComputerSteps, fixtureComputerTree } from "@/fixtures/computer-steps"
import { StepsPanel, noCheckpointReason, type StepsActions } from "./steps-panel"
import { arrangeTree } from "./fork-tree-layout"

const computer = { configuration: { id: "vm-dev", name: "dev" } }

afterEach(() => { toast.dismiss() })

function renderPanel(actions: StepsActions, disabled = false) {
  return render(<SettingsProvider initialSettings={{ theme: "light" }}><Toaster /><TooltipProvider><StepsPanel computer={computer} target="dev" actions={actions} disabled={disabled} /></TooltipProvider></SettingsProvider>)
}

it("lists steps newest first with command, exit code and checkpoint", async () => {
  renderPanel({ listComputerSteps: vi.fn().mockResolvedValue(fixtureComputerSteps) })
  await screen.findByText(/#3 git status/)
  expect([...document.querySelectorAll("[data-step-number]")].map(row => row.getAttribute("data-step-number"))).toEqual(["3", "2", "1"])
  const failed = document.querySelector<HTMLElement>('[data-step-number="2"]')!
  expect(within(failed).getByText("Exit 1")).toBeTruthy()
  expect(within(failed).getByText(/Checkpoint saved/)).toBeTruthy()
  expect(within(document.querySelector<HTMLElement>('[data-step-number="3"]')!).getByText(/No checkpoint/)).toBeTruthy()
})

it("explains that step checkpoints are off when there are no steps", async () => {
  renderPanel({ listComputerSteps: vi.fn().mockResolvedValue([]) })
  expect(await screen.findByText("No steps yet")).toBeTruthy()
  expect(screen.getByText(/--step-checkpoints/)).toBeTruthy()
})

it("forks from the step's checkpoint with the trimmed name", async () => {
  const user = userEvent.setup()
  const forkCheckpoint = vi.fn().mockResolvedValue(undefined)
  renderPanel({ listComputerSteps: vi.fn().mockResolvedValue(fixtureComputerSteps), forkCheckpoint })
  await screen.findByText(/#2 npm test/)
  await user.click(screen.getByRole("button", { name: "Fork from step 2" }))
  await user.type(screen.getByRole("textbox", { name: "New computer name" }), " retry ")
  await user.click(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Fork" }))
  await waitFor(() => expect(forkCheckpoint).toHaveBeenCalledWith("dev", "step-cp-2", "retry"))
  await waitFor(() => expect(document.querySelector("[data-slot=popover-content]")).toBeNull())
  expect(screen.queryByText(/already exists/)).toBeNull()
})

it("disables the fork action with a reason when the step has no checkpoint", async () => {
  renderPanel({ listComputerSteps: vi.fn().mockResolvedValue(fixtureComputerSteps), forkCheckpoint: vi.fn() })
  await screen.findByText(/#3 git status/)
  const button = screen.getByRole("button", { name: "Fork from step 3" })
  expect((button as HTMLButtonElement).disabled).toBe(true)
  expect(button.parentElement).toHaveAttribute("aria-label", noCheckpointReason)
  expect((screen.getByRole("button", { name: "Fork from step 1" }) as HTMLButtonElement).disabled).toBe(false)
})

it("shows the fork tree with the pending review marker", async () => {
  renderPanel({ listComputerSteps: vi.fn().mockResolvedValue([]), readComputerTree: vi.fn().mockResolvedValue(fixtureComputerTree) })
  const tree = await screen.findByRole("list", { name: "Fork tree" })
  expect(within(tree).getAllByRole("listitem").map(item => item.getAttribute("data-tree-node"))).toEqual(["main", "try-redis", "redis-tuning", "try-sqlite"])
  expect(within(tree.querySelector<HTMLElement>('[data-tree-node="try-redis"]')!).getByText("Review requested")).toBeTruthy()
  expect(arrangeTree(fixtureComputerTree).map(row => row.depth)).toEqual([0, 1, 2, 1])
})
