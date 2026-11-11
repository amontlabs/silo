import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createFixtureMacosComputersStore } from "@/fixtures/macos-computers"
import { MacosComputersContext, type MacosComputersStore } from "@/features/macos-computers/model/macos-computers"
import { clearComputerAgents } from "@/features/agents/model/use-computer-agents"
import type { AgentInfo } from "@/features/agents/model/agents"
import { useMacosApplication } from "../model/macos-application-computers"
import type { ApplicationActions, ApplicationSource } from "../model/application-source"
import { computerTarget } from "../model/connections"
import { OverviewPage } from "./overview-page"

const agent = (id: AgentInfo["id"], state: AgentInfo["state"], version?: string): AgentInfo => ({
  id, name: id === "codex" ? "Codex" : "Claude Code", state, version, expectedVersion: id === "codex" ? "0.167.0" : "2.1.287",
})

beforeEach(() => clearComputerAgents())

function linuxSource() {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  source.runtimeRepair = null
  source.computerConfigurationOperation = null
  const computer = source.computers.find(item => !item.device)!
  computer.state = "running"
  computer.freshness = "fresh"
  return { source, computer }
}

async function openDetail(source: ApplicationSource, name: string, actions: Partial<ApplicationActions>) {
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={actions as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: `Open ${name}` }))
  return user
}

it("opens the installed agent from the page header, once per installed agent, and shows the Agents section", async () => {
  const { source, computer } = linuxSource()
  const name = computer.configuration.name
  const openAgent = vi.fn()
  const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "ready", "0.167.0")])
  const user = await openDetail(source, name, { openAgent, listAgents, installAgent: vi.fn(), openTerminal: vi.fn() })
  expect(listAgents).toHaveBeenCalledWith(computerTarget(computer))
  await user.click(await screen.findByRole("button", { name: `Open Claude Code in ${name}` }))
  expect(openAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(computer), "claude-code")
  await user.click(screen.getByRole("button", { name: `Open Codex in ${name}` }))
  expect(openAgent).toHaveBeenLastCalledWith(computerTarget(computer), "codex")
  expect(await screen.findByRole("group", { name: "Claude Code" })).toHaveTextContent("Ready · 2.1.287")
  expect(screen.getByRole("group", { name: "Codex" })).toHaveTextContent("Ready · 0.167.0")
})

it("offers Open only for the agents that are installed", async () => {
  const { source, computer } = linuxSource()
  const name = computer.configuration.name
  const listAgents = vi.fn(async () => [agent("claude-code", "missing"), agent("codex", "ready", "0.167.0")])
  await openDetail(source, name, { openAgent: vi.fn(), listAgents, installAgent: vi.fn() })
  expect(await screen.findByRole("button", { name: `Open Codex in ${name}` })).toBeVisible()
  expect(screen.queryByRole("button", { name: `Open Claude Code in ${name}` })).not.toBeInTheDocument()
  expect(await screen.findByRole("button", { name: `Install Claude Code in ${name}` })).toBeEnabled()
})

it("offers no Open while the computer is stopped, where nothing is known about its agents", async () => {
  const { source, computer } = linuxSource()
  computer.state = "stopped"
  const name = computer.configuration.name
  const listAgents = vi.fn(async () => [agent("claude-code", "unknown"), agent("codex", "unknown")])
  await openDetail(source, name, { openAgent: vi.fn(), listAgents, installAgent: vi.fn() })
  expect(await screen.findByRole("group", { name: "Claude Code" })).toHaveTextContent("Start the computer to see this")
  expect(screen.queryByRole("button", { name: /^Open (Claude Code|Codex) in/ })).not.toBeInTheDocument()
})

it("puts Open with the agent in the computer's menu too", async () => {
  const { source, computer } = linuxSource()
  const name = computer.configuration.name
  const openAgent = vi.fn()
  const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "missing")])
  const user = await openDetail(source, name, { openAgent, listAgents, installAgent: vi.fn() })
  await screen.findByRole("button", { name: `Open Claude Code in ${name}` })
  await user.click(screen.getByRole("button", { name: `More actions for ${name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Open Claude Code in ${name}` }))
  expect(openAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(computer), "claude-code")
  await user.click(screen.getByRole("button", { name: `More actions for ${name}` }))
  expect(screen.queryByRole("menuitem", { name: `Open Codex in ${name}` })).not.toBeInTheDocument()
})

it("has no agent actions or section where the app cannot list agents", async () => {
  const { source, computer } = linuxSource()
  await openDetail(source, computer.configuration.name, {})
  expect(screen.queryByRole("region", { name: "Agents" })).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: /^Open (Claude Code|Codex) in/ })).not.toBeInTheDocument()
})

it("installs an agent from the section and then offers to open it", async () => {
  const { source, computer } = linuxSource()
  const name = computer.configuration.name
  const installAgent = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "ready", "0.167.0")])
  const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "missing")])
  const user = await openDetail(source, name, { openAgent: vi.fn(), listAgents, installAgent })
  await user.click(await screen.findByRole("button", { name: `Install Codex in ${name}` }))
  expect(installAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(computer), "codex", expect.any(Function))
  expect(await screen.findByRole("button", { name: `Open Codex in ${name}` })).toBeVisible()
})

function MacosPage({ source, actions }: { source: ApplicationSource; actions: ApplicationActions }) {
  const merged = useMacosApplication(source, actions)
  return <OverviewPage source={merged.source} actions={merged.actions} onConfigurationsChange={vi.fn()} />
}

it("opens the agent of a macOS computer by its name, as the terminal does", async () => {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  source.runtimeRepair = null
  source.computerConfigurationOperation = null
  const store: MacosComputersStore = createFixtureMacosComputersStore()
  const openAgent = vi.fn()
  const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "missing")])
  const actions = { openAgent, listAgents, installAgent: vi.fn(), openTerminal: vi.fn(), openEditor: vi.fn(), readWorkspaceStorage: vi.fn(), refreshSshAccess: vi.fn(async () => {}) } as unknown as ApplicationActions
  const user = userEvent.setup()
  render(<MacosComputersContext.Provider value={store}><MacosPage source={source} actions={actions} /></MacosComputersContext.Provider>)
  await user.click(await screen.findByRole("button", { name: "Open daily" }))
  await user.click(await screen.findByRole("button", { name: "Open Claude Code in daily" }))
  expect(openAgent).toHaveBeenCalledExactlyOnceWith("daily", "claude-code")
  const section = await screen.findByRole("group", { name: "Codex" })
  expect(within(section).getByRole("button", { name: "Install Codex in daily" })).toBeEnabled()
  await waitFor(() => expect(listAgents).toHaveBeenCalledWith("daily"))
})
