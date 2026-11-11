import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import type { ApplicationActions } from "@/features/application/model/application-source"

import type { AgentInfo } from "../model/agents"
import { clearComputerAgents, useComputerAgents } from "../model/use-computer-agents"
import { AgentsSection } from "./agents-section"

const agent = (id: AgentInfo["id"], state: AgentInfo["state"], version?: string): AgentInfo => ({
  id, name: id === "codex" ? "Codex" : "Claude Code", state, version, expectedVersion: id === "codex" ? "0.167.0" : "2.1.287",
})

function Harness({ listAgents, running = true, actions }: { listAgents: NonNullable<ApplicationActions["listAgents"]>; running?: boolean; actions: Pick<ApplicationActions, "installAgent" | "ensureSshName"> }) {
  const state = useComputerAgents(listAgents, "main", true)
  return <TooltipProvider><AgentsSection target="main" computerName="main" running={running} agents={state.agents} error={state.error} loading={state.loading} onReload={() => void state.reload()} actions={actions} /></TooltipProvider>
}

beforeEach(() => clearComputerAgents())

describe("Agents section", () => {
  it("shows each agent in its state with the button that applies", async () => {
    const listAgents = vi.fn(async () => [agent("claude-code", "outdated", "2.0.0"), agent("codex", "missing")])
    render(<Harness listAgents={listAgents} actions={{ installAgent: vi.fn() }} />)
    const claude = await screen.findByRole("group", { name: "Claude Code" })
    expect(claude).toHaveTextContent("Update available · 2.0.0 installed, 2.1.287 ready to install")
    expect(within(claude).getByRole("button", { name: "Update Claude Code in main" })).toBeEnabled()
    const codex = screen.getByRole("group", { name: "Codex" })
    expect(codex).toHaveTextContent("Not installed")
    expect(within(codex).getByRole("button", { name: "Install Codex in main" })).toBeEnabled()
  })

  it("shows a ready agent with its version and no button, and one being installed without a button", async () => {
    const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "installing")])
    render(<Harness listAgents={listAgents} actions={{ installAgent: vi.fn() }} />)
    const claude = await screen.findByRole("group", { name: "Claude Code" })
    expect(claude).toHaveTextContent("Ready · 2.1.287")
    expect(within(claude).queryByRole("button")).not.toBeInTheDocument()
    const codex = screen.getByRole("group", { name: "Codex" })
    expect(codex).toHaveTextContent("Installing…")
    expect(within(codex).queryByRole("button")).not.toBeInTheDocument()
  })

  it("says the state is unknown while the computer is stopped and offers no install", async () => {
    const listAgents = vi.fn(async () => [agent("claude-code", "unknown"), agent("codex", "unknown")])
    render(<Harness listAgents={listAgents} running={false} actions={{ installAgent: vi.fn() }} />)
    const claude = await screen.findByRole("group", { name: "Claude Code" })
    expect(claude).toHaveTextContent("Start the computer to see this")
    expect(within(claude).queryByRole("button")).not.toBeInTheDocument()
  })

  it("installs an agent showing the job's progress, then shows it ready", async () => {
    const user = userEvent.setup()
    let progress!: (value: Parameters<NonNullable<ApplicationActions["installAgent"]>>[2] extends ((value: infer P) => void) | undefined ? P : never) => void
    let finish!: (agents: AgentInfo[]) => void
    const installAgent = vi.fn<NonNullable<ApplicationActions["installAgent"]>>((_target, _agent, onProgress) => new Promise<AgentInfo[]>(resolve => { progress = onProgress!; finish = resolve }))
    const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "missing")])
    render(<Harness listAgents={listAgents} actions={{ installAgent }} />)
    await user.click(await screen.findByRole("button", { name: "Install Codex in main" }))
    expect(installAgent).toHaveBeenCalledExactlyOnceWith("main", "codex", expect.any(Function))
    const codex = screen.getByRole("group", { name: "Codex" })
    expect(codex).toHaveTextContent("Waiting to start…")
    expect(within(codex).queryByRole("button")).not.toBeInTheDocument()
    act(() => progress({ computerId: "main-1", agent: "codex", step: "downloading", detail: "Downloading Codex (42%)" }))
    expect(codex).toHaveTextContent("Downloading Codex (42%)")
    act(() => progress({ computerId: "main-1", agent: "codex", step: "registering", detail: undefined }))
    expect(codex).toHaveTextContent("Setting up Silo's tools…")
    await act(async () => finish([agent("claude-code", "ready", "2.1.287"), agent("codex", "ready", "0.167.0")]))
    expect(screen.getByRole("group", { name: "Codex" })).toHaveTextContent("Ready · 0.167.0")
    expect(screen.queryByRole("button", { name: /Install Codex/ })).not.toBeInTheDocument()
  })

  it("ends a failed install in a plain error and lets it be tried again", async () => {
    const user = userEvent.setup()
    const installAgent = vi.fn<NonNullable<ApplicationActions["installAgent"]>>()
      .mockRejectedValueOnce(new Error("The download could not be verified."))
      .mockResolvedValueOnce([agent("claude-code", "ready", "2.1.287"), agent("codex", "ready", "0.167.0")])
    const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "missing")])
    render(<Harness listAgents={listAgents} actions={{ installAgent }} />)
    await user.click(await screen.findByRole("button", { name: "Install Codex in main" }))
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not install Codex: The download could not be verified.")
    expect(screen.queryByRole("button", { name: "Install Codex in main" })).not.toBeInTheDocument()
    const retry = screen.getByRole("button", { name: "Retry installing Codex in main" })
    expect(retry).toHaveTextContent("Retry")
    await user.click(retry)
    await waitFor(() => expect(screen.getByRole("group", { name: "Codex" })).toHaveTextContent("Ready · 0.167.0"))
    expect(installAgent).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })

  it("explains why it cannot install while the computer is stopped", async () => {
    const listAgents = vi.fn(async () => [agent("claude-code", "missing"), agent("codex", "missing")])
    render(<Harness listAgents={listAgents} running={false} actions={{ installAgent: vi.fn() }} />)
    expect(await screen.findByRole("button", { name: "Install Codex in main" })).toBeDisabled()
    expect(screen.getAllByRole("group", { name: "Start the computer to install an agent." }).length).toBeGreaterThan(0)
  })

  it("reports a list that could not be read, and reads it again on Retry", async () => {
    const user = userEvent.setup()
    const listAgents = vi.fn<NonNullable<ApplicationActions["listAgents"]>>()
      .mockRejectedValueOnce(new Error("main did not answer."))
      .mockResolvedValueOnce([agent("claude-code", "ready", "2.1.287"), agent("codex", "missing")])
    render(<Harness listAgents={listAgents} actions={{ installAgent: vi.fn() }} />)
    expect(await screen.findByRole("alert")).toHaveTextContent("main did not answer.")
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(await screen.findByRole("group", { name: "Claude Code" })).toHaveTextContent("Ready")
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })

  describe("SSH name", () => {
    const listAgents = vi.fn(async () => [agent("claude-code", "ready", "2.1.287"), agent("codex", "missing")])

    it("makes the host name when asked and offers to copy it", async () => {
      const user = userEvent.setup()
      const ensureSshName = vi.fn(async () => ({ alias: "silo-6f0c1a52", includeInstalled: true, includeLine: "" }))
      render(<Harness listAgents={listAgents} actions={{ installAgent: vi.fn(), ensureSshName }} />)
      const line = await screen.findByRole("group", { name: "SSH name for desktop apps" })
      // Nothing is written to the SSH configuration until it is asked for.
      expect(ensureSshName).not.toHaveBeenCalled()
      await user.click(within(line).getByRole("button", { name: "Show SSH name" }))
      expect(ensureSshName).toHaveBeenCalledExactlyOnceWith("main")
      expect(await within(line).findByText("silo-6f0c1a52")).toBeVisible()
      expect(line).toHaveTextContent("Silo added it to your SSH configuration.")
      expect(within(line).getByRole("button", { name: "Copy SSH name" })).toBeVisible()
    })

    it("says plainly what went wrong and offers to try again", async () => {
      const user = userEvent.setup()
      const ensureSshName = vi.fn().mockRejectedValueOnce(new Error("Silo could not change ~/.ssh/config.")).mockResolvedValueOnce({ alias: "silo-x", includeInstalled: true, includeLine: "" })
      render(<Harness listAgents={listAgents} actions={{ installAgent: vi.fn(), ensureSshName }} />)
      const line = await screen.findByRole("group", { name: "SSH name for desktop apps" })
      await user.click(within(line).getByRole("button", { name: "Show SSH name" }))
      expect(await within(line).findByText("Silo could not change ~/.ssh/config.")).toBeVisible()
      await user.click(within(line).getByRole("button", { name: "Try again" }))
      expect(await within(line).findByText("silo-x")).toBeVisible()
    })

    it("shows no name for a computer that is not running", async () => {
      render(<Harness listAgents={listAgents} running={false} actions={{ installAgent: vi.fn(), ensureSshName: vi.fn() }} />)
      const line = await screen.findByRole("group", { name: "SSH name for desktop apps" })
      expect(within(line).getByRole("button", { name: "Show SSH name" })).toBeDisabled()
    })

    it("has no line where the app cannot make a name", async () => {
      render(<Harness listAgents={listAgents} actions={{ installAgent: vi.fn() }} />)
      await screen.findByRole("group", { name: "Claude Code" })
      expect(screen.queryByRole("group", { name: "SSH name for desktop apps" })).not.toBeInTheDocument()
    })
  })
})
