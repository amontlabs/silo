import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions, ApplicationSource, ApplicationComputer } from "../model/application-source"
import { computerTarget } from "../model/connections"
import { OverviewPage } from "./overview-page"

function localVmSource() {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  return source
}

function localComputer(source: ApplicationSource): ApplicationComputer {
  return source.computers.find(item => !item.device)!
}

async function openDetail(source: ApplicationSource, actions: Partial<ApplicationActions> = {}, computer = localComputer(source)) {
  const user = userEvent.setup()
  const view = render(<OverviewPage source={source} actions={actions as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  return { ...view, user, computer }
}

it.each([false, true])("shows pending secret revocation on the computer row and detail, routing Restart to its owner (remote=%s)", async remote => {
  const source = localVmSource()
  const computer = localComputer(source)
  computer.state = "running"
  computer.freshness = "fresh"
  computer.pendingSecretRevocations = ["GITHUB_TOKEN"]
  const message = "May still have access to GITHUB_TOKEN until it restarts."
  computer.attention = { level: "warning", message }
  if (remote) computer.device = { id: "office", computerId: computer.configuration.id, name: "Office", address: "office.test", connected: true }
  const restartComputer = vi.fn()
  const actions = { restartComputer } as unknown as ApplicationActions
  const user = userEvent.setup()
  const { rerender } = render(<OverviewPage source={source} actions={actions} onConfigurationsChange={vi.fn()} />)
  expect(screen.getByText(message, { exact: false })).toBeVisible()
  await user.click(screen.getByRole("button", { name: `Restart ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Restart" }))
  expect(restartComputer).toHaveBeenCalledExactlyOnceWith(computerTarget(computer))
  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  expect(screen.getByRole("note", { name: "Pending secret revocation" })).toHaveTextContent(message)
  await user.click(screen.getByRole("button", { name: `Restart ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Restart" }))
  expect(restartComputer).toHaveBeenCalledTimes(2)
  const refreshed = structuredClone(source)
  refreshed.computers.find(item => item.configuration.id === computer.configuration.id)!.pendingSecretRevocations = undefined
  refreshed.computers.find(item => item.configuration.id === computer.configuration.id)!.attention = undefined
  rerender(<OverviewPage source={refreshed} actions={actions} onConfigurationsChange={vi.fn()} />)
  expect(screen.queryByRole("note", { name: "Pending secret revocation" })).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: `Restart ${computer.configuration.name}` })).not.toBeInTheDocument()
})

it.each([false, true])("clears the overview row revocation warning and Restart action only after source refresh (remote=%s)", async remote => {
  const source = localVmSource()
  const computer = localComputer(source)
  computer.state = "running"
  computer.freshness = "fresh"
  computer.pendingSecretRevocations = ["REMOVED_TOKEN"]
  const message = "May still have access to REMOVED_TOKEN until it restarts."
  computer.attention = { level: "warning", message }
  if (remote) computer.device = { id: "office", computerId: computer.configuration.id, name: "Office", address: "office.test", connected: true }
  const restartComputer = vi.fn()
  const actions = { restartComputer } as unknown as ApplicationActions
  const user = userEvent.setup()
  const { rerender } = render(<OverviewPage source={source} actions={actions} onConfigurationsChange={vi.fn()} />)
  const row = screen.getByRole("button", { name: `Open ${computer.configuration.name}` }).closest("li")!
  expect(row).toHaveTextContent(message)
  expect(within(row).getByRole("img", { name: "warning status" })).toBeVisible()
  await user.click(within(row).getByRole("button", { name: `Restart ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Restart" }))
  expect(restartComputer).toHaveBeenCalledExactlyOnceWith(computerTarget(computer))
  expect(row).toHaveTextContent(message)
  expect(within(row).getByRole("button", { name: `Restart ${computer.configuration.name}` })).toBeVisible()

  const refreshed = structuredClone(source)
  const refreshedComputer = refreshed.computers.find(item => item.configuration.id === computer.configuration.id)!
  refreshedComputer.pendingSecretRevocations = undefined
  refreshedComputer.attention = undefined
  rerender(<OverviewPage source={refreshed} actions={actions} onConfigurationsChange={vi.fn()} />)
  expect(row).not.toHaveTextContent(message)
  expect(within(row).queryByRole("button", { name: `Restart ${computer.configuration.name}` })).not.toBeInTheDocument()
  expect(within(row).queryByRole("img", { name: "warning status" })).not.toBeInTheDocument()
})

it("opens a computer detail page from the row body and returns to the list from the breadcrumb", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ openTerminal: vi.fn() } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  // The list heading and the detail breadcrumb root share their styling so opening
  // a computer never shifts "Computers".
  const listHeading = screen.getByRole("heading", { name: "Computers" })
  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" })
  expect(breadcrumb).toHaveTextContent(`Computers${computer.configuration.name}`)
  const breadcrumbRoot = within(breadcrumb).getByRole("button", { name: "Computers" })
  expect(breadcrumbRoot.className).toContain("font-medium")
  expect(listHeading.className).toContain("font-medium")
  expect(screen.queryByRole("list", { name: "Configured computers" })).not.toBeInTheDocument()

  await user.click(breadcrumbRoot)
  expect(screen.getByRole("list", { name: "Configured computers" })).toBeVisible()
})

it("opens the Checkpoints tab directly from the row menu", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ createCheckpoint: vi.fn(), forkCheckpoint: vi.fn(), restoreCheckpoint: vi.fn() } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Checkpoints for ${computer.configuration.name}` }))
  expect(screen.getByRole("tab", { name: "Checkpoints" })).toHaveAttribute("aria-selected", "true")
  expect(screen.getByRole("region", { name: `Checkpoints for ${computer.configuration.name}` })).toBeVisible()
})

it("hides Storage and SSH access tabs for a remote computer without those capabilities", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  computer.device = { id: "office", computerId: computer.configuration.id, name: "Office", address: "office.test", connected: true }
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ readWorkspaceStorage: vi.fn(), forkCheckpoint: vi.fn() } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  expect(screen.getAllByRole("tab").map(tab => tab.textContent)).toEqual(["Overview", "Checkpoints"])
})

