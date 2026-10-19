import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { ComputerConfigurationList } from "./computer-configuration-list"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"

it("opens the Add menu with the keyboard and navigates its items", async () => {
  const onImportComputer = vi.fn()
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={vi.fn()} onImportComputer={onImportComputer} /></TooltipProvider>)
  const add = screen.getByRole("button", { name: "Add" })
  add.focus()
  await user.keyboard("{ArrowDown}")
  expect(screen.getByRole("menu", { name: "Add computer" })).toBeVisible()
  await waitFor(() => expect(screen.getByRole("menuitem", { name: "New computer" })).toHaveFocus())
  await user.keyboard("{ArrowDown}")
  expect(screen.getByRole("menuitem", { name: "Import computer…" })).toHaveFocus()
  await user.keyboard("{End}")
  expect(screen.getByRole("menuitem", { name: "Import computer…" })).toHaveFocus()
  await user.keyboard("{Home}{Escape}")
  await waitFor(() => expect(add).toHaveFocus())
  expect(screen.queryByRole("menu")).not.toBeInTheDocument()
  expect(onImportComputer).not.toHaveBeenCalled()
})

it("hands focus to the new computer editor after a keyboard menu selection", async () => {
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={vi.fn()} /></TooltipProvider>)
  screen.getByRole("button", { name: "Add" }).focus()
  await user.keyboard("{Enter}")
  await waitFor(() => expect(screen.getByRole("menuitem", { name: "New computer" })).toHaveFocus())
  await user.keyboard("{Enter}")
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Computer name" })).toHaveFocus())
  expect(screen.queryByRole("menu")).not.toBeInTheDocument()
})

it("restores Add focus when an import selection opens no review", async () => {
  const onImportComputer = vi.fn()
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={vi.fn()} onImportComputer={onImportComputer} /></TooltipProvider>)
  const add = screen.getByRole("button", { name: "Add" })
  await user.click(add)
  await user.click(screen.getByRole("menuitem", { name: "Import computer…" }))
  expect(onImportComputer).toHaveBeenCalledOnce()
  await waitFor(() => expect(add).toHaveFocus())
})

it("returns focus to Add when a new computer editor is cancelled", async () => {
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={vi.fn()} /></TooltipProvider>)
  const add = screen.getByRole("button", { name: "Add" })
  await user.click(add)
  await user.click(screen.getByRole("menuitem", { name: "New computer" }))
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  expect(screen.queryByRole("textbox", { name: "Computer name" })).not.toBeInTheDocument()
  expect(add).toHaveFocus()
})

it("returns focus to the row's actions menu when its editor is cancelled", async () => {
  const configuration = productionComputerDefaults[0]
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  expect(screen.getByRole("button", { name: `More actions for ${configuration.name}` })).toHaveFocus()
})

it("returns focus to the row menu when edits opened from that menu are discarded", async () => {
  const configuration = productionComputerDefaults[0]
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  expect(screen.getByRole("button", { name: `More actions for ${configuration.name}` })).toHaveFocus()
})

it("returns focus to the source row when a duplicate editor is cancelled", async () => {
  const configuration = productionComputerDefaults[0]
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Duplicate settings for ${configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  expect(screen.getByRole("button", { name: `More actions for ${configuration.name}` })).toHaveFocus()
})

it("returns focus to the row after a successful asynchronous save", async () => {
  const configuration = productionComputerDefaults[0]
  const user = userEvent.setup()
  const onCommitComputer = vi.fn().mockResolvedValue(undefined)
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()} onCommitComputer={onCommitComputer} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(screen.getByRole("button", { name: `More actions for ${configuration.name}` })).toHaveFocus())
  expect(onCommitComputer).toHaveBeenCalledOnce()
})

it("preserves focus moved outside the editor while a save settles", async () => {
  const configuration = productionComputerDefaults[0]
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]}
    onConfigurationsChange={() => screen.getByRole("button", { name: "Other action" }).focus()}
    footer={<button type="button">Other action</button>} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Save" }))
  expect(screen.getByRole("button", { name: "Other action" })).toHaveFocus()
})

it.each(["Edit", "Duplicate settings for"])("focuses the editor after selecting %s from the row menu", async action => {
  const configuration = productionComputerDefaults[0]
  const user = userEvent.setup()
  render(<TooltipProvider><ComputerConfigurationList configurations={[configuration]} onConfigurationsChange={vi.fn()} getRowPresentation={() => ({ menuActions: [] })} /></TooltipProvider>)
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `${action} ${configuration.name}` }))
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Computer name" })).toHaveFocus())
})

it("describes the reorder keys and announces a keyboard move without losing focus", async () => {
  const first = productionComputerDefaults[0]
  const second = { ...first, id: "00000000-0000-4000-8000-0000000000ff", name: "second" }
  const onConfigurationsChange = vi.fn()
  const view = (configurations: typeof first[]) => <TooltipProvider><ComputerConfigurationList configurations={configurations} onConfigurationsChange={onConfigurationsChange} /></TooltipProvider>
  const { rerender } = render(view([first, second]))
  const user = userEvent.setup()
  const reorder = screen.getByRole("button", { name: `Reorder ${first.name}` })
  expect(reorder).toHaveAccessibleDescription("Use the Up and Down arrow keys to reorder.")
  reorder.focus()
  await user.keyboard("{ArrowDown}")
  expect(onConfigurationsChange).toHaveBeenCalledExactlyOnceWith([second, first], [first, second])
  rerender(view([second, first]))
  expect(reorder).toHaveFocus()
  expect(screen.getByText(`${first.name} moved to position 2 of 2.`)).toHaveAttribute("aria-live", "polite")
})
