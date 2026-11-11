import { act, render, renderHook, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import { clearComputerAgents } from "@/features/agents/model/use-computer-agents"
import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"
import { OverviewPage } from "@/features/application/pages/overview-page"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createNetworkFixture, type NetworkFixtureMode } from "@/fixtures/network-policy"
import { useNetworkPolicy } from "./model/network-policy"

beforeEach(() => clearComputerAgents())

function linuxSource(state: "running" | "stopped" = "running") {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  source.runtimeRepair = null
  source.computerConfigurationOperation = null
  const computer = source.computers.find(item => !item.device)!
  computer.state = state
  computer.freshness = "fresh"
  return { source, computer }
}

/** Opens the computer's page with the network fixture in `mode`, and the actions the test wants to watch. */
async function openDetail(mode: NetworkFixtureMode, extra: Partial<ApplicationActions> = {}, state: "running" | "stopped" = "running") {
  const { source, computer } = linuxSource(state)
  const fixture = createNetworkFixture({ mode })
  fixture.setComputers(source.computers)
  const mocks = {
    readNetworkPolicy: vi.fn(fixture.readNetworkPolicy),
    saveNetworkPolicy: vi.fn(fixture.saveNetworkPolicy),
    readComputerTools: vi.fn(fixture.readComputerTools),
    setComputerTools: vi.fn(fixture.setComputerTools),
  }
  const actions = { ...mocks, ...extra } as ApplicationActions
  const user = userEvent.setup()
  render(<OverviewPage source={source as ApplicationSource} actions={actions} onConfigurationsChange={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  return { user, computer, name: computer.configuration.name, target: computerTarget(computer), fixture: mocks }
}

const network = () => screen.findByRole("region", { name: "Network" })
const tools = () => screen.findByRole("region", { name: "Silo's tools" })

it("shows the three modes with one sentence each and the published ports below", async () => {
  await openDetail("open")
  const section = await network()
  const modes = within(section).getByRole("radiogroup", { name: "Network mode" })
  expect(within(modes).getByRole("radio", { name: /Open/ })).toBeChecked()
  expect(modes).toHaveTextContent("Can reach any public address.")
  expect(modes).toHaveTextContent("Only the hosts you list.")
  expect(modes).toHaveTextContent("Nothing out.")
  expect(within(section).queryByRole("region", { name: "Allowed hosts" })).not.toBeInTheDocument()
  const headings = screen.getAllByRole("heading").map(heading => heading.textContent)
  expect(headings.indexOf("Network")).toBeLessThan(headings.indexOf("Ports"))
})

it("shows a closed network as selected and nothing else to set", async () => {
  await openDetail("closed")
  const section = await network()
  await waitFor(() => expect(within(section).getByRole("radio", { name: /Closed/ })).toBeChecked())
  expect(within(section).queryByRole("button", { name: "Save" })).not.toBeInTheDocument()
})

it("lists an allowlist's hosts and presets and what Silo adds on its own, apart and read-only", async () => {
  const { user } = await openDetail("allowlist")
  const section = await network()
  const hosts = await within(section).findByRole("region", { name: "Allowed hosts" })
  expect(within(hosts).getByText("example.com")).toBeVisible()
  expect(within(hosts).getByText("*.internal.example.org")).toBeVisible()
  expect(within(section).getByRole("checkbox", { name: "npm packages" })).toBeChecked()
  expect(within(section).getByRole("checkbox", { name: "Python packages" })).toBeChecked()
  expect(within(section).getByRole("checkbox", { name: "Anthropic API" })).not.toBeChecked()
  expect(within(section).queryByText("files.pythonhosted.org")).not.toBeInTheDocument()
  await user.click(within(section).getByRole("button", { name: "Show the hosts of Python packages" }))
  expect(within(section).getByText("pypi.org, files.pythonhosted.org")).toBeVisible()
  const added = within(section).getByRole("region", { name: "Added by Silo" })
  expect(added).toHaveTextContent("github.com")
  expect(added).toHaveTextContent("For GitHub access")
  expect(added).toHaveTextContent("api.stripe.com")
  expect(added).toHaveTextContent("For the secret stripe-key")
  expect(within(added).queryByRole("button")).not.toBeInTheDocument()
  expect(within(added).queryByText("example.com")).not.toBeInTheDocument()
})

it("shows the engine's notices", async () => {
  await openDetail("allowlist")
  const section = await network()
  expect(await within(section).findByText(/The secret stripe-key allows \*\.stripe\.com, which is not used as a network rule/)).toBeVisible()
})

it("saves a new mode with the policy the person was looking at, and says a running computer needs a restart", async () => {
  const { user, target, fixture } = await openDetail("open")
  const save = fixture.saveNetworkPolicy
  const section = await network()
  await user.click(within(section).getByRole("radio", { name: /Closed/ }))
  await user.click(within(section).getByRole("button", { name: "Save" }))
  await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith(target, { mode: "closed", hosts: [], presets: [] }, { mode: "open", hosts: [], presets: [] }))
  expect(await within(section).findByText(/This applies when the computer restarts/)).toBeVisible()
  expect(within(section).getByRole("button", { name: /^Restart .* to apply the network change$/ })).toBeVisible()
  expect(within(section).queryByRole("button", { name: "Save" })).not.toBeInTheDocument()
})

it("says a stopped computer takes the change when it next starts, and offers no restart", async () => {
  const { user } = await openDetail("open", {}, "stopped")
  const section = await network()
  await user.click(await within(section).findByRole("radio", { name: /Closed/ }))
  await user.click(within(section).getByRole("button", { name: "Save" }))
  expect(await within(section).findByText("This applies when the computer next starts.")).toBeVisible()
  expect(within(section).queryByRole("button", { name: /Restart/ })).not.toBeInTheDocument()
})

it("adds and removes hosts and ticks presets, and sends the whole list", async () => {
  const { user, target, fixture } = await openDetail("allowlist")
  const save = fixture.saveNetworkPolicy
  const section = await network()
  const hosts = await within(section).findByRole("region", { name: "Allowed hosts" })
  await user.type(within(hosts).getByLabelText("Hosts you allow"), "api.example.net")
  await user.click(within(hosts).getByRole("button", { name: "Add" }))
  await user.click(within(hosts).getByRole("button", { name: "Remove example.com" }))
  await user.click(within(section).getByRole("checkbox", { name: "Anthropic API" }))
  await user.click(within(section).getByRole("checkbox", { name: "npm packages" }))
  await user.click(within(section).getByRole("button", { name: "Save" }))
  await waitFor(() => expect(save).toHaveBeenCalledOnce())
  expect(save.mock.calls[0]![0]).toBe(target)
  expect(save.mock.calls[0]![1]).toEqual({ mode: "allowlist", hosts: ["*.internal.example.org", "api.example.net"], presets: ["pypi", "anthropic"] })
  expect(save.mock.calls[0]![2]).toEqual({ mode: "allowlist", hosts: ["example.com", "*.internal.example.org"], presets: ["npm", "pypi"] })
})

it("lowercases added hosts and rejects invalid hosts at add time", async () => {
  const { user } = await openDetail("allowlist")
  const section = await network()
  const hosts = await within(section).findByRole("region", { name: "Allowed hosts" })
  const input = within(hosts).getByLabelText("Hosts you allow")
  const addButton = within(hosts).getByRole("button", { name: "Add" })

  await user.type(input, "Bad Host:8080")
  await user.click(addButton)
  expect(await within(hosts).findByRole("alert")).toHaveTextContent("give a name without a port; port 443 is allowed")
  expect(within(hosts).queryByText("Bad Host:8080")).not.toBeInTheDocument()

  await user.clear(input)
  await user.type(input, "*.co.uk")
  await user.click(addButton)
  expect(await within(hosts).findByRole("alert")).toHaveTextContent("a name under which anyone can get a host would allow all of them")

  await user.clear(input)
  await user.type(input, "NEW-HOST.EXAMPLE.COM")
  await user.click(addButton)
  expect(within(hosts).queryByRole("alert")).not.toBeInTheDocument()
  expect(within(hosts).getByText("new-host.example.com")).toBeVisible()
})

it("shows a preset update with its hosts and accepts it by saving the policy again", async () => {
  const { user, target, fixture } = await openDetail("update")
  const save = fixture.saveNetworkPolicy
  const section = await network()
  const updates = await within(section).findByRole("region", { name: "Preset updates" })
  expect(updates).toHaveTextContent("npm packages: This preset now includes 2 more hosts")
  expect(updates).toHaveTextContent("registry.npmmirror.com, npm.pkg.example")
  await user.click(within(updates).getByRole("button", { name: "Accept the update to npm packages" }))
  const policy = { mode: "allowlist", hosts: ["example.com", "*.internal.example.org"], presets: ["npm", "pypi"] }
  await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith(target, policy, policy, { acceptPresets: true }))
})