it("summarizes resources, repositories, and secrets on the Overview tab", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  expect(screen.getByRole("heading", { name: "Resources" })).toBeVisible()
  expect(screen.getByRole("heading", { name: "Repositories" })).toBeVisible()
  expect(screen.getByRole("heading", { name: "Secrets" })).toBeVisible()
})

it("jumps from Overview sections to Files, Network, and Secrets scoped to the computer", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const onNavigate = vi.fn()
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={vi.fn()} onNavigate={onNavigate} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))

  await user.click(screen.getByRole("button", { name: "View all files for this computer" }))
  expect(onNavigate).toHaveBeenLastCalledWith({ computerSection: "files", computer: computer.configuration.id })

  await user.click(screen.getByRole("button", { name: "View all network for this computer" }))
  expect(onNavigate).toHaveBeenLastCalledWith({ computerSection: "network", computer: computer.configuration.id })

  await user.click(screen.getByRole("button", { name: "View all secrets" }))
  expect(onNavigate).toHaveBeenLastCalledWith({ tab: "secrets" })
})

it("opens the computer editor in place on the detail page without leaving it", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${computer.configuration.name}` }))

  // The editor renders on the detail page under an "Edit <name>" label, the tabs are hidden,
  // and the computer list is never shown.
  expect(screen.getByRole("heading", { name: `Edit ${computer.configuration.name}` })).toBeVisible()
  expect(screen.getByRole("textbox", { name: "Computer name" })).toBeVisible()
  expect(screen.queryByRole("tab", { name: "Overview" })).not.toBeInTheDocument()
  expect(screen.queryByRole("list", { name: "Configured computers" })).not.toBeInTheDocument()
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(`Computers${computer.configuration.name}`)
})

it("opens the editor in place from the Resources Edit button", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Edit" }))
  expect(screen.getByRole("heading", { name: `Edit ${computer.configuration.name}` })).toBeVisible()
})

it("commits an in-place edit with a baseline and returns to the overview tab", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const onConfigurationsChange = vi.fn()
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={onConfigurationsChange} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Edit" }))
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  // The computer is running, so saving asks to stop it first.
  await user.click(screen.getByRole("button", { name: "Stop and save…" }))
  await user.click(screen.getByRole("button", { name: "Stop and save" }))

  // The commit carries a baseline (targeted change), the editor closes, and the detail page
  // stays open on the same computer rather than returning to the list.
  expect(onConfigurationsChange).toHaveBeenCalledWith(expect.any(Array), expect.any(Array))
  expect(screen.queryByRole("heading", { name: `Edit ${computer.configuration.name}` })).not.toBeInTheDocument()
  expect(screen.getByRole("heading", { name: "Resources" })).toBeVisible()
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(`Computers${computer.configuration.name}`)
  expect(screen.queryByRole("list", { name: "Configured computers" })).not.toBeInTheDocument()
})

it("cancels an in-place edit without committing", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const onConfigurationsChange = vi.fn()
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={onConfigurationsChange} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Edit" }))
  await user.click(screen.getByRole("button", { name: "Cancel" }))

  expect(onConfigurationsChange).not.toHaveBeenCalled()
  expect(screen.queryByRole("heading", { name: `Edit ${computer.configuration.name}` })).not.toBeInTheDocument()
  expect(screen.getByRole("heading", { name: "Resources" })).toBeVisible()
})

it("shows the stale-edit conflict review in place when a save is rejected", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const onConfigurationsChange = vi.fn().mockRejectedValue(new Error("This computer changed while your edit was waiting. Review it and try again."))
  // A defined saveRemoteComputer routes the commit through the awaited path, which keeps the
  // editor open on a stale rejection (the optimistic list path closes it immediately).
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ saveRemoteComputer: vi.fn() } as unknown as ApplicationActions} onConfigurationsChange={onConfigurationsChange} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: "Edit" }))
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  // The computer is running, so saving asks to stop it first.
  await user.click(screen.getByRole("button", { name: "Stop and save…" }))
  await user.click(screen.getByRole("button", { name: "Stop and save" }))

  expect(await screen.findByText("This computer changed since you opened it.")).toBeVisible()
  expect(screen.getByRole("button", { name: "Review changes" })).toBeVisible()
  // Still on the detail page, not the list.
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toBeVisible()
  expect(screen.queryByRole("list", { name: "Configured computers" })).not.toBeInTheDocument()

  await user.click(screen.getByRole("button", { name: "Review changes" }))
  expect(screen.queryByText("This computer changed since you opened it.")).not.toBeInTheDocument()
})

it("adds a secret from the Overview tab preselected to this computer", async () => {
  const source = localVmSource()
  const computer = localComputer(source)
  source.secrets = []
  const saveSecret = vi.fn().mockResolvedValue(undefined)
  const { user } = await openDetail(source, { saveSecret, removeSecret: vi.fn() })

  await user.click(screen.getByRole("button", { name: "Add secret" }))
  const form = within(screen.getByRole("form", { name: "Add secret" }))
  // The computer is preselected, so its removal chip is already present.
  expect(form.getByRole("button", { name: `Remove ${computer.configuration.name}` })).toBeVisible()
  await user.type(form.getByRole("textbox", { name: "Name" }), "SERVICE_TOKEN")
  await user.type(form.getByLabelText("Value"), "fixture-token")
  await user.type(form.getByRole("textbox", { name: "Allowed domains" }), "api.example.test")
  await user.click(form.getByRole("button", { name: "Save" }))

  expect(saveSecret).toHaveBeenCalledExactlyOnceWith({ operation: "add", name: "SERVICE_TOKEN", value: "fixture-token", computers: [computer.configuration.name], allowedDomains: ["api.example.test"] })
})

it("edits and removes a computer's secret from the Overview tab", async () => {
  const source = localVmSource()
  const computer = localComputer(source)
  source.secrets = [{ id: "svc", name: "SERVICE_TOKEN", computers: [computer.configuration.name], allowedDomains: ["api.example.test"], state: "active" }]
  const saveSecret = vi.fn().mockResolvedValue(undefined)
  const removeSecret = vi.fn().mockResolvedValue(undefined)
  const { user } = await openDetail(source, { saveSecret, removeSecret })

  // Edit routes through the shared inline editor and the edit save path.
  await user.click(screen.getByRole("button", { name: "Edit SERVICE_TOKEN" }))
  const form = within(screen.getByRole("form", { name: "Edit SERVICE_TOKEN" }))
  await user.type(form.getByLabelText("Replacement value"), "rotated")
  await user.click(form.getByRole("button", { name: "Save" }))
  expect(saveSecret).toHaveBeenLastCalledWith(expect.objectContaining({ operation: "edit", id: "svc", value: "rotated" }))

  // Remove requires the same confirmation popover as the Secrets page.
  await user.click(screen.getByRole("button", { name: "Remove SERVICE_TOKEN" }))
  expect(removeSecret).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  expect(removeSecret).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Remove SERVICE_TOKEN" }))
  await user.keyboard("{Escape}")
  await waitFor(() => expect(screen.queryByText("Remove SERVICE_TOKEN?")).not.toBeInTheDocument())
  expect(removeSecret).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Remove SERVICE_TOKEN" }))
  await user.click(screen.getByRole("button", { name: /^Remove$/ }))
  expect(removeSecret).toHaveBeenCalledExactlyOnceWith("svc")
})

it("never lists a same-named local computer's secrets on a remote computer page", async () => {
  const source = localVmSource()
  const computer = localComputer(source)
  computer.device = { id: "office", computerId: computer.configuration.id, name: "Office", address: "office.test", connected: true }
  source.secrets = [{ id: "svc", name: "SERVICE_TOKEN", computers: [computer.configuration.name], allowedDomains: [], state: "active" }]
  await openDetail(source, { saveSecret: vi.fn(), removeSecret: vi.fn() }, computer)

  expect(screen.getByText("Secrets are available only for computers on this device.")).toBeVisible()
  expect(screen.queryByText("SERVICE_TOKEN")).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Add secret" })).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Edit SERVICE_TOKEN" })).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Remove SERVICE_TOKEN" })).not.toBeInTheDocument()
})

function runningVmWithPort(source: ApplicationSource, computer: ApplicationComputer) {
  computer.state = "running"
  computer.freshness = "fresh"
  source.network = { computers: [{ computer: computerTarget(computer), error: null, ports: [
    { port: 3000, hostPort: 43000, scheme: "http", state: "reachable", configured: true },
  ] }] }
}

it("reflects live network data and opens a reachable port from the Overview tab", async () => {
  const source = localVmSource()
  const computer = localComputer(source)
  runningVmWithPort(source, computer)
  const openNetworkPort = vi.fn().mockResolvedValue(undefined)
  const { user } = await openDetail(source, { openNetworkPort, saveNetworkPort: vi.fn(), removeNetworkPort: vi.fn(), refreshNetwork: vi.fn(async () => {}) })

  expect(screen.getByText("3000 → http://127.0.0.1:43000")).toBeVisible()
  await user.click(screen.getByRole("button", { name: `Open port 3000 in browser` }))
  expect(openNetworkPort).toHaveBeenCalledWith(computerTarget(computer), 3000)
})

it("adds a port fixed to this computer from the Overview tab", async () => {
  const source = localVmSource()
  const computer = localComputer(source)
  runningVmWithPort(source, computer)
  const saveNetworkPort = vi.fn().mockResolvedValue(undefined)
  const { user } = await openDetail(source, { saveNetworkPort, removeNetworkPort: vi.fn(), openNetworkPort: vi.fn(), refreshNetwork: vi.fn(async () => {}) })

  await user.click(screen.getByRole("button", { name: "Add port" }))
  await user.type(screen.getByRole("spinbutton", { name: "Port" }), "9000")
  await user.click(screen.getByRole("button", { name: "Add" }))
  expect(saveNetworkPort).toHaveBeenCalledWith({ computer: computerTarget(computer), port: 9000, hostPort: null, scheme: "http" })
})

it("confirms before removing a port from the Overview tab", async () => {
  const source = localVmSource()
  const computer = localComputer(source)
  runningVmWithPort(source, computer)
  const removeNetworkPort = vi.fn().mockResolvedValue(undefined)
  const { user } = await openDetail(source, { saveNetworkPort: vi.fn(), removeNetworkPort, openNetworkPort: vi.fn(), refreshNetwork: vi.fn(async () => {}) })

  await user.click(screen.getByRole("button", { name: `Remove port 3000 from ${computer.configuration.name}` }))
  expect(removeNetworkPort).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Remove" }))
  expect(removeNetworkPort).toHaveBeenCalledWith(computerTarget(computer), 3000)
})

it("confirms a delete in a popover on the detail page and returns to the list", async () => {
  const source = localVmSource()
  // A stopped computer so Delete is allowed.
  const computer = source.computers.find(item => item.state !== "running")
    ?? source.computers[0]!
  computer.state = "stopped"
  const onConfigurationsChange = vi.fn()
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={onConfigurationsChange} />)

  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${computer.configuration.name}` }))

  // The confirmation is a popover anchored to the menu button, not a dialog.
  expect(await screen.findByText(`Delete ${computer.configuration.name} permanently?`)).toBeVisible()
  expect(screen.queryByRole("dialog", { hidden: true })?.getAttribute("aria-modal")).not.toBe("true")
  const popover = within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!)
  expect(popover.getByText(/will be deleted. This can't be undone./)).toBeVisible()
  await user.click(popover.getByRole("button", { name: "Delete permanently" }))

  expect(onConfigurationsChange).toHaveBeenCalled()
  expect(await screen.findByRole("list", { name: "Configured computers" })).toBeVisible()
})

