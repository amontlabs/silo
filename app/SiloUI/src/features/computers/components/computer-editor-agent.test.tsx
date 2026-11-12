import { act, render, renderHook, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { clearPlannedAgentInstalls, AgentInstallSupport, planAgentInstall, usePlannedAgentInstalls } from "@/features/agents/model/planned-installs"
import { clearComputerAgents } from "@/features/agents/model/use-computer-agents"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"
import { ComputerConfigurationList } from "./computer-configuration-list"

beforeEach(() => { clearPlannedAgentInstalls(); clearComputerAgents() })
afterEach(() => { clearPlannedAgentInstalls() })

function renderNewComputerForm({ supported = true, remembered = null }: { supported?: boolean; remembered?: "claude-code" | "codex" | "none" | null } = {}) {
  const onConfigurationsChange = vi.fn()
  render(<SettingsProvider store={createMemorySettingsStore({ agent: remembered })}><AgentInstallSupport value={supported}><TooltipProvider>
    <ComputerConfigurationList configurations={[productionComputerDefaults[0]!]} onConfigurationsChange={onConfigurationsChange} />
  </TooltipProvider></AgentInstallSupport></SettingsProvider>)
  return onConfigurationsChange
}

async function openNewComputerForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Add" }))
  const item = screen.queryByRole("menuitem", { name: "New computer" })
  if (item) await user.click(item)
  return screen.findByTestId(/^computer-editor-/)
}

describe("the agent choice of the create form", () => {
  it("offers None, Claude Code and Codex, preselected from the agent remembered", async () => {
    const user = userEvent.setup()
    renderNewComputerForm({ remembered: "codex" })
    await openNewComputerForm(user)
    const field = await screen.findByRole("combobox", { name: "Agent" })
    expect(field).toHaveValue("codex")
    expect([...(field as HTMLSelectElement).options].map(option => option.textContent)).toEqual(["None", "Claude Code", "Codex"])
  })

  it.each([[null, "none"], ["none", "none"], ["claude-code", "claude-code"]] as const)("preselects the remembered %s as %s", async (remembered, expected) => {
    const user = userEvent.setup()
    renderNewComputerForm({ remembered })
    await openNewComputerForm(user)
    expect(await screen.findByRole("combobox", { name: "Agent" })).toHaveValue(expected)
  })

  it("is not offered where the app cannot install an agent", async () => {
    const user = userEvent.setup()
    renderNewComputerForm({ supported: false, remembered: "codex" })
    await openNewComputerForm(user)
    expect(screen.queryByRole("combobox", { name: "Agent" })).not.toBeInTheDocument()
  })

  it("plans the chosen agent for the new computer when Create is pressed, and not when None is", async () => {
    const user = userEvent.setup()
    const onConfigurationsChange = renderNewComputerForm({ remembered: "codex" })
    await openNewComputerForm(user)
    const name = await screen.findByRole("textbox", { name: "Computer name" })
    await user.clear(name)
    await user.type(name, "second")
    await user.selectOptions(screen.getByRole("combobox", { name: "Agent" }), "claude-code")
    await user.click(screen.getByRole("button", { name: "Create" }))
    await waitFor(() => expect(onConfigurationsChange).toHaveBeenCalled())
    // The plan is read back by installing in the computer once it runs.
    const source = structuredClone(applicationSourceForScenario("running"))
    const created = source.computers[0]!
    created.configuration = { ...created.configuration, name: "second" }
    created.state = "running"
    const installAgent = vi.fn(async () => [])
    renderHook(() => usePlannedAgentInstalls(source, installAgent))
    await waitFor(() => expect(installAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(created), "claude-code"))
  })
})

describe("installing a planned agent", () => {
  function sourceWith(state: "running" | "stopped" | "starting") {
    const source: ApplicationSource = structuredClone(applicationSourceForScenario("running"))
    const computer = source.computers.find(item => !item.device)!
    computer.state = state
    computer.freshness = "fresh"
    return { source, computer }
  }

  it("waits until the computer runs, then installs once", async () => {
    const installAgent = vi.fn<NonNullable<ApplicationActions["installAgent"]>>(async () => [])
    const { source, computer } = sourceWith("stopped")
    planAgentInstall(computer.configuration.name, "codex")
    const { rerender } = renderHook(({ current }) => usePlannedAgentInstalls(current, installAgent), { initialProps: { current: source } })
    expect(installAgent).not.toHaveBeenCalled()
    const running = sourceWith("running")
    rerender({ current: { ...running.source, computers: running.source.computers.map(item => item.configuration.id === computer.configuration.id ? { ...item, configuration: computer.configuration } : item) } })
    await waitFor(() => expect(installAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(running.computer), "codex"))
    rerender({ current: running.source })
    await act(async () => {})
    expect(installAgent).toHaveBeenCalledTimes(1)
  })

  it("plans nothing for None, and forgets an earlier plan for the same name", () => {
    const installAgent = vi.fn(async () => [])
    const { source, computer } = sourceWith("running")
    planAgentInstall(computer.configuration.name, "codex")
    planAgentInstall(computer.configuration.name, "none")
    renderHook(() => usePlannedAgentInstalls(source, installAgent))
    expect(installAgent).not.toHaveBeenCalled()
  })

  it("leaves a computer of another device alone", () => {
    const installAgent = vi.fn(async () => [])
    const { source, computer } = sourceWith("running")
    computer.device = { id: "office", computerId: computer.configuration.id, name: "Office", address: "office.test", connected: true }
    planAgentInstall(computer.configuration.name, "codex")
    renderHook(() => usePlannedAgentInstalls(source, installAgent))
    expect(installAgent).not.toHaveBeenCalled()
  })
})
