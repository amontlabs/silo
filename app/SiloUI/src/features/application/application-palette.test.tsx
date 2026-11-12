import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, expect, it, vi } from "vitest"

import { ApplicationPreview } from "@/fixtures/application-preview"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"
import { clearComputerAgents } from "@/features/agents/model/use-computer-agents"

afterEach(() => { toast.dismiss() })

async function run(user: ReturnType<typeof userEvent.setup>, search: string, option: string | RegExp) {
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), search)
  await user.click(screen.getByRole("option", { name: option }))
}

function withStoppedDev(): ApplicationSource {
  const source = structuredClone(applicationSourceForScenario("running"))
  source.computers.find(({ configuration }) => configuration.name === "dev")!.state = "stopped"
  return source
}

it("opens a computer's page and its Checkpoints tab", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} />)
  await run(user, "open dev", "Open dev")
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Computersdev")
  await run(user, "dev checkpoints", "Open dev checkpoints")
  expect(screen.getByRole("tab", { name: "Checkpoints" })).toHaveAttribute("aria-selected", "true")
})

it("opens the same folder picker as the editor buttons instead of opening the editor directly", async () => {
  const user = userEvent.setup()
  const openEditor = vi.fn()
  const source = applicationSourceForScenario("running")
  render(<ApplicationPreview source={source} actions={{ openEditor }} />)
  await run(user, "dev editor", `Open dev in ${source.preferences.editor}…`)
  expect(await screen.findByRole("heading", { name: "dev folders" })).toBeVisible()
  expect(openEditor).not.toHaveBeenCalled()
})

it("forks from the palette through the computer's own Fork popover", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} />)
  await run(user, "fork dev", "Fork dev…")
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Computersdev")
  const popover = await waitFor(() => within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!))
  expect(popover.getByText("Fork dev")).toBeVisible()
  expect(popover.getByRole("textbox")).toHaveFocus()

  // Leaving the page and coming back does not reopen the popover.
  await user.keyboard("{Escape}")
  await waitFor(() => expect(document.querySelector("[data-slot=popover-content]")).toBeNull())
  await user.click(within(screen.getByRole("navigation", { name: "Breadcrumb" })).getByRole("button", { name: "Computers" }))
  await user.click(within(screen.getByRole("region", { name: "Computers" })).getByRole("button", { name: "Open dev" }))
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Computersdev")
  expect(document.querySelector("[data-slot=popover-content]")).toBeNull()
})

it("deletes from the palette through the one Delete dialog", async () => {
  const user = userEvent.setup()
  const saveComputerConfiguration = vi.fn()
  render(<ApplicationPreview source={withStoppedDev()} actions={{ saveComputerConfiguration }} />)
  await run(user, "delete dev", "Delete dev…")
  const popover = await waitFor(() => within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!))
  expect(popover.getByText("Delete dev permanently?")).toBeVisible()
  await user.click(popover.getByRole("button", { name: "Delete permanently" }))
  await waitFor(() => expect(saveComputerConfiguration).toHaveBeenCalled())
})

it("does not offer Delete for a running computer", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} />)
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "delete dev")
  expect(screen.queryByRole("option", { name: "Delete dev…" })).not.toBeInTheDocument()
})

it("starts a new computer from the palette", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} />)
  await run(user, "new computer", "New computer…")
  expect(await screen.findByRole("textbox", { name: "Computer name" })).toBeVisible()
})

it("keeps same-named computers on different devices apart", async () => {
  const user = userEvent.setup()
  const openTerminal = vi.fn()
  const source = structuredClone(applicationSourceForScenario("running"))
  const dev = source.computers.find(({ configuration }) => configuration.name === "dev")!
  const twin = (id: string) => ({ ...structuredClone(dev), configuration: { ...dev.configuration, id: `remote-${id}` }, device: { id, computerId: `vm-${id}`, name: "Office", address: `${id}.test`, connected: true } })
  source.computers = [twin("first"), twin("second")]
  source.devices = [{ id: "first", name: "Office", address: "first.test", connected: true }, { id: "second", name: "Office", address: "second.test", connected: true }]
  render(<ApplicationPreview source={source} actions={{ openTerminal }} />)
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "dev terminal")
  const options = screen.getAllByRole("option", { name: `Open dev on Office in ${source.preferences.terminal}` })
  expect(options).toHaveLength(2)
  // Keyboard selection tells the two apart only when each item has its own value.
  expect(options.filter(option => option.getAttribute("aria-selected") === "true")).toHaveLength(1)
  await user.keyboard("{ArrowDown}{Enter}")
  expect(openTerminal).toHaveBeenCalledExactlyOnceWith("silo-remote:second:vm-second")
})

it("opens each installed agent of a running computer from the palette", async () => {
  const user = userEvent.setup()
  const openAgent = vi.fn()
  const listAgents = vi.fn(async () => [
    { id: "claude-code" as const, name: "Claude Code", state: "ready" as const, version: "2.1.287", expectedVersion: "2.1.287" },
    { id: "codex" as const, name: "Codex", state: "missing" as const, version: undefined, expectedVersion: "0.167.0" },
  ])
  clearComputerAgents()
  const source = applicationSourceForScenario("running")
  const dev = source.computers.find(({ configuration }) => configuration.name === "dev")!
  render(<ApplicationPreview source={source} actions={{ openAgent, listAgents, installAgent: vi.fn() }} />)
  await waitFor(() => expect(listAgents).toHaveBeenCalledWith(computerTarget(dev)))
  await run(user, "claude code dev", "Open Claude Code in dev")
  expect(openAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(dev), "claude-code")
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "codex dev")
  expect(screen.queryByRole("option", { name: "Open Codex in dev" })).not.toBeInTheDocument()
})

it("offers no agent command for a computer that is not running", async () => {
  const user = userEvent.setup()
  clearComputerAgents()
  const listAgents = vi.fn(async () => [{ id: "claude-code" as const, name: "Claude Code", state: "unknown" as const, version: undefined, expectedVersion: "2.1.287" }])
  render(<ApplicationPreview source={withStoppedDev()} actions={{ openAgent: vi.fn(), listAgents, installAgent: vi.fn() }} />)
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "claude code dev")
  expect(screen.queryByRole("option", { name: "Open Claude Code in dev" })).not.toBeInTheDocument()
})