it("shows a destructive Delete confirmation from the detail ⋯ menu and Fork still opens its own popover", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  computer.state = "stopped"
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ forkCheckpoint: vi.fn() } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Delete ${computer.configuration.name}` }))
  expect(await screen.findByText(`Delete ${computer.configuration.name} permanently?`)).toBeVisible()
  const remove = within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Delete permanently" })
  expect(remove.className).toContain("destructive")
  await user.keyboard("{Escape}")
  await waitFor(() => expect(screen.queryByText(`Delete ${computer.configuration.name} permanently?`)).not.toBeInTheDocument())

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Fork ${computer.configuration.name}` }))
  expect(await screen.findByText(`Fork ${computer.configuration.name}`)).toBeVisible()
  expect(screen.queryByText(`Delete ${computer.configuration.name} permanently?`)).not.toBeInTheDocument()
})

it("does not bring a closed fork popover back when returning to the list", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ forkCheckpoint: vi.fn() } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Fork ${computer.configuration.name}` }))
  expect(await screen.findByText(`Fork ${computer.configuration.name}`)).toBeVisible()
  await user.keyboard("{Escape}")
  await waitFor(() => expect(screen.queryByText(`Fork ${computer.configuration.name}`)).not.toBeInTheDocument())

  await user.click(within(screen.getByRole("navigation", { name: "Breadcrumb" })).getByRole("button", { name: "Computers" }))
  expect(screen.getByRole("list", { name: "Configured computers" })).toBeVisible()
  expect(screen.queryByText(`Fork ${computer.configuration.name}`)).not.toBeInTheDocument()
  expect(document.querySelector("[data-slot=popover-content]")).toBeNull()
})

it("does not carry an open fork popover from the detail page to the list", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ forkCheckpoint: vi.fn() } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: `Open ${computer.configuration.name}` }))
  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Fork ${computer.configuration.name}` }))
  expect(await screen.findByText(`Fork ${computer.configuration.name}`)).toBeVisible()
  await user.click(within(screen.getByRole("navigation", { name: "Breadcrumb" })).getByRole("button", { name: "Computers" }))
  expect(screen.queryByText(`Fork ${computer.configuration.name}`)).not.toBeInTheDocument()
})

