import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, expect, it, vi } from "vitest"

import { ApplicationPreview } from "@/fixtures/application-preview"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"

afterEach(() => { toast.dismiss() })

function lifecycleActions() {
  return { startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn() }
}

function computers() {
  return within(screen.getByRole("region", { name: "Computers" }))
}

function popover() {
  return within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!)
}

it("confirms Stop from the list row with the tray's wording (decision 8)", async () => {
  const user = userEvent.setup()
  const actions = lifecycleActions()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} actions={actions} />)
  await user.click(within(computers().getByText("dev").closest("li")!).getByRole("button", { name: "Stop dev" }))
  expect(popover().getByText("Stop dev?")).toBeVisible()
  expect(popover().getByText("Running processes will be interrupted. Files in /workspace are kept.")).toBeVisible()
  expect(actions.stopComputer).not.toHaveBeenCalled()
  const stop = popover().getByRole("button", { name: "Stop" })
  expect(stop.className).toContain("destructive")
  await user.click(stop)
  await waitFor(() => expect(actions.stopComputer).toHaveBeenCalledExactlyOnceWith("dev"))
})

it("confirms Stop from the computer page header and keeps Cancel harmless", async () => {
  const user = userEvent.setup()
  const actions = lifecycleActions()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} actions={actions} />)
  await user.click(computers().getByRole("button", { name: "Open dev" }))
  const stop = screen.getByRole("button", { name: "Stop dev" })
  expect(stop).toHaveTextContent("Stop…")
  await user.click(stop)
  await user.click(popover().getByRole("button", { name: "Cancel" }))
  expect(actions.stopComputer).not.toHaveBeenCalled()
  await user.click(stop)
  await user.click(popover().getByRole("button", { name: "Stop" }))
  await waitFor(() => expect(actions.stopComputer).toHaveBeenCalledExactlyOnceWith("dev"))
})

it("confirms Restart from the ⋯ menu of a running computer, but not Start", async () => {
  const user = userEvent.setup()
  const actions = lifecycleActions()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} actions={actions} />)
  await user.click(computers().getByRole("button", { name: "More actions for dev" }))
  const restart = screen.getByRole("menuitem", { name: "Restart dev" })
  expect(restart).toHaveTextContent("Restart…")
  expect(screen.getByRole("menuitem", { name: "Delete dev" })).toHaveTextContent("Delete…")
  await user.click(restart)
  expect(await screen.findByText("Restart dev?")).toBeVisible()
  expect(actions.restartComputer).not.toHaveBeenCalled()
  await user.click(popover().getByRole("button", { name: "Restart" }))
  await waitFor(() => expect(actions.restartComputer).toHaveBeenCalledExactlyOnceWith("dev"))

  // Start never asks.
  await user.click(within(computers().getByText("playgrounds").closest("li")!).getByRole("button", { name: "Start playgrounds" }))
  expect(actions.startComputer).toHaveBeenCalledExactlyOnceWith("playgrounds")
})

it("confirms Stop and Restart inside the command palette", async () => {
  const user = userEvent.setup()
  const actions = lifecycleActions()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} actions={actions} />)
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "stop dev")
  await user.click(screen.getByRole("option", { name: "Stop dev…" }))
  const palette = within(screen.getByRole("dialog", { name: "Commands" }))
  expect(palette.getByText("Stop dev?")).toBeVisible()
  expect(palette.getByText("Running processes will be interrupted. Files in /workspace are kept.")).toBeVisible()
  expect(actions.stopComputer).not.toHaveBeenCalled()
  await user.click(palette.getByRole("button", { name: "Stop" }))
  expect(actions.stopComputer).toHaveBeenCalledExactlyOnceWith("dev")

  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "restart dev")
  expect(screen.getByRole("option", { name: "Restart dev…" })).toBeVisible()
})

it("restarts a crashed computer without asking, since nothing is running", async () => {
  const user = userEvent.setup()
  const actions = lifecycleActions()
  const source = structuredClone(applicationSourceForScenario("running"))
  const dev = source.computers.find(({ configuration }) => configuration.name === "dev")!
  Object.assign(dev, { state: "failed", attention: { level: "error", message: "The computer runtime crashed. Restart it to retry." } })
  render(<ApplicationPreview source={source} actions={actions} />)
  await user.click(computers().getByRole("button", { name: "More actions for dev" }))
  const restart = screen.getByRole("menuitem", { name: "Restart dev" })
  expect(restart).toHaveTextContent(/^Restart$/)
  await user.click(restart)
  expect(actions.restartComputer).toHaveBeenCalledExactlyOnceWith("dev")
})


it.each(["busy", "replaced"])("drops a palette confirmation when its computer becomes %s", async change => {
  const source = structuredClone(applicationSourceForScenario("running"))
  const actions = lifecycleActions()
  const user = userEvent.setup()
  const view = render(<ApplicationPreview source={source} actions={actions} />)
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "stop dev")
  await user.click(screen.getByRole("option", { name: "Stop dev…" }))
  expect(within(screen.getByRole("dialog", { name: "Commands" })).getByText("Stop dev?")).toBeVisible()

  const changed = structuredClone(source)
  const dev = changed.computers.find(({ configuration }) => configuration.name === "dev")!
  if (change === "replaced") dev.configuration.id = "replacement-vm"
  else dev.lifecycleAction = "restart"
  view.rerender(<ApplicationPreview source={changed} actions={actions} />)
  const dialog = within(screen.getByRole("dialog", { name: "Commands" }))
  expect(dialog.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument()
  expect(dialog.getByRole("combobox", { name: "Search commands" })).toBeVisible()
  expect(actions.stopComputer).not.toHaveBeenCalled()
})

it("uses the current computer name when confirming a palette command after a rename", async () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  const actions = lifecycleActions()
  const user = userEvent.setup()
  const view = render(<ApplicationPreview source={source} actions={actions} />)
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "stop dev")
  await user.click(screen.getByRole("option", { name: "Stop dev…" }))
  const renamed = structuredClone(source)
  renamed.computers.find(({ configuration }) => configuration.name === "dev")!.configuration.name = "renamed"
  view.rerender(<ApplicationPreview source={renamed} actions={actions} />)
  const dialog = within(screen.getByRole("dialog", { name: "Commands" }))
  expect(dialog.getByText("Stop renamed?")).toBeVisible()
  await user.click(dialog.getByRole("button", { name: "Stop" }))
  expect(actions.stopComputer).toHaveBeenCalledExactlyOnceWith("renamed")
})