it("will not accept an update while there are unsaved edits", async () => {
  const { user } = await openDetail("update")
  const section = await network()
  const updates = await within(section).findByRole("region", { name: "Preset updates" })
  await user.click(within(section).getByRole("checkbox", { name: "Anthropic API" }))
  expect(within(updates).getByRole("button", { name: "Accept the update to npm packages" })).toBeDisabled()
  expect(updates).toHaveTextContent("Save or discard your changes first.")
})

it("tells the person the policy changed meanwhile and reads it again instead of saving over it", async () => {
  const { user, fixture } = await openDetail("stale-save")
  const read = fixture.readNetworkPolicy
  const section = await network()
  await user.click(await within(section).findByRole("radio", { name: /Closed/ }))
  await user.click(within(section).getByRole("button", { name: "Save" }))
  expect(await within(section).findByRole("alert")).toHaveTextContent(/changed while this was being decided\. Read it again and retry\./)
  const reads = read.mock.calls.length
  await user.click(within(section).getByRole("button", { name: "Read again" }))
  await waitFor(() => expect(read.mock.calls.length).toBe(reads + 1))
  await waitFor(() => expect(within(section).getByRole("radio", { name: /Allowlist/ })).toBeChecked())
  expect(within(section).queryByRole("alert")).not.toBeInTheDocument()
})