it("confirms a delete from the list row ⋯ menu with the same popover as the detail page", async () => {
  const source = localVmSource()
  const computer = source.computers[0]!
  computer.state = "stopped"
  const onConfigurationsChange = vi.fn()
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={onConfigurationsChange} />)
  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Delete ${computer.configuration.name}` }))
  expect(await screen.findByText(`Delete ${computer.configuration.name} permanently?`)).toBeVisible()
  const popover = within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!)
  // The row states what is lost exactly as the page does, checkpoint count included.
  expect(popover.getByText(`Its files and ${computer.checkpoints?.length ?? 0} checkpoints will be deleted. This can't be undone.`)).toBeVisible()
  expect(screen.queryByRole("menuitem", { name: /Confirm deletion/ })).not.toBeInTheDocument()
  await user.click(popover.getByRole("button", { name: "Delete permanently" }))
  await waitFor(() => expect(onConfigurationsChange).toHaveBeenCalled())
})

it("resets per-computer edit state when the page switches to another computer", async () => {
  const source = localVmSource()
  const [first, second] = source.computers.filter(item => !item.device)
  const user = userEvent.setup()
  const props = { source, actions: {} as ApplicationActions, onConfigurationsChange: vi.fn(), onOpenComputer: vi.fn(), onCloseComputer: vi.fn() }
  const { rerender } = render(<OverviewPage {...props} selectedComputerId={first.configuration.id} />)

  await user.click(screen.getByRole("button", { name: `More actions for ${first.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Edit ${first.configuration.name}` }))
  expect(screen.getByRole("heading", { name: `Edit ${first.configuration.name}` })).toBeVisible()

  rerender(<OverviewPage {...props} selectedComputerId={second.configuration.id} />)
  expect(screen.queryByRole("heading", { name: `Edit ${second.configuration.name}` })).not.toBeInTheDocument()
  expect(screen.queryByRole("heading", { name: `Edit ${first.configuration.name}` })).not.toBeInTheDocument()
})

