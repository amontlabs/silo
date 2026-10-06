import { createApplicationActionsMock } from "@/test/application-actions"
import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { ApplicationPreview } from "@/fixtures/application-preview"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ComputerFixtureMode } from "@/fixtures/application-scenarios"


function renderApplication(scenario: Parameters<typeof applicationSourceForScenario>[0] = "running", source?: ApplicationSource) {
  const actions = createApplicationActionsMock()

  return {
    actions,
    user: userEvent.setup(),
    ...render(<ApplicationPreview source={source ?? applicationSourceForScenario(scenario)} actions={actions} />),
  }
}

function appNavigation() {
  return screen.getByRole("navigation", { name: "Silo navigation" })
}

function appPanel(name: string) {
  return screen.getByRole("region", { name })
}


it("sorts attention first without letting health order rewrite configuration order", async () => {
  const source = applicationSourceForScenario("running")
  const [dev, playgrounds, personal] = source.computers
  source.computers = [
    { ...dev, configuration: { ...dev.configuration, name: "normal" }, state: "running", stateDetail: "Running for 2h 18m", attention: undefined },
    { ...playgrounds, configuration: { ...playgrounds.configuration, name: "warning" }, state: "stopped", stateDetail: "Waiting for verification", attention: { level: "warning", message: "Storage is almost full." } },
    { ...personal, configuration: { ...personal.configuration, name: "error" }, state: "failed", stateDetail: "Start failed 3m ago", attention: { level: "warning", message: "Candidate networking did not become ready." } },
  ]
  const { actions, user } = renderApplication("running", source)

  const overview = within(appPanel("Computers"))
  const list = overview.getByRole("list", { name: "Configured computers" })
  const rows = within(list).getAllByRole("listitem")
  expect(rows.map((row) => row.getAttribute("data-computer-name"))).toEqual(["error", "warning", "normal"])
  expect(rows[0].querySelector("[data-computer-icon-state='error']")).toBeVisible()
  expect(rows[1].querySelector("[data-computer-icon-state='warning']")).toBeVisible()
  expect(rows[2].querySelector("[data-computer-icon-state='normal']")).toBeVisible()
  expect(within(rows[0]).getByRole("img", { name: "error status" })).toBeVisible()
  expect(within(rows[1]).getByRole("img", { name: "warning status" })).toBeVisible()
  expect(rows[0]).toHaveTextContent("Failed")
  expect(rows[1]).toHaveTextContent("Stopped")
  expect(rows[2]).toHaveTextContent("Running")
  expect(overview.queryByText("Running for 2h 18m")).not.toBeInTheDocument()
  expect(overview.queryByText("Waiting for verification")).not.toBeInTheDocument()

  expect(within(rows[0]).getByText(/Candidate networking did not become ready/)).toBeVisible()
  expect(within(rows[1]).getByText(/Storage is almost full/)).toBeVisible()
  expect(overview.queryByLabelText("Computer attention")).not.toBeInTheDocument()
  expect(overview.queryByText(/needs attention/i)).not.toBeInTheDocument()

  const overviewNavigation = within(within(appNavigation()).getByRole("group", { name: "Computer sections" })).getByRole("button", { name: /All computers/ })
  expect(within(overviewNavigation).getByRole("status", { name: "1 computer has an error" })).toHaveTextContent("1")
  expect(within(overviewNavigation).getByRole("status", { name: "1 computer has a warning" })).toHaveTextContent("1")

  await user.click(screen.getByRole("button", { name: "More actions for error" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for error" }))
  expect(within(list).getAllByRole("listitem").map((row) => row.getAttribute("data-computer-name"))).toEqual(["error", "error-copy", "warning", "normal"])
  await user.click(overview.getByRole("button", { name: "Cancel" }))

  within(rows[0]).getByRole("button", { name: "Reorder error" }).focus()
  await user.keyboard("{ArrowDown}")
  expect(screen.getByText("error can only be reordered within its status group.")).toBeInTheDocument()
  expect(actions.saveComputerConfiguration).not.toHaveBeenCalled()
})


it("dismisses a known crash and disables dismissal for stale observations", async () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  source.computers[0] = { ...source.computers[0], state: "failed", canDismissError: true }
  const app = renderApplication("running", source)
  await userEvent.setup().click(screen.getByRole("button", { name: "Dismiss dev error" }))
  expect(app.actions.dismissComputerError).toHaveBeenCalledWith("dev")
  expect(app.actions.startComputer).not.toHaveBeenCalled()
  app.unmount()
  source.computers[0].freshness = "stale"
  const stale = renderApplication("running", source)
  expect(screen.getByRole("button", { name: "Dismiss dev error" })).toBeDisabled()
  stale.unmount()
  source.computers[0].canDismissError = false
  renderApplication("running", source)
  expect(screen.queryByRole("button", { name: "Dismiss dev error" })).not.toBeInTheDocument()
})


it("shows Restarting and disables lifecycle controls while the runtime works", async () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  source.computers[0].lifecycleAction = "restart"
  const app = renderApplication("running", source)
  const panel = within(appPanel("Computers"))
  expect(panel.getByText("Restarting…")).toBeVisible()
  expect(panel.getByRole("button", { name: "Stop dev" })).toBeDisabled()
  // The ⋯ menu stays open to navigation; its items that change the computer are locked.
  await app.user.click(panel.getByRole("button", { name: "More actions for dev" }))
  for (const name of ["Restart dev", "Edit dev", "Delete dev"]) expect(screen.getByRole("menuitem", { name })).toHaveAttribute("data-disabled")
  app.unmount()
})


it("uses subtle row tones and readable labels for every fixture state", async () => {
  // One availability rule (I-12): Stop waits for a start to finish, and a computer whose
  // status could not be refreshed (the error fixture is stale) takes no lifecycle action.
  const cases: Array<{ mode: ComputerFixtureMode; state: string; tone: string; labelClass: string; hoverClass: string; stopShown: boolean; lifecycleEnabled: boolean; restartEnabled: boolean }> = [
    { mode: "running", state: "running", tone: "running", labelClass: "text-emerald-700", hoverClass: "hover:bg-emerald-500/[0.07]", stopShown: true, lifecycleEnabled: true, restartEnabled: true },
    { mode: "starting", state: "starting", tone: "starting", labelClass: "text-amber-700", hoverClass: "hover:bg-amber-500/[0.07]", stopShown: true, lifecycleEnabled: false, restartEnabled: false },
    { mode: "stopped", state: "stopped", tone: "stopped", labelClass: "text-muted-foreground", hoverClass: "hover:bg-muted/35", stopShown: false, lifecycleEnabled: true, restartEnabled: false },
    { mode: "warning", state: "stopped", tone: "warning", labelClass: "text-muted-foreground", hoverClass: "hover:bg-amber-500/[0.08]", stopShown: false, lifecycleEnabled: true, restartEnabled: false },
    { mode: "error", state: "failed", tone: "error", labelClass: "text-destructive", hoverClass: "hover:bg-destructive/[0.07]", stopShown: false, lifecycleEnabled: false, restartEnabled: false },
  ]

  for (const { mode, state, tone, labelClass, hoverClass, stopShown, lifecycleEnabled, restartEnabled } of cases) {
    const source = applicationSourceForScenario("running", undefined, mode)
    const application = renderApplication("running", source)
    const overview = within(appPanel("Computers"))
    const rows = within(overview.getByRole("list", { name: "Configured computers" })).getAllByRole("listitem")
    for (const row of rows) {
      expect(row.querySelector(`[data-computer-row-tone="${tone}"]`)).toHaveClass(hoverClass)
      expect(row.querySelector(`[data-computer-state="${state}"]`)).toHaveClass(labelClass)
    }
    if (mode === "warning") expect(overview.getAllByText(/Storage is almost full/)).toHaveLength(3)
    if (mode === "error") expect(overview.getAllByText(/Candidate networking did not become ready/)).toHaveLength(3)
    const devRow = rows.find((row) => row.getAttribute("data-computer-name") === "dev") as HTMLElement
    const controls = within(devRow).getByLabelText("Controls for dev")
    const stop = within(controls).queryByRole("button", { name: "Stop dev" })
    const lifecycle = stopShown ? stop : within(controls).getByRole("button", { name: "Start dev" })
    if (!stopShown) expect(stop).not.toBeInTheDocument()
    if (lifecycleEnabled) expect(lifecycle).toBeEnabled()
    else expect(lifecycle).toBeDisabled()
    await application.user.click(within(controls).getByRole("button", { name: "More actions for dev" }))
    const restart = screen.getByRole("menuitem", { name: "Restart dev" })
    if (restartEnabled) expect(restart).not.toHaveAttribute("data-disabled")
    else expect(restart).toHaveAttribute("data-disabled")
    application.unmount()
  }
})


it.each([
  ["add-configuring", "scratch", "Configuring computer 'scratch'.", "0 of 3 steps complete"],
  ["add-networking", "scratch", "Starting candidate networking for 'scratch'.", "1 of 3 steps complete"],
  ["add-verifying", "scratch", "Verifying 'scratch'.", "2 of 3 steps complete"],
] as const)("shows %s progress inside only the affected computer", (fixture, computer, message, progressLabel) => {
  renderApplication("running", applicationSourceForScenario("running", undefined, undefined, fixture))
  const overview = within(appPanel("Computers"))
  const row = overview.getByText(computer).closest("li") as HTMLElement
  const overviewNavigation = within(within(appNavigation()).getByRole("group", { name: "Computer sections" })).getByRole("button", { name: /All computers/ })

  const existing = overview.getByText("dev").closest("li") as HTMLElement
  expect(existing).not.toHaveAttribute("aria-busy")
  expect(within(existing).queryByRole("progressbar")).not.toBeInTheDocument()
  expect(within(existing).getByLabelText("Controls for dev")).toBeInTheDocument()
  expect(row).toHaveAttribute("aria-busy", "true")
  expect(within(row).getByRole("status")).toHaveTextContent(message)
  expect(within(row).getByRole("progressbar", { name: progressLabel })).toBeVisible()
  expect(within(row).queryByLabelText(`Controls for ${computer}`)).not.toBeInTheDocument()
  expect(within(row).queryByLabelText(`Manage ${computer}`)).not.toBeInTheDocument()
  expect(within(row).getByRole("button", { name: `Reorder ${computer}` })).toHaveAttribute("aria-disabled", "true")
  expect(overview.getByRole("button", { name: "Add" })).toBeDisabled()
  expect(overview.queryByText(/Creating your computers/)).not.toBeInTheDocument()
  expect(within(overviewNavigation).queryByRole("status")).not.toBeInTheDocument()
})


it("keeps removal feedback inside the retained computer row", () => {
  renderApplication("running", applicationSourceForScenario("running", undefined, undefined, "remove-pending"))
  const overview = within(appPanel("Computers"))
  const row = overview.getByText("playgrounds").closest("li") as HTMLElement

  expect(row).toHaveAttribute("aria-busy", "true")
  expect(within(row).getByRole("status")).toHaveTextContent("Deleting the computer’s files and checkpoints.")
  expect(within(row).queryByRole("progressbar")).not.toBeInTheDocument()
  expect(within(row).queryByLabelText("Controls for playgrounds")).not.toBeInTheDocument()
  expect(within(row).queryByLabelText("Manage playgrounds")).not.toBeInTheDocument()
})


it("puts a retryable configuration failure and recovery inside its computer", async () => {
  const { actions, user } = renderApplication("running", applicationSourceForScenario("running", undefined, "warning", "computer-error"))
  const overview = within(appPanel("Computers"))
  const list = overview.getByRole("list", { name: "Configured computers" })
  const rows = within(list).getAllByRole("listitem")
  expect(rows.map((item) => item.getAttribute("data-computer-name"))).toEqual(["scratch", "dev", "playgrounds", "personal"])
  const row = rows[0]

  expect(row).not.toHaveAttribute("aria-busy")
  expect(within(row).getByRole("alert")).toHaveTextContent("Networking failed")
  expect(within(row).getByRole("alert")).toHaveTextContent("Repair computer startup or SSH forwarding, then retry.")
  expect(within(row).queryByLabelText("Manage scratch")).not.toBeInTheDocument()
  await user.click(within(row).getByRole("button", { name: "Retry scratch configuration" }))
  expect(actions.retryComputerConfiguration).toHaveBeenCalledWith("scratch")
  expect(overview.queryByText(/needs attention/i)).not.toBeInTheDocument()

  const overviewNavigation = within(within(appNavigation()).getByRole("group", { name: "Computer sections" })).getByRole("button", { name: /All computers/ })
  expect(within(overviewNavigation).getByRole("status", { name: "1 computer has an error" })).toHaveTextContent("1")
  expect(within(overviewNavigation).getByRole("status", { name: "3 computers have warnings" })).toHaveTextContent("3")
})


it("starts a new computer as an in-card configuration operation", async () => {
  const { user, actions } = renderApplication()
  const overview = within(appPanel("Computers"))
  const list = overview.getByRole("list", { name: "Configured computers" })
  const devRow = within(list).getByText("dev").closest("li")
  expect(devRow).not.toBeNull()

  expect(within(devRow as HTMLElement).queryByLabelText("Manage dev")).not.toBeInTheDocument()
  await user.click(within(devRow as HTMLElement).getByRole("button", { name: "More actions for dev" }))
  for (const action of ["Edit dev", "Duplicate settings for dev", "Delete dev"]) expect(screen.getByRole("menuitem", { name: action })).toBeVisible()
  await user.keyboard("{Escape}")
  await user.hover(devRow as HTMLElement)

  await user.click(overview.getByRole("button", { name: "Add" }))
  await user.click(screen.getByRole("menuitem", { name: "New computer" }))
  const name = overview.getByRole("textbox", { name: "Computer name" })
  expect(name).toHaveValue("computer-4")
  expect(name).toHaveFocus()
  await user.clear(name)
  await user.type(name, "scratch")
  await user.click(overview.getByRole("button", { name: "Create" }))

  expect(overview.getByText("3 configured · Applying computer changes")).toBeVisible()
  const scratchRow = within(overview.getByRole("list", { name: "Configured computers" })).getByText("scratch").closest("li") as HTMLElement
  expect(scratchRow).toHaveAttribute("aria-busy", "true")
  expect(within(scratchRow).getByRole("status")).toHaveTextContent("Preparing computer configuration.")
  expect(within(scratchRow).queryByText("Stopped")).not.toBeInTheDocument()
  expect(actions.saveComputerConfiguration).toHaveBeenCalledWith(expect.objectContaining({
    computers: expect.arrayContaining([expect.objectContaining({ name: "scratch" })]),
  }), expect.anything())
})


it("keeps committed detail pages stable while an edit is being applied", async () => {
  const { user, actions } = renderApplication()
  const navigation = within(appNavigation())
  const overview = within(appPanel("Computers"))

  await user.click(screen.getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev" }))
  const name = overview.getByRole("textbox", { name: "Computer name" })
  expect(name).toHaveAttribute("readonly")
  expect(overview.getByText("Existing computers cannot be renamed or have their disks resized. To use a different disk size, create a new computer and transfer your data.")).toBeVisible()
  expect(overview.getByRole("combobox", { name: "Workspace disk" })).toBeDisabled()
  expect(overview.getByRole("combobox", { name: "Runtime disk" })).toBeDisabled()
  await user.hover(overview.getByLabelText(/Workspace disk: .*read-only/))
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Disk size is read-only.")
  await user.selectOptions(overview.getByRole("combobox", { name: "CPUs at start" }), "4")
  await user.click(overview.getByRole("button", { name: "Stop and save…" }))
  await user.click(overview.getByRole("button", { name: "Stop and save" }))

  const developmentRow = overview.getByText("dev").closest("li") as HTMLElement
  expect(developmentRow).toHaveAttribute("aria-busy", "true")
  expect(within(developmentRow).getByRole("status")).toHaveTextContent("Preparing computer configuration.")
  expect(overview.getByRole("button", { name: "Add" })).toBeDisabled()

  const computerSections = within(navigation.getByRole("group", { name: "Computer sections" }))
  await user.click(computerSections.getByRole("button", { name: "Files" }))
  const files = within(appPanel("Computers"))
  const repositories = files.getByRole("list", { name: "Repositories" })
  expect(within(repositories).getByText("silo")).toBeVisible()
  expect(within(repositories).getAllByLabelText("dev, Running")[0]).toBeVisible()

  await user.click(navigation.getByRole("button", { name: "Settings" }))
  const settings = within(appPanel("Settings"))
  await user.click(settings.getByRole("switch", { name: "Start computers at launch" }))
  expect(settings.getByRole("button", { name: "Remove dev" })).toBeVisible()
  expect(actions.saveComputerConfiguration).toHaveBeenLastCalledWith(expect.objectContaining({
    computers: expect.arrayContaining([expect.objectContaining({ name: "dev", cpus: 4 })]),
  }), expect.anything())
})


it("disables deletion of a running computer with a stop-first explanation but keeps editing available", async () => {
  const { user, actions } = renderApplication()
  await user.click(screen.getByRole("button", { name: "More actions for dev" }))
  const remove = screen.getByRole("menuitem", { name: "Delete dev" })
  expect(remove).toHaveAttribute("aria-disabled", "true")
  expect(screen.getByRole("menuitem", { name: "Edit dev" })).not.toHaveAttribute("data-disabled")
  await user.hover(remove.parentElement!)
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Stop the computer before deleting it.")
  expect(actions.saveComputerConfiguration).not.toHaveBeenCalled()
})


it("shows an unscoped removal failure and restores lifecycle controls when dismissed", async () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  source.computerConfigurationOperation = {
    id: "removal", status: "failed", result: null, progressEvents: [],
    candidate: { schemaVersion: 1, computers: source.computers.filter(w => w.configuration.name !== "dev").map(w => w.configuration) },
    error: { code: "native_bridge_failed", computer: null, message: "Stop computer 'dev' before removing it.", recovery: null, retryable: true },
  }
  const { user } = renderApplication("running", source)
  expect(screen.getAllByText("Stop computer 'dev' before removing it.").length).toBeGreaterThan(0)
  expect(screen.queryByText(/Applying computer changes/)).not.toBeInTheDocument()
  expect(within(appPanel("Computers")).getByText("dev").closest("li")).not.toHaveAttribute("aria-busy")
  await user.click(screen.getByRole("button", { name: "Dismiss configuration error" }))
  expect(screen.getByRole("button", { name: "Stop dev" })).toBeEnabled()
})


it("keeps a removed computer as a progress tombstone until the native snapshot changes", async () => {
  const { actions, user } = renderApplication()
  const overview = within(appPanel("Computers"))

  await user.click(screen.getByRole("button", { name: "More actions for playgrounds" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete playgrounds" }))
  await user.click(within((await screen.findByText("Delete playgrounds permanently?")).closest<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Delete permanently" }))

  const row = overview.getByText("playgrounds").closest("li") as HTMLElement
  expect(row).toHaveAttribute("aria-busy", "true")
  expect(within(row).getByRole("status")).toHaveTextContent("Deleting the computer’s files and checkpoints.")
  expect(actions.saveComputerConfiguration).toHaveBeenLastCalledWith(expect.objectContaining({
    computers: expect.not.arrayContaining([expect.objectContaining({ name: "playgrounds" })]),
  }), expect.anything())
})


it("shows pending secret changes on the affected computer until the source confirms they are active", async () => {
  const source = applicationSourceForScenario("running")
  source.secrets.push({ id: "service-token", name: "SERVICE_TOKEN", computers: ["dev"], allowedDomains: [], state: "restart-required" })
  const { user, actions, rerender } = renderApplication("running", source)
  const overview = within(appPanel("Computers"))
  const label = overview.getByRole("note", { name: "Restart required for dev" })
  expect(label).toHaveTextContent("Restart required")
  expect(overview.getAllByRole("note")).toHaveLength(1)

  await user.hover(label)
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Restart dev to apply secret changes: DATABASE_URL, SERVICE_TOKEN.")
  await user.unhover(label)
  await user.click(screen.getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Restart dev" }))
  await user.click(within((await screen.findByText("Restart dev?")).closest<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Restart" }))
  await waitFor(() => expect(actions.restartComputer).toHaveBeenCalledWith("dev"))
  expect(label).toBeVisible()

  rerender(<ApplicationPreview source={{ ...source, secrets: source.secrets.map((secret) => ({ ...secret, state: "active" })) }} actions={actions} />)
  expect(overview.queryByRole("note", { name: "Restart required for dev" })).not.toBeInTheDocument()
})


it("explains pending secrets on keyboard focus and uses next-start wording for a stopped computer", async () => {
  const source = applicationSourceForScenario("running", undefined, "stopped")
  renderApplication("running", source)
  const overview = within(appPanel("Computers"))
  const label = overview.getByRole("note", { name: "Secret changes apply on next start for dev" })
  expect(label).toHaveTextContent("Applies on next start")
  act(() => label.focus())
  expect(label).toHaveFocus()
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Start dev to apply secret changes: DATABASE_URL.")
  expect(overview.queryByRole("note", { name: "Restart required for dev" })).not.toBeInTheDocument()
})


it("opens the computer terminal and a selected editor folder from overview", async () => {
  const source = applicationSourceForScenario("running")
  const { actions, user } = renderApplication("running", source)
  const overview = within(appPanel("Computers"))
  await user.click(overview.getByRole("button", { name: `Open dev in ${source.preferences.terminal}` }))
  expect(actions.openTerminal).toHaveBeenCalledWith("dev")
  expect(overview.getByRole("button", { name: `Open playgrounds in ${source.preferences.editor}` })).toBeDisabled()
  await user.click(overview.getByRole("button", { name: `Open dev in ${source.preferences.editor}` }))
  const open = overview.getByRole("button", { name: `Open in ${source.preferences.editor}` })
  await waitFor(() => expect(open).toBeEnabled())
  await user.click(open)
  expect(actions.openEditor).toHaveBeenCalledWith("dev", "/workspace")
  await user.click(overview.getByRole("button", { name: "Back to computers" }))
  expect(overview.getByRole("button", { name: "Stop dev" })).toBeVisible()
  const sections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))
  await user.click(sections.getByRole("button", { name: "Files" }))
  const repositories = within(overview.getByRole("list", { name: "Repositories" }))
  await user.click(repositories.getAllByRole("button", { name: `Open in ${source.preferences.editor}` })[0])
  expect(actions.openEditor).toHaveBeenLastCalledWith("dev", source.computers[0].repositories[0].path)
})


it("routes compact lifecycle actions with the exact computer", async () => {
  const running = renderApplication()
  const overview = within(appPanel("Computers"))
  const list = overview.getByRole("list", { name: "Configured computers" })
  const devRow = within(list).getByText("dev").closest("li") as HTMLElement
  const devControls = within(devRow).getByLabelText("Controls for dev")
  const playgroundsRow = within(list).getByText("playgrounds").closest("li") as HTMLElement
  const playgroundsControls = within(playgroundsRow).getByLabelText("Controls for playgrounds")

  expect(overview.queryByText("Silo is ready")).not.toBeInTheDocument()
  expect(overview.queryByText(/items? need attention/)).not.toBeInTheDocument()
  expect(within(devControls).queryByRole("button", { name: "Pause dev" })).not.toBeInTheDocument()
  expect(within(devControls).queryByRole("button", { name: "Start dev" })).not.toBeInTheDocument()
  const confirm = async (title: string, label: string) => running.user.click(within((await screen.findByText(title)).closest<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: label }))
  await running.user.click(within(devControls).getByRole("button", { name: "Stop dev" }))
  await confirm("Stop dev?", "Stop")
  await waitFor(() => expect(running.actions.stopComputer).toHaveBeenCalledWith("dev"))
  await running.user.click(within(devControls).getByRole("button", { name: "More actions for dev" }))
  await running.user.click(screen.getByRole("menuitem", { name: "Restart dev" }))
  await confirm("Restart dev?", "Restart")
  await waitFor(() => expect(running.actions.restartComputer).toHaveBeenCalledWith("dev"))
  const startPlaygrounds = within(playgroundsControls).getByRole("button", { name: "Start playgrounds" })
  const stopPlaygrounds = within(playgroundsControls).queryByRole("button", { name: "Stop playgrounds" })
  await running.user.click(within(playgroundsControls).getByRole("button", { name: "More actions for playgrounds" }))
  const restartPlaygrounds = screen.getByRole("menuitem", { name: "Restart playgrounds" })
  expect(startPlaygrounds).toBeEnabled()
  expect(stopPlaygrounds).not.toBeInTheDocument()
  expect(restartPlaygrounds).toHaveAttribute("data-disabled")
  await running.user.keyboard("{Escape}")
  await running.user.click(startPlaygrounds)
  expect(running.actions.startComputer).toHaveBeenCalledWith("playgrounds")
  expect(running.actions.stopComputer).not.toHaveBeenCalledWith("playgrounds")
  expect(running.actions.restartComputer).not.toHaveBeenCalledWith("playgrounds")
  running.unmount()
})


it("routes a global runtime failure to a dedicated item above Settings", async () => {
  const failed = renderApplication("dependency-failure")
  const navigationElement = appNavigation()
  const navigation = within(navigationElement)
  const primaryItems = [...navigationElement.querySelectorAll<HTMLElement>("[data-navigation-level='primary']")]

  expect(primaryItems).toEqual([
    "Computers",
    "GitHub",
    "Secrets",
    "System issue",
    "Settings",
  ].map(name => navigation.getByRole("button", { name })))
  expect(primaryItems.at(-2)).toHaveAttribute("data-navigation-tone", "danger")
  expect(within(appPanel("Computers")).queryByText("Silo runtime is unavailable")).not.toBeInTheDocument()

  await failed.user.click(navigation.getByRole("button", { name: "System issue" }))

  expect(navigation.getByRole("button", { name: "System issue" })).toHaveAttribute("aria-current", "page")
  const systemIssue = within(appPanel("System issue"))
  expect(systemIssue.getByRole("heading", { name: "System issue", level: 2 })).toBeVisible()
  expect(systemIssue.getByRole("heading", { name: "Silo runtime is unavailable", level: 3 })).toBeVisible()
  expect(systemIssue.getByText("Silo could not verify the bundled runtime used to manage computers.")).toBeVisible()
  expect(systemIssue.queryByText(/Repair reinstalls Silo/)).not.toBeInTheDocument()
  expect(systemIssue.getByText("Retry checks. If the runtime is still unavailable, quit and reopen Silo.")).toBeVisible()

  await failed.user.click(systemIssue.getByRole("button", { name: "Retry checks" }))
  expect(failed.actions.retryRuntimeChecks).toHaveBeenCalledOnce()
})


it("removes a resolved system issue and returns to Computers", async () => {
  const source = applicationSourceForScenario("dependency-failure")
  const application = renderApplication("dependency-failure", source)
  const navigation = within(appNavigation())

  await application.user.click(navigation.getByRole("button", { name: "System issue" }))
  expect(appPanel("System issue")).toBeVisible()

  application.rerender(
    <ApplicationPreview
      source={{ ...source, runtimeRepair: null }}
      actions={application.actions}
    />,
  )

  expect(navigation.queryByRole("button", { name: "System issue" })).not.toBeInTheDocument()
  expect(within(appPanel("Computers")).getByRole("list", { name: "Configured computers" })).toBeVisible()
})


it("keeps the issue visible and prevents duplicate retries while checking", async () => {
  const source = applicationSourceForScenario("running", undefined, undefined, undefined, "checking")
  const application = renderApplication("running", source)
  await application.user.click(within(appNavigation()).getByRole("button", { name: "System issue" }))
  const page = within(appPanel("System issue"))
  expect(page.getByRole("alert")).toHaveTextContent("Silo could not verify the bundled runtime")
  expect(page.getByRole("button", { name: "Checking…" })).toBeDisabled()
  expect(page.queryAllByRole("list")).toEqual([])
  await application.user.click(page.getByRole("button", { name: "Checking…" }))
  expect(application.actions.retryRuntimeChecks).not.toHaveBeenCalled()
})


it("shows the specific host fix without recommending reinstallation", async () => {
  const source = applicationSourceForScenario("running")
  source.runtimeRepair = { status: "unavailable", reason: "KVM access is denied.", recovery: "Ask your administrator to grant your user access to /dev/kvm, then retry checks." }
  const application = renderApplication("running", source)
  await application.user.click(within(appNavigation()).getByRole("button", { name: "System issue" }))
  const page = within(appPanel("System issue"))
  expect(page.getByText(source.runtimeRepair.recovery!)).toBeVisible()
  expect(page.queryByText(/reinstall/i)).not.toBeInTheDocument()
  await application.user.click(page.getByRole("button", { name: "Retry checks" }))
  expect(application.actions.retryRuntimeChecks).toHaveBeenCalledOnce()
})


it("gives reinstall guidance when the bundled runtime is unavailable", async () => {
  const source = applicationSourceForScenario("running", undefined, undefined, undefined, "runtime-missing")
  const application = renderApplication("running", source)

  await application.user.click(within(appNavigation()).getByRole("button", { name: "System issue" }))
  const page = within(appPanel("System issue"))
  expect(page.getByRole("heading", { name: "Silo runtime is unavailable", level: 3 })).toBeVisible()
  expect(page.getByText("This app build is missing its bundled Silo runtime.")).toBeVisible()
  expect(page.getByText("Reinstall Silo from a complete app bundle. Keep your existing computers and settings.")).toBeVisible()
  expect(page.queryByRole("button", { name: /repair/i })).not.toBeInTheDocument()
})


it("renders the native app domains in the polished Silo shell", async () => {
  const { user } = renderApplication()
  const navigation = within(appNavigation())

  await user.click(navigation.getByRole("button", { name: "GitHub" }))
  const github = within(appPanel("GitHub"))
  expect(github.getByText("Connected as @taylor")).toBeVisible()
  expect(github.getByRole("region", { name: "Computer Git identity and repository access" })).toBeVisible()

  await user.click(navigation.getByRole("button", { name: "Secrets" }))
  const secrets = within(appPanel("Secrets"))
  expect(secrets.getByText("DATABASE_URL")).toBeVisible()
  expect(secrets.queryByRole("alert")).not.toBeInTheDocument()
  const secretList = within(secrets.getByRole("list", { name: "Configured secrets" }))
  const pendingSecret = secretList.getAllByRole("listitem").find((row) => row.textContent?.includes("DATABASE_URL"))!
  expect(within(pendingSecret).getByText("Restart to apply")).toBeVisible()
})


it("applies Reduce motion to tooltips outside the app window and restores animations when disabled", async () => {
  // jsdom does not load the app stylesheet. Supply an animation so this checks
  // that the preference overrides it, rather than passing on an unstyled tooltip.
  render(<style>{'[data-slot="tooltip-content"] { animation: tooltip-fade 150ms; }'}</style>)
  const { user } = renderApplication()
  const navigation = within(appNavigation())

  for (const reduceMotion of [true, false]) {
    await user.click(navigation.getByRole("button", { name: "Settings" }))
    await user.click(screen.getByRole("switch", { name: "Reduce motion" }))
    await user.click(navigation.getByRole("button", { name: "Secrets" }))
    act(() => screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }).focus())

    const tooltip = screen.getByRole("tooltip", { name: "Edit PACKAGE_TOKEN" }).closest<HTMLElement>('[data-slot="tooltip-content"]')!
    expect(tooltip).not.toBeNull()
    // Radix temporarily disables animations while measuring offscreen content.
    await waitFor(() => expect(tooltip.parentElement).not.toHaveStyle({ transform: "translate(0, -200%)" }))
    expect(screen.getByRole("region", { name: "Silo" })).not.toContainElement(tooltip)
    expect(getComputedStyle(tooltip).animation).toBe(reduceMotion ? "none" : "tooltip-fade 150ms")
  }
})


it("preserves notification and general preferences across app sections", async () => {
  const { user } = renderApplication()
  const navigation = within(appNavigation())

  await user.click(navigation.getByRole("button", { name: "Expand Settings menu" }))
  const settingsNavigation = within(navigation.getByRole("group", { name: "Settings sections" }))
  await user.click(settingsNavigation.getByRole("button", { name: "Notifications" }))
  const settings = within(appPanel("Settings"))
  await user.click(await settings.findByRole("switch", { name: "Enable notifications" }))
  expect(settings.getByRole("switch", { name: "Unexpected computer changes" })).toBeDisabled()

  await user.click(settingsNavigation.getByRole("button", { name: "General" }))
  await user.click(settings.getByRole("switch", { name: "Reduce motion" }))
  expect(screen.getByRole("region", { name: "Silo" })).toHaveAttribute("data-reduce-motion", "true")
  await user.click(settings.getByRole("switch", { name: "Start computers at launch" }))
  await user.click(settings.getByRole("combobox", { name: "Add computer at startup" }))
  await user.click(screen.getByRole("option", { name: "playgrounds" }))
  expect(settings.getByRole("button", { name: "Remove playgrounds" })).toBeVisible()
  const browser = settings.getByRole("combobox", { name: "Browser" })
  expect(browser).toHaveTextContent("Safari")
  await user.click(browser)
  await user.click(screen.getByRole("option", { name: "Firefox" }))

  await user.click(navigation.getByRole("button", { name: "GitHub" }))
  await user.click(navigation.getByRole("button", { name: "Settings" }))
  expect(settings.getByRole("button", { name: "Remove playgrounds" })).toBeVisible()
  expect(settings.getByRole("combobox", { name: "Browser" })).toHaveTextContent("Firefox")
  expect(settings.getByRole("switch", { name: "Reduce motion" })).toBeChecked()
  await user.click(settings.getByRole("switch", { name: "Reduce motion" }))
  expect(screen.getByRole("region", { name: "Silo" })).not.toHaveAttribute("data-reduce-motion")

  await user.click(settingsNavigation.getByRole("button", { name: "Notifications" }))
  expect(settings.getByRole("switch", { name: "Enable notifications" })).not.toBeChecked()
})


it("prevents app interaction during installation and restores the existing page after failure", async () => {
const { UpdatesProvider } = await import("@/features/updates/update-store")
const user = userEvent.setup()
let emit!: (snapshot: import("@/features/updates/update-store").UpdateSnapshot) => void
const state: import("@/features/updates/update-store").UpdateSnapshot = { phase: "idle", lastChecked: null, retryAction: null, currentVersion: "0.1.0", availableVersion: null, releaseNotes: null, downloadedBytes: 0, totalBytes: null, automaticChecks: true, packageKind: "macos", releaseUrl: "https://github.com/amontlabs/silo/releases", error: null, errorDetails: null, installBlockReason: null, runningComputers: [], canInstall: true }
const backend = { read: async () => state, subscribe: async (receive: typeof emit) => { emit = receive; return () => {} }, check: vi.fn(), download: vi.fn(), install: vi.fn(), setAutomaticChecks: vi.fn(), openRelease: vi.fn() }
render(<UpdatesProvider backend={backend}><ApplicationPreview source={applicationSourceForScenario("running")} initialRoute={{ tab: "settings", settingsSection: "general" }} /></UpdatesProvider>)
await screen.findByText("Version 0.1.0")
await user.click(screen.getByRole("button", { name: "Search or jump to" }))
expect(screen.getByRole("dialog", { name: "Commands" })).toBeVisible()
act(() => emit({ ...state, phase: "installing" }))
expect(screen.queryByRole("dialog", { name: "Commands" })).not.toBeInTheDocument()
expect(screen.getByRole("button", { name: "Search or jump to" })).toBeDisabled()
expect(screen.getByRole("navigation", { name: "Silo navigation" })).toHaveAttribute("inert")
expect(screen.getByRole("region", { name: "Settings" }).closest("[inert]")).not.toBeNull()
// ProductionSurface owns the accessible installation status outside its guard.
expect(screen.queryByRole("button", { name: "View update" })).not.toBeInTheDocument()
act(() => emit({ ...state, phase: "error", error: "Could not install." }))
expect(screen.getByRole("region", { name: "Settings" }).closest("[inert]")).toBeNull()
expect(screen.getByRole("navigation", { name: "Silo navigation" })).not.toHaveAttribute("inert")
})


it.each([
["ready", null, "Restart and update", true],
["error", "install", "Retry", true],
["available", null, "Download update", false],
] as const)("guards app commands during pending %s action without blocking background downloads", async (phase, retryAction, label, blocked) => {
const { UpdatesProvider } = await import("@/features/updates/update-store")
const user = userEvent.setup()
const state: import("@/features/updates/update-store").UpdateSnapshot = {
  phase, retryAction, lastChecked: null, currentVersion: "0.3.3", availableVersion: "0.3.4", releaseNotes: null,
  downloadedBytes: 0, totalBytes: null, automaticChecks: true, packageKind: "appimage",
  releaseUrl: "https://github.com/amontlabs/silo/releases", error: retryAction ? "Could not install." : null,
  errorDetails: null, installBlockReason: null, runningComputers: [], canInstall: true,
}
let failAction!: (error: Error) => void
const action = vi.fn(() => new Promise<import("@/features/updates/update-store").UpdateSnapshot>((_, reject) => { failAction = reject }))
const backend = {
  read: async () => state, subscribe: async () => () => {}, check: vi.fn(), download: action, install: action,
  setAutomaticChecks: vi.fn(), openRelease: vi.fn(),
}
render(<UpdatesProvider backend={backend}><ApplicationPreview source={applicationSourceForScenario("running")} initialRoute={{ tab: "settings", settingsSection: "general" }} /></UpdatesProvider>)
await user.click(await screen.findByRole("button", { name: label }))
expect(action).toHaveBeenCalledOnce()
const palette = screen.getByRole("button", { name: "Search or jump to" })
if (blocked) {
  expect(palette).toBeDisabled()
  expect(screen.getByRole("navigation", { name: "Silo navigation" })).toHaveAttribute("inert")
  expect(screen.getByRole("region", { name: "Settings" }).closest("[inert]")).not.toBeNull()
} else {
  expect(palette).toBeEnabled()
  expect(screen.getByRole("navigation", { name: "Silo navigation" })).not.toHaveAttribute("inert")
}
await act(async () => failAction(new Error("Settings flush or native action failed")))
expect(palette).toBeEnabled()
expect(screen.getByRole("navigation", { name: "Silo navigation" })).not.toHaveAttribute("inert")
})