it("keeps the baseline the draft started from and shows read again when re-read changes policy under dirty draft", async () => {
  let policy: { mode: "open" | "allowlist" | "closed"; hosts: string[]; presets: string[] } = { mode: "open", hosts: [], presets: [] }
  const state = () => ({ computerId: "dev", policy, allowedHosts: [], presets: [], presetUpdates: [], timing: "applied" as const, notices: [] })
  const read = vi.fn(async () => state())
  const save = vi.fn(async () => state())
  const { result, rerender } = renderHook(
    ({ refreshKey }) => useNetworkPolicy({ read, save, target: "dev", enabled: true, refreshKey }),
    { initialProps: { refreshKey: "1" } }
  )
  await waitFor(() => expect(result.current.state?.policy.mode).toBe("open"))

  act(() => result.current.setDraft({ mode: "closed", hosts: [], presets: [] }))
  expect(result.current.dirty).toBe(true)

  policy = { mode: "allowlist", hosts: ["added.com"], presets: [] }
  rerender({ refreshKey: "2" })
  await waitFor(() => expect(result.current.isStale).toBe(true))
  expect(result.current.saveError).toMatch(/changed/i)

  await act(async () => { await result.current.submit(result.current.draft!) })
  expect(save).toHaveBeenCalledWith("dev", { mode: "closed", hosts: [], presets: [] }, { mode: "open", hosts: [], presets: [] })
})

it("cannot be edited in a read-only Silo", async () => {
  const { source, computer } = linuxSource()
  const fixture = createNetworkFixture({ mode: "allowlist" })
  fixture.setComputers(source.computers)
  const user = userEvent.setup()
  render(<OverviewPage readOnly source={source as ApplicationSource} actions={fixture as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  const section = await network()
  expect(await within(section).findByRole("radio", { name: /Open/ })).toBeDisabled()
})

it("reports a policy it cannot read, and reads it again on Retry", async () => {
  const read = vi.fn().mockRejectedValueOnce(new Error("The engine is not answering.")).mockResolvedValue((await createNetworkFixture({ mode: "open" }).readNetworkPolicy("main")))
  const { user } = await openDetail("open", { readNetworkPolicy: read })
  const section = await network()
  expect(await within(section).findByRole("alert")).toHaveTextContent("The engine is not answering.")
  await user.click(within(section).getByRole("button", { name: "Retry" }))
  expect(await within(section).findByRole("radio", { name: /Open/ })).toBeChecked()
})

it("shows Silo's tools off, and turns them on for a stopped computer at its next start", async () => {
  const { user, target, fixture } = await openDetail("tools-off", {}, "stopped")
  const set = fixture.setComputerTools
  const section = await tools()
  expect(await within(section).findByText("Off")).toBeVisible()
  expect(section).toHaveTextContent("When on, the agent inside this computer can use the silo command and its MCP server.")
  await user.click(within(section).getByRole("switch"))
  await waitFor(() => expect(set).toHaveBeenCalledExactlyOnceWith(target, true))
  expect(await within(section).findByText("On")).toBeVisible()
  expect(section).toHaveTextContent("Takes effect at the next start.")
})

it("shows Silo's tools on and turns them off", async () => {
  const { user, target, fixture } = await openDetail("tools-on")
  const set = fixture.setComputerTools
  const section = await tools()
  expect(await within(section).findByText("On")).toBeVisible()
  expect(section).not.toHaveTextContent(/Restart needed|Takes effect/)
  await user.click(within(section).getByRole("switch"))
  await waitFor(() => expect(set).toHaveBeenCalledExactlyOnceWith(target, false))
  expect(await within(section).findByText("Off")).toBeVisible()
})

it("says a restart is needed, with a Restart button, when the engine says so", async () => {
  await openDetail("tools-restart")
  const section = await tools()
  expect(await within(section).findByText("Restart needed.")).toBeVisible()
  expect(within(section).getByRole("button", { name: /^Restart .* for Silo's tools$/ })).toBeVisible()
})

it("says in plain words which piece of the tools failed, and shows no port or token", async () => {
  await openDetail("tools-failed")
  const section = await tools()
  const problems = await within(section).findByRole("status", { name: "Problems with Silo's tools" })
  expect(problems).toHaveTextContent("Could not set up the MCP server for your agents. Claude Code could not be reached.")
  expect(problems).not.toHaveTextContent("silo command")
  expect(section).not.toHaveTextContent(/port|token/i)
})

it("shows a failure to turn the tools on and leaves the switch as it was", async () => {
  const set = vi.fn().mockRejectedValue(new Error("The computer is not reachable."))
  const { user } = await openDetail("tools-off", { setComputerTools: set })
  const section = await tools()
  await user.click(await within(section).findByRole("switch"))
  expect(await within(section).findByRole("alert")).toHaveTextContent("The computer is not reachable.")
  expect(within(section).getByRole("switch")).not.toBeChecked()
})