it("keeps a remote computer's Ports section reachable when the local device's discovery fails", async () => {
  const source = localVmSource()
  const computer = localComputer(source)
  computer.device = { id: "office", computerId: computer.configuration.id, name: "Office", address: "office.test", connected: true }
  source.networkError = "Could not check network services."
  source.network = { computers: [
    { computer: computer.configuration.name, error: "Local discovery failed", ports: [] },
    { computer: computerTarget(computer), error: null, ports: [{ port: 3000, hostPort: 43000, scheme: "http", state: "reachable", configured: true }] },
  ] }
  const openNetworkPort = vi.fn(async () => {})
  const { user } = await openDetail(source, { openNetworkPort, refreshNetwork: vi.fn(async () => {}) }, computer)
  expect(screen.getByText("Reachable")).toBeVisible()
  expect(screen.queryByText("Could not check network services.")).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Open port 3000 in browser" }))
  expect(openNetworkPort).toHaveBeenCalledExactlyOnceWith(computerTarget(computer), 3000)
})


it.each([false, true])("does not claim there are no ports after failed discovery (remote=%s)", async remote => {
  const source = localVmSource()
  const computer = localComputer(source)
  computer.state = "running"
  computer.freshness = "fresh"
  if (remote) computer.device = { id: "office", computerId: computer.configuration.id, name: "Office", address: "office.test", connected: true }
  source.network = { computers: [{ computer: computerTarget(computer), error: "Could not check services.", ports: [] }] }
  const refreshNetwork = vi.fn(async () => {})
  const actions = { refreshNetwork } as unknown as ApplicationActions
  const { user, rerender } = await openDetail(source, actions, computer)
  expect(screen.getByRole("alert")).toHaveTextContent("Could not check services.")
  expect(screen.queryByText("No ports")).not.toBeInTheDocument()
  refreshNetwork.mockClear()
  await user.click(within(screen.getByRole("alert")).getByRole("button", { name: "Retry" }))
  expect(refreshNetwork).toHaveBeenCalledOnce()
  const refreshed = { ...source, network: { computers: [{ computer: computerTarget(computer), error: null, ports: [] }] } }
  rerender(<OverviewPage source={refreshed} actions={actions} onConfigurationsChange={vi.fn()} />)
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  expect(screen.getByText("No ports")).toBeVisible()
})


