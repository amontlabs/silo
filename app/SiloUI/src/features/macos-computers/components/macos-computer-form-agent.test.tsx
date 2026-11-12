import { render, renderHook, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import { AgentInstallSupport, clearPlannedAgentInstalls, usePlannedAgentInstalls } from "@/features/agents/model/planned-installs"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import { computerTarget } from "@/features/application/model/connections"
import { defaultMacosFields } from "../model/macos-computers"
import { MacosComputerForm } from "./macos-computer-form"

beforeEach(() => clearPlannedAgentInstalls())

function renderForm(remembered: "claude-code" | "codex" | "none" | null, onCreate = vi.fn()) {
  const fields = { ...defaultMacosFields(), name: "builder" }
  render(<SettingsProvider store={createMemorySettingsStore({ agent: remembered })}><AgentInstallSupport value>
    <MacosComputerForm fields={fields} onChange={vi.fn()} creating={false} existingNames={[]} otherNames={[]} osField={null} onCancel={vi.fn()} onCreate={onCreate} />
  </AgentInstallSupport></SettingsProvider>)
  return onCreate
}

it("preselects the remembered agent and installs it after setup, when the macOS computer runs", async () => {
  const user = userEvent.setup()
  const onCreate = renderForm("claude-code")
  expect(screen.getByRole("combobox", { name: "Agent" })).toHaveValue("claude-code")
  await user.click(screen.getByRole("button", { name: "Create" }))
  expect(onCreate).toHaveBeenCalledOnce()

  // A macOS computer is stopped when its setup ends; the install waits for it to run.
  const source = structuredClone(applicationSourceForScenario("running"))
  const computer = source.computers[0]!
  computer.configuration = { ...computer.configuration, name: "builder" }
  computer.macos = { setupComplete: false } as never
  computer.state = "running"
  const installAgent = vi.fn(async () => [])
  const { rerender } = renderHook(({ current }) => usePlannedAgentInstalls(current, installAgent), { initialProps: { current: source } })
  expect(installAgent).not.toHaveBeenCalled()
  const ready = structuredClone(source)
  ready.computers[0]!.macos = { setupComplete: true } as never
  rerender({ current: ready })
  await waitFor(() => expect(installAgent).toHaveBeenCalledExactlyOnceWith(computerTarget(ready.computers[0]!), "claude-code"))
})

it("plans nothing when None is chosen", async () => {
  const user = userEvent.setup()
  renderForm("codex")
  await user.selectOptions(screen.getByRole("combobox", { name: "Agent" }), "none")
  await user.click(screen.getByRole("button", { name: "Create" }))
  const source = structuredClone(applicationSourceForScenario("running"))
  source.computers[0]!.configuration = { ...source.computers[0]!.configuration, name: "builder" }
  const installAgent = vi.fn(async () => [])
  renderHook(() => usePlannedAgentInstalls(source, installAgent))
  expect(installAgent).not.toHaveBeenCalled()
})
