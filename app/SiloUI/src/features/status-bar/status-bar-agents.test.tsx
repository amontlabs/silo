import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import { clearComputerAgents } from "@/features/agents/model/use-computer-agents"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { fixtureDirectoryLoader } from "@/fixtures/directory-loader"
import { computerTarget } from "@/features/application/model/connections"
import type { AgentInfo } from "@/features/agents/model/agents"
import { StatusBar } from "./status-bar-popover"
import type { StatusBarActions } from "./status-bar-types"

const agent = (id: AgentInfo["id"], state: AgentInfo["state"], version?: string): AgentInfo => ({
  id, name: id === "codex" ? "Codex" : "Claude Code", state, version, expectedVersion: id === "codex" ? "0.167.0" : "2.1.287",
})

beforeEach(() => clearComputerAgents())

function setup(actions: Partial<StatusBarActions>) {
  const source = { ...applicationSourceForScenario("complete"), activities: [] }
  const running = source.computers.find(computer => computer.state === "running" && !computer.device)!
  const all: StatusBarActions = {
    listComputerDirectory: fixtureDirectoryLoader(source.computers),
    openSilo: vi.fn(), quit: vi.fn(), refresh: vi.fn(), pushRepository: vi.fn(), dismissRepositoryPush: vi.fn(),
    startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn(),
    openTerminal: vi.fn(), openEditor: vi.fn(), openSite: vi.fn(), ...actions,
  }
  return { actions: all, running, user: userEvent.setup(), ...render(<StatusBar source={source} actions={all} defaultOpen />) }
}

it("puts Open with the agent on the computer's row, and closes the panel as the terminal does", async () => {
  const openAgent = vi.fn()
  const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "ready", "0.167.0")])
  const { user, running } = setup({ openAgent, listAgents })
  const name = running.configuration.name
  const row = (await screen.findByRole("list", { name: "Computers" })).querySelector<HTMLElement>(`[aria-label="${name}"]`)!
  // One button on the row, for the agent the person set up first; the menu lists each installed agent.
  await user.click(await within(row).findByRole("button", { name: `Open Claude Code in ${name}` }))
  expect(openAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(running), "claude-code")
  expect(within(row).queryByRole("button", { name: `Open Codex in ${name}` })).not.toBeInTheDocument()
})

it("lists each installed agent in the computer's menu", async () => {
  const openAgent = vi.fn()
  const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "ready", "0.167.0")])
  const { user, running } = setup({ openAgent, listAgents })
  const name = running.configuration.name
  await screen.findByRole("button", { name: `Open Claude Code in ${name}` })
  await user.click(screen.getByRole("button", { name: `Actions for ${name}` }))
  await user.click(await screen.findByRole("menuitem", { name: "Open Codex" }))
  expect(openAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(running), "codex")
})

it("shows no agent action for an agent that is not installed", async () => {
  const listAgents = vi.fn(async () => [agent("claude-code", "missing"), agent("codex", "unknown")])
  const { running } = setup({ openAgent: vi.fn(), listAgents })
  await screen.findByRole("list", { name: "Computers" })
  await vi.waitFor(() => expect(listAgents).toHaveBeenCalled())
  expect(screen.queryByRole("button", { name: new RegExp(`^Open (Claude Code|Codex) in ${running.configuration.name}`) })).not.toBeInTheDocument()
})