it.each([false, true])("waits for this computer's port discovery when another device has already loaded (cached=%s)", async cached => {
  const source = localVmSource()
  const computer = localComputer(source)
  computer.state = "running"
  computer.freshness = "fresh"
  computer.ports = cached ? [{ port: 3000, hostPort: 43000, scheme: "http", listening: true, configured: true }] : []
  computer.device = { id: "office", computerId: computer.configuration.id, name: "Office", address: "office.test", connected: true }
  source.network = { computers: [{ computer: computer.configuration.name, error: null, ports: [] }] }
  const actions = { refreshNetwork: vi.fn(async () => {}) } as unknown as ApplicationActions
  const { rerender } = await openDetail(source, actions, computer)
  expect(screen.getByRole("status", { name: "Loading ports" })).toBeVisible()
  expect(screen.queryByText("No ports")).not.toBeInTheDocument()
  if (cached) expect(screen.getByText("http://127.0.0.1:43000")).toBeVisible()
  const refreshed = { ...source, network: { computers: [...source.network.computers, { computer: computerTarget(computer), error: null, ports: [] }] } }
  rerender(<OverviewPage source={refreshed} actions={actions} onConfigurationsChange={vi.fn()} />)
  expect(screen.queryByRole("status", { name: "Loading ports" })).not.toBeInTheDocument()
  expect(screen.getByText("No ports")).toBeVisible()
  expect(screen.queryByText("http://127.0.0.1:43000")).not.toBeInTheDocument()
})
