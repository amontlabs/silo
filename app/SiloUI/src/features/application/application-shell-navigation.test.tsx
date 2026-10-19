import { setupFakeTimerUser } from "@/test/fake-timer-user"
import { createApplicationActionsMock } from "@/test/application-actions"
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { ApplicationPreview } from "@/fixtures/application-preview"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"

import { fixtureLogPage, type LogQuery } from "@/features/application/model/logs"

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

it("returns to cached logs after visiting another computer page", async () => {
  const source = applicationSourceForScenario("running")
  const computer = source.computers[0]
  const queryLogs = vi.fn(async (query: LogQuery) => fixtureLogPage(computer, query))
  const user = userEvent.setup()
  render(<ApplicationPreview source={source} actions={{ queryLogs }} initialRoute={{ computer: computer.configuration.name, computerSection: "logs" }} />)
  await screen.findByText(/Showing .* matching records/)
  const calls = queryLogs.mock.calls.length
  const sections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))
  await user.click(sections.getByRole("button", { name: "Files" }))
  await user.click(sections.getByRole("button", { name: "Logs" }))
  expect(screen.getByText(/Showing .* matching records/)).toBeVisible()
  expect(queryLogs).toHaveBeenCalledTimes(calls)
  await user.click(sections.getByRole("button", { name: "All computers" }))
  await user.click(sections.getByRole("button", { name: "Logs" }))
  expect(screen.getByText(/Showing .* matching records/)).toBeVisible()
  expect(queryLogs).toHaveBeenCalledTimes(calls)
})

it("refreshes repositories without toggling the pane and disables the button while loading", async () => {
  let finish!: () => void
  const refreshRepositories = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} actions={{ refreshRepositories }} initialRoute={{ computerSection: "files" }} />)
  const button = screen.getByRole("button", { name: "Refresh repositories" })
  await user.click(button)
  expect(refreshRepositories).toHaveBeenCalledTimes(1)
  expect(button).toBeDisabled()
  expect(screen.getByRole("button", { name: "Collapse repositories" })).toHaveAttribute("aria-expanded", "true")
  finish()
  await waitFor(() => expect(button).toBeEnabled())
})


it("opens failed activity logs in a diagnostic window and keeps a cleared range cleared", async () => {
  const source = applicationSourceForScenario("running")
  const computer = source.computers[0]
  source.activities = [{ id: "failed-start", category: "computer", title: "Start failed", detail: "Runtime failed", occurredAt: "2026-09-18T10:00:00Z", time: "Now", tone: "danger", status: "completed", computer: computer.configuration.name }]
  const queryLogs = vi.fn(async () => ({ entries: [], nextCursor: null, oldestAvailableTimestamp: null, newestAvailableTimestamp: null, totalMatches: 0, timestampEstimated: false }))
  const user = userEvent.setup()
  render(<ApplicationPreview source={source} actions={{ queryLogs }} initialRoute={{ computerSection: "activity" }} />)
  await user.click(screen.getByRole("button", { name: "Show logs" }))
  await waitFor(() => expect(queryLogs).toHaveBeenLastCalledWith(expect.objectContaining({ computerId: computer.configuration.id, since: "2026-09-18T09:55:00.000Z", until: "2026-09-18T10:05:00.000Z" })))
  await user.click(screen.getByRole("button", { name: /^Remove Date/ }))
  await waitFor(() => expect(queryLogs).toHaveBeenLastCalledWith(expect.objectContaining({ since: undefined, until: undefined })))
  const sections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))
  await user.click(sections.getByRole("button", { name: "Activity" }))
  await user.click(sections.getByRole("button", { name: "Logs" }))
  await waitFor(() => expect(queryLogs).toHaveBeenLastCalledWith(expect.objectContaining({ since: undefined, until: undefined })))
  expect(screen.queryByLabelText("Logs from")).not.toBeInTheDocument()
})


it("shows the recorded time for runtime logs without an embedded timestamp", async () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  const occurredAt = "2026-09-09T19:03:05.952Z"
  source.computers[0].logs = [{ line: "[  61.851852] reboot: Power down", occurredAt }]
  render(<ApplicationPreview source={source} initialRoute={{ computer: "dev", computerSection: "logs" }} />)
  const logs = within(await screen.findByRole("table", { name: "Logs" }))
  expect(logs.getByText("[ 61.851852] reboot: Power down")).toBeVisible()
  expect(logs.getByText(new Date(occurredAt).toLocaleTimeString())).toBeVisible()
})


it.each(["add-configuring", "remove-pending"] as const)("ignores property order on unchanged computers during %s", (operation) => {
  const source = structuredClone(applicationSourceForScenario("running", undefined, undefined, operation))
  source.computers = source.computers.map(computer => ({
    ...computer, configuration: Object.fromEntries(Object.entries(computer.configuration).reverse()) as typeof computer.configuration,
  }))
  renderApplication("running", source)
  const overview = within(appPanel("Computers"))
  for (const computer of source.computers.filter(({ configuration }) => source.computerConfigurationOperation?.candidate.computers.some(({ id }) => id === configuration.id))) {
    const row = overview.getByText(computer.configuration.name).closest("li") as HTMLElement
    expect(row).not.toHaveAttribute("aria-busy")
    expect(within(row).queryByText("Preparing computer configuration.")).not.toBeInTheDocument()
  }
})


it("formats activity dates in the user's locale and timezone", () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  const occurredAt = "2026-09-10T09:03:05Z"
  source.activities = [{ ...source.activities[0], occurredAt, time: occurredAt }]
  render(<ApplicationPreview source={source} initialRoute={{ computerSection: "activity" }} />)
  expect(screen.getByText(new Date(occurredAt).toLocaleString(undefined, { dateStyle: "short", timeStyle: "medium" }))).toBeVisible()
  expect(screen.queryByText(occurredAt)).not.toBeInTheDocument()
})


it("keeps secret edits across navigation and shows pending changes on affected computers", async () => {
  const { user, actions } = renderApplication()
  await user.click(within(appNavigation()).getByRole("button", { name: "Secrets" }))
  await user.click(await screen.findByRole("button", { name: "Edit PACKAGE_TOKEN" }))
  const form = within(screen.getByRole("form", { name: "Edit PACKAGE_TOKEN" }))
  await user.click(form.getByRole("combobox", { name: "Add computer" }))
  await user.click(screen.getByRole("option", { name: "personal" }))
  await user.clear(form.getByRole("textbox", { name: "Allowed domains" }))
  await user.type(form.getByRole("textbox", { name: "Allowed domains" }), "packages.example.test")
  await user.click(form.getByRole("button", { name: "Save" }))
  expect(actions.saveSecret).toHaveBeenCalledExactlyOnceWith({ operation: "edit", id: "package-token", name: "PACKAGE_TOKEN", computers: ["dev", "playgrounds", "personal"], allowedDomains: ["packages.example.test"] })

  await user.click(within(appNavigation()).getByRole("button", { name: "All computers" }))
  expect(screen.getByRole("note", { name: "Secret changes apply on next start for personal" })).toBeVisible()
  await user.click(within(appNavigation()).getByRole("button", { name: "Secrets" }))
  expect(screen.getByLabelText("Allowed domains for PACKAGE_TOKEN")).toHaveTextContent("packages.example.test")
  expect(within(screen.getByRole("group", { name: "Computers for PACKAGE_TOKEN" })).getByText("personal")).toBeVisible()
})


it("applies repeated status-panel routes without resetting the open application", async () => {
  const source = applicationSourceForScenario("running")
  const user = userEvent.setup()
  const { rerender } = render(<ApplicationPreview source={source} />)
  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }))
  rerender(<ApplicationPreview source={source} initialRoute={{ computer: "dev", computerSection: "logs" }} />)
  expect(within(appPanel("Computers")).getByRole("button", { name: "Remove dev" })).toBeVisible()
  expect(within(appNavigation()).getByRole("button", { name: "Logs" })).toHaveAttribute("aria-current", "page")
  rerender(<ApplicationPreview source={source} initialRoute={{ computer: "playgrounds", computerSection: "activity" }} />)
  expect(within(appPanel("Computers")).getByRole("button", { name: "Remove playgrounds" })).toBeVisible()
  expect(within(appPanel("Computers")).queryByRole("button", { name: "Remove dev" })).not.toBeInTheDocument()
  expect(within(appNavigation()).getByRole("button", { name: "Activity" })).toHaveAttribute("aria-current", "page")
  expect(appNavigation()).toHaveAttribute("data-collapsed", "true")
  rerender(<ApplicationPreview source={source} initialRoute={{ computer: "dev" }} />)
  expect(within(appNavigation()).getByRole("button", { name: "All computers" })).toHaveAttribute("aria-current", "page")
})


it("opens a computer detail page and returns to the list with the app's Back control", async () => {
  const { user } = renderApplication("running")
  await user.click(within(appPanel("Computers")).getByRole("button", { name: "Open dev" }))
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Computersdev")
  expect(within(appPanel("Computers")).queryByRole("list", { name: "Configured computers" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Go back" }))
  expect(within(appPanel("Computers")).getByRole("list", { name: "Configured computers" })).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Go forward" }))
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Computersdev")
})


it("deep-links into a computer detail page and tab from an initial route", () => {
  render(<ApplicationPreview source={applicationSourceForScenario("complete")} initialRoute={{ computer: "dev", computerTab: "checkpoints" }} />)
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Computersdev")
  expect(screen.getByRole("tab", { name: "Checkpoints" })).toHaveAttribute("aria-selected", "true")
})


it("collapses the sidebar to labelled icons and keeps every destination usable", async () => {
  const { user } = renderApplication()
  const navigation = within(appNavigation())

  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }))
  expect(screen.getByRole("button", { name: "Expand sidebar" })).toHaveAttribute("aria-expanded", "false")
  expect(appNavigation()).toHaveAttribute("data-collapsed", "true")
  expect(navigation.queryByRole("button", { name: "Collapse Computers menu" })).not.toBeInTheDocument()
  expect(navigation.queryByRole("group", { name: "Settings sections" })).not.toBeInTheDocument()
  for (const label of ["All computers", "Files", "Logs", "Network", "Activity", "GitHub", "Secrets", "Settings"]) {
    const button = navigation.getByRole("button", { name: label })
    expect(button.querySelector("svg")).toBeInTheDocument()
    expect(button).toHaveAccessibleName(label)
  }
  await user.click(navigation.getByRole("button", { name: "Settings" }))
  await user.click(navigation.getByRole("button", { name: "Notifications" }))
  expect(await within(appPanel("Settings")).findByRole("heading", { name: "Notifications", level: 2 })).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Expand sidebar" }))
  expect(appNavigation()).toHaveAttribute("data-collapsed", "false")
  expect(navigation.getByRole("button", { name: "Notifications" })).toHaveAttribute("aria-current", "page")
})


it("preserves closed computer and open settings menus when toggling sidebar width", async () => {
  const { user } = renderApplication()
  const navigation = within(appNavigation())
  await user.click(navigation.getByRole("button", { name: "Collapse Computers menu" }))
  await user.click(navigation.getByRole("button", { name: "Settings" }))

  for (const toggle of ["Collapse sidebar", "Expand sidebar"]) {
    await user.click(screen.getByRole("button", { name: toggle }))
    expect(navigation.queryByRole("group", { name: "Computer sections" })).not.toBeInTheDocument()
    expect(navigation.getByRole("group", { name: "Settings sections" })).toBeVisible()
    expect(navigation.getByRole("button", { name: "General" })).toHaveAttribute("aria-current", "page")
  }
})


it("navigates backward and forward through pages and nested sections, replacing the forward branch after a new visit", async () => {
  const { user } = renderApplication()
  const navigation = within(appNavigation())
  const back = screen.getByRole("button", { name: "Go back" })
  const forward = screen.getByRole("button", { name: "Go forward" })
  expect(back).toBeDisabled()
  expect(forward).toBeDisabled()

  await user.click(navigation.getByRole("button", { name: "Files" }))
  await user.click(navigation.getByRole("button", { name: "GitHub" }))
  await user.click(navigation.getByRole("button", { name: "Settings" }))
  await user.click(navigation.getByRole("button", { name: "Notifications" }))
  await user.click(back)
  expect(within(appPanel("Settings")).getByRole("heading", { name: "Appearance", level: 3 })).toBeVisible()
  await user.click(back)
  expect(appPanel("GitHub")).toBeVisible()
  await user.click(back)
  expect(within(appPanel("Computers")).getByRole("list", { name: "Repositories" })).toBeVisible()
  await user.click(forward)
  expect(appPanel("GitHub")).toBeVisible()
  await user.click(navigation.getByRole("button", { name: "Secrets" }))
  expect(forward).toBeDisabled()
  await user.click(navigation.getByRole("button", { name: "Secrets" }))
  await user.click(back)
  expect(appPanel("GitHub")).toBeVisible()
})


it("keeps busy and warning indicators visible in the collapsed sidebar", async () => {
  const { user } = renderApplication("running", applicationSourceForScenario("running", "connecting", "warning", undefined, undefined, "pushing"))
  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }))
  const navigation = within(appNavigation())
  for (const label of ["Files", "GitHub"]) {
    const button = navigation.getByRole("button", { name: label })
    expect(button).toHaveAttribute("aria-busy", "true")
    const icons = button.querySelectorAll("svg")
    expect(icons).toHaveLength(2)
    expect(icons[0]).not.toHaveAttribute("data-navigation-loading-indicator")
    expect(icons[0]).not.toHaveClass("animate-spin")
    expect(icons[1]).toHaveAttribute("data-navigation-loading-indicator")
    expect(icons[1]).toHaveClass("size-2")
  }
  expect(navigation.getByRole("status", { name: "3 computers need attention" })).toBeInTheDocument()
})


it.each(["past", "future"] as const)("removes a resolved issue from the %s navigation history", async (position) => {
  const application = renderApplication("running", applicationSourceForScenario("running", undefined, undefined, undefined, "checking"))
  const navigation = within(appNavigation())
  await application.user.click(navigation.getByRole("button", { name: "Files" }))
  await application.user.click(navigation.getByRole("button", { name: "System issue" }))
  await application.user.click(navigation.getByRole("button", { name: "GitHub" }))
  const back = screen.getByRole("button", { name: "Go back" })
  const forward = screen.getByRole("button", { name: "Go forward" })
  if (position === "future") {
    await application.user.click(back)
    await application.user.click(back)
  }
  application.rerender(<ApplicationPreview source={applicationSourceForScenario("running")} actions={application.actions} />)
  if (position === "past") {
    await application.user.click(back)
    expect(within(appPanel("Computers")).getByRole("list", { name: "Repositories" })).toBeVisible()
  }
  await application.user.click(forward)
  expect(appPanel("GitHub")).toBeVisible()
  expect(forward).toBeDisabled()
  expect(navigation.queryByRole("button", { name: "System issue" })).not.toBeInTheDocument()
})


it("opens and dismisses commands with either platform shortcut without losing the current page", async () => {
  const { user } = renderApplication()
  const invoker = within(appNavigation()).getByRole("button", { name: "GitHub" })
  await user.click(invoker)
  for (const shortcut of ["{Meta>}k{/Meta}", "{Control>}k{/Control}"]) {
    await user.keyboard(shortcut)
    const input = screen.getByRole("combobox", { name: "Search commands" })
    expect(input).toHaveFocus()
    expect(input).toHaveValue("")
    await user.type(input, "nothing-matches-this-command")
    expect(screen.getByText("No commands found.")).toBeVisible()
    await user.keyboard("{Escape}")
    expect(screen.queryByRole("dialog", { name: "Commands" })).not.toBeInTheDocument()
    expect(invoker).toHaveFocus()
  }
  expect(appPanel("GitHub")).toBeVisible()
})


it("filters commands and opens one computer's files using the keyboard", async () => {
  const { user } = renderApplication()
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), "dev")
  expect(screen.queryByRole("option", { name: "Computers" })).not.toBeInTheDocument()
  expect(screen.queryByRole("option", { name: "Open playgrounds activity" })).not.toBeInTheDocument()
  await user.keyboard(" files")
  expect(screen.getByRole("option", { name: "Open dev files" })).toBeVisible()
  await user.keyboard("{Enter}")
  expect(screen.queryByRole("dialog", { name: "Commands" })).not.toBeInTheDocument()
  expect(screen.getByRole("list", { name: "Files in dev" })).toBeVisible()
  expect(screen.queryByRole("list", { name: "Files in playgrounds" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Go back" }))
  expect(screen.getByRole("heading", { name: "Computers" })).toBeVisible()
})


it("toggles the menu with Ctrl-K instead of moving the selection", async () => {
  const { user } = renderApplication()
  await user.keyboard("{Control>}k{/Control}")
  expect(screen.getByRole("dialog", { name: "Commands" })).toBeVisible()
  await user.keyboard("{Control>}k{/Control}")
  expect(screen.queryByRole("dialog", { name: "Commands" })).not.toBeInTheDocument()
})


it.each(["Activity", "Files", "Logs", "Network"])("clears the computer filter when the general %s command follows a scoped command", async (section) => {
  const { user } = renderApplication()
  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.click(screen.getByRole("option", { name: `Open dev ${section.toLowerCase()}` }))
  expect(screen.getByRole("button", { name: "Remove dev" })).toBeVisible()
  if (section === "Activity") expect(screen.queryByText("Stop verified")).not.toBeInTheDocument()

  await user.click(screen.getByRole("button", { name: "Search or jump to" }))
  await user.click(screen.getByRole("option", { name: section }))
  const filters = within(screen.getByRole("group", { name: "Computer filters" }))
  expect(filters.queryByRole("button", { name: /^Remove / })).not.toBeInTheDocument()
  expect(filters.getByRole("button", { name: "Clear" })).toBeDisabled()
  if (section === "Activity") expect(screen.getByText("Stop verified")).toBeVisible()
})


it("dispatches available computer commands and removes them when status becomes stale", async () => {
  const application = renderApplication()
  await application.user.keyboard("{Meta>}k{/Meta}")
  const input = screen.getByRole("combobox", { name: "Search commands" })
  await application.user.type(input, "dev terminal")
  await application.user.keyboard("{Enter}")
  expect(application.actions.openTerminal).toHaveBeenCalledExactlyOnceWith("dev")
  await application.user.keyboard("{Control>}k{/Control}")
  await application.user.type(screen.getByRole("combobox", { name: "Search commands" }), "start playgrounds")
  await application.user.keyboard("{Enter}")
  expect(application.actions.startComputer).toHaveBeenCalledExactlyOnceWith("playgrounds")
  await application.user.keyboard("{Meta>}k{/Meta}")
  const source = applicationSourceForScenario("running")
  application.rerender(<ApplicationPreview source={{ ...source, computers: source.computers.map((computer) => ({ ...computer, freshness: "stale" })) }} actions={application.actions} />)
  expect(screen.queryByRole("option", { name: "Start playgrounds" })).not.toBeInTheDocument()
  expect(screen.queryByRole("option", { name: /Open dev in/ })).not.toBeInTheDocument()
  expect(screen.getByRole("option", { name: "Open dev logs" })).toBeVisible()
})


it("runs an import to completion and reflects the new stopped computer", async () => {
  vi.useFakeTimers()
  const user = setupFakeTimerUser()
  const application = renderApplication()
  try {
    const overviewNav = within(within(appNavigation()).getByRole("group", { name: "Computer sections" })).getByRole("button", { name: "All computers" })
    await user.click(screen.getByRole("button", { name: "Add" }))
    await user.click(screen.getByRole("menuitem", { name: "Import computer…" }))
    await act(async () => { await Promise.resolve() })
    // The import review popover opens anchored to Add; the import starts from it, then continues as a toast.
    fireEvent.click(screen.getByRole("button", { name: "Import" }))
    await act(async () => { await Promise.resolve() })
    expect(overviewNav).toHaveAttribute("aria-busy", "true")
    for (let step = 0; step < 4; step += 1) {
      await act(async () => { await vi.advanceTimersByTimeAsync(900) })
    }
    await act(async () => { await vi.advanceTimersByTimeAsync(50) })
    expect(overviewNav).not.toHaveAttribute("aria-busy", "true")
    const overview = within(appPanel("Computers"))
    expect(overview.getByRole("button", { name: "Stop dev" })).toBeEnabled()
    expect(overview.getByRole("button", { name: "Start dev-imported" })).toBeEnabled()
    expect(application.actions.stopComputer).not.toHaveBeenCalled()
    // Completion is a background toast, not an inline panel.
    expect(screen.queryByRole("region", { name: "Import computer" })).not.toBeInTheDocument()
    expect(screen.getByText("Imported dev-imported")).toBeInTheDocument()
  } finally {
    application.unmount()
    vi.useRealTimers()
  }
})


it("opens on the nested computer Overview with compact navigation", () => {
  renderApplication()

  const navigation = appNavigation()
  const primaryItems = [...navigation.querySelectorAll<HTMLElement>("[data-navigation-level='primary']")]
  expect(primaryItems).toEqual(["Computers", "GitHub", "Secrets", "Settings"].map(name => within(navigation).getByRole("button", { name })))
  for (const item of primaryItems) expect(item).toHaveClass("flex-none", "w-full")

  expect(within(navigation).getByRole("button", { name: "Computers" })).toHaveAttribute("aria-current", "page")
  const computerSections = within(navigation).getByRole("group", { name: "Computer sections" })
  expect(within(computerSections).getAllByRole("button")).toEqual(["All computers", "Files", "Logs", "Network", "Activity"].map(name => within(computerSections).getByRole("button", { name })))
  expect(within(computerSections).getByRole("button", { name: "All computers" })).toHaveAttribute("aria-current", "page")
  expect(computerSections).toHaveClass("sidebar-subnav")

  const overview = within(appPanel("Computers"))
  expect(overview.queryByRole("heading", { name: "All computers" })).not.toBeInTheDocument()
  expect(overview.queryByText(/Updated just now/)).not.toBeInTheDocument()
  expect(overview.getByRole("heading", { name: "Computers" })).toBeVisible()
  expect(overview.getByText("3 computers · 3 on this device · 0 on other devices")).toBeVisible()
  expect(overview.getByRole("button", { name: "Add" })).toBeVisible()
  const computerList = overview.getByRole("list", { name: "Configured computers" })
  expect(computerList).toBeVisible()
  expect(appPanel("Computers")).toHaveClass("h-full", "min-h-0", "overflow-hidden")
  expect(appPanel("Computers").parentElement).toHaveClass("overflow-hidden")
  expect(computerList.closest('[data-slot="scroll-area"]')).toHaveClass("max-h-full", "min-h-0")
  expect(computerList.closest('[data-slot="scroll-area"]')).not.toHaveClass("flex-1")
  expect(screen.queryByRole("group", { name: "Settings sections" })).not.toBeInTheDocument()
})


it.each([
  ["warning", "3 computers have warnings", "3"],
  ["error", "3 computers have errors", "3"],
] as const)("counts %s computers next to Overview", (mode, label, count) => {
  renderApplication("running", applicationSourceForScenario("running", undefined, mode))

  const overview = within(within(appNavigation()).getByRole("group", { name: "Computer sections" })).getByRole("button", { name: /All computers/ })
  expect(within(overview).getByRole("status", { name: label })).toHaveTextContent(count)
})


it("shows spinners on sidebar destinations that own active work", () => {
  const cases: Array<{
    label: string
    source: ApplicationSource
    section?: "computer"
  }> = [
    { label: "All computers", source: applicationSourceForScenario("running", undefined, "starting"), section: "computer" },
    { label: "All computers", source: applicationSourceForScenario("running", undefined, undefined, "add-verifying"), section: "computer" },
    { label: "Files", source: applicationSourceForScenario("running", undefined, undefined, undefined, undefined, "pushing"), section: "computer" },
    { label: "Files", source: applicationSourceForScenario("running", undefined, undefined, undefined, undefined, undefined, "git-live"), section: "computer" },
    { label: "GitHub", source: applicationSourceForScenario("running", "connecting") },
    { label: "GitHub", source: applicationSourceForScenario("running", "connected", undefined, undefined, undefined, undefined, undefined, 0, "applying") },
    { label: "Secrets", source: applicationSourceForScenario("running", undefined, undefined, undefined, undefined, undefined, "secrets-live") },
    { label: "All computers", source: applicationSourceForScenario("running", undefined, undefined, undefined, undefined, undefined, "backup-live"), section: "computer" },
    { label: "System issue", source: applicationSourceForScenario("running", undefined, undefined, undefined, "checking") },
  ]

  for (const { label, source, section } of cases) {
    const application = renderApplication("running", source)
    const navigation = within(appNavigation())
    const button = section === "computer"
      ? within(navigation.getByRole("group", { name: "Computer sections" })).getByRole("button", { name: label })
      : navigation.getByRole("button", { name: label })

    expect(button).toHaveAttribute("aria-busy", "true")
    const icons = button.querySelectorAll("svg")
    expect(icons).toHaveLength(2)
    expect(icons[0]).not.toHaveClass("animate-spin")
    expect(icons[1]).toHaveAttribute("data-navigation-loading-indicator")
    expect(icons[1]).toHaveClass("animate-spin")
    application.unmount()
  }
})


it("keeps Activity static while background work is running", () => {
  renderApplication("running", applicationSourceForScenario("running", undefined, undefined, undefined, undefined, undefined, "backup-live"))
  const activity = within(appNavigation()).getByRole("button", { name: "Activity" })
  expect(activity).not.toHaveAttribute("aria-busy")
  expect(activity.querySelector("[data-navigation-loading-indicator]")).toBeNull()
})


it("keeps idle and completed sidebar destinations static", () => {
  const source = applicationSourceForScenario("running", "connected", undefined, undefined, undefined, "succeeded", "backup-live", 4, "succeeded")
  renderApplication("running", source)
  const navigation = within(appNavigation())
  const computerSections = within(navigation.getByRole("group", { name: "Computer sections" }))

  for (const label of ["All computers", "Files", "Logs", "Network", "Activity"]) {
    expect(computerSections.getByRole("button", { name: label })).not.toHaveAttribute("aria-busy")
  }
  for (const label of ["GitHub", "Secrets", "Settings"]) {
    expect(navigation.getByRole("button", { name: label })).not.toHaveAttribute("aria-busy")
  }
})


it("lets each caret expand or collapse without navigating", async () => {
  const { user } = renderApplication()
  const navigation = within(appNavigation())
  const computers = navigation.getByRole("button", { name: "Computers" })

  await user.click(navigation.getByRole("button", { name: "Collapse Computers menu" }))
  expect(navigation.queryByRole("group", { name: "Computer sections" })).not.toBeInTheDocument()
  expect(computers).toHaveAttribute("aria-current", "page")
  expect(within(appPanel("Computers")).getByRole("list", { name: "Configured computers" })).toBeVisible()
  const computerCaret = navigation.getByRole("button", { name: "Expand Computers menu" })
  expect(computerCaret).toBeVisible()
  expect(computerCaret.querySelector("svg")).toBeVisible()

  const settingsCaret = navigation.getByRole("button", { name: "Expand Settings menu" })
  expect(settingsCaret).toBeVisible()
  expect(settingsCaret.querySelector("svg")).toBeVisible()
  await user.click(settingsCaret)
  expect(navigation.getByRole("group", { name: "Settings sections" })).toBeVisible()
  expect(computers).toHaveAttribute("aria-current", "page")
  expect(navigation.getByRole("button", { name: "Settings" })).not.toHaveAttribute("aria-current")

  await user.click(within(navigation.getByRole("group", { name: "Settings sections" })).getByRole("button", { name: "Notifications" }))
  expect(within(appPanel("Settings")).getByRole("heading", { name: "Notifications", level: 2 })).toBeVisible()

  await user.click(navigation.getByRole("button", { name: "Collapse Settings menu" }))
  expect(navigation.queryByRole("group", { name: "Settings sections" })).not.toBeInTheDocument()
  expect(within(appPanel("Settings")).getByRole("heading", { name: "Notifications", level: 2 })).toBeVisible()
})


it("fits a new computer to the capacity this device reports (I-24)", async () => {
  const source = { ...applicationSourceForScenario("running"), deviceCapacity: { logicalCpus: 8, physicalMemoryBytes: 16 * 1024 ** 3, maxMemoryGib: 16 } }
  const { user } = renderApplication("running", source)
  const panel = within(appPanel("Computers"))
  await user.click(panel.getByRole("button", { name: "Add" }))
  await user.click(screen.getByRole("menuitem", { name: "New computer" }))
  expect(panel.getByRole("combobox", { name: "Maximum CPUs" })).toHaveValue("8")
  expect(panel.getByRole("combobox", { name: "Maximum memory" })).toHaveValue("16")
  expect(panel.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
})


it("keeps an unsaved computer edit while visiting another section (I-37)", async () => {
  const { user } = renderApplication()
  const computerSections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))
  await user.click(screen.getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev" }))
  await user.selectOptions(within(appPanel("Computers")).getByRole("combobox", { name: "CPUs at start" }), "4")
  await user.click(computerSections.getByRole("button", { name: "Files" }))
  expect(within(appPanel("Computers")).queryByRole("combobox", { name: "CPUs at start" })).not.toBeInTheDocument()
  await user.click(computerSections.getByRole("button", { name: "All computers" }))
  expect(within(appPanel("Computers")).getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
})


it.each(["local", "office", "lab"])("notification routes keep the next action on %s despite duplicate names and a rename", async (owner) => {
  const source = structuredClone(applicationSourceForScenario("running"))
  const local = source.computers[0]
  const computerId = local.configuration.id
  source.devices = ["office", "lab"].map(id => ({ id, name: id, address: `user@${id}`, connected: true }))
  for (const device of source.devices) {
    source.computers.push({
      ...structuredClone(local),
      device: { ...device, computerId },
      configuration: { ...local.configuration, id: `silo-remote:${device.id}:${computerId}` },
    })
  }
  const target = owner === "local" ? computerId : `silo-remote:${owner}:${computerId}`
  const openTerminal = vi.fn()
  const user = userEvent.setup()
  const { rerender } = render(<ApplicationPreview source={source} actions={{ openTerminal }} initialRoute={{ computer: target }} />)
  await user.click(screen.getByRole("button", { name: /^Open .* in Terminal$/ }))
  expect(openTerminal).toHaveBeenLastCalledWith(owner === "local" ? local.configuration.name : target)
  const renamed = structuredClone(source)
  renamed.computers.find(computer => computer.configuration.id === target)!.configuration.name = "renamed"
  rerender(<ApplicationPreview source={renamed} actions={{ openTerminal }} initialRoute={{ computer: target }} />)
  await user.click(screen.getByRole("button", { name: /^Open .* in Terminal$/ }))
  expect(openTerminal).toHaveBeenLastCalledWith(owner === "local" ? "renamed" : target)
})


it("legacy local-name routes select only the local computer's logs", async () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  const local = source.computers[0]
  source.computers = [{
    ...structuredClone(local),
    configuration: { ...local.configuration, id: `silo-remote:office:${local.configuration.id}` },
    device: { id: "office", computerId: local.configuration.id, name: "Office", address: "office.test", connected: true },
  }, local]
  const queryLogs = vi.fn(async (query: LogQuery) => fixtureLogPage(query.deviceId ? source.computers[0] : local, query))
  render(<ApplicationPreview source={source} actions={{ queryLogs }} initialRoute={{ computer: local.configuration.name, computerSection: "logs" }} />)
  await screen.findByText(/Showing .* matching records/)
  expect(queryLogs).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ computerId: local.configuration.id }))
  expect(queryLogs.mock.calls[0][0].deviceId).toBeUndefined()
})

it("legacy local-name overview routes keep actions on the local device", async () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  const local = source.computers[0]
  source.computers = [{
    ...structuredClone(local),
    configuration: { ...local.configuration, id: `silo-remote:office:${local.configuration.id}` },
    device: { id: "office", computerId: local.configuration.id, name: "Office", address: "office.test", connected: true },
  }, local]
  const openTerminal = vi.fn()
  const user = userEvent.setup()
  render(<ApplicationPreview source={source} actions={{ openTerminal }} initialRoute={{ computer: local.configuration.name }} />)
  await user.click(screen.getByRole("button", { name: /^Open .* in Terminal$/ }))
  expect(openTerminal).toHaveBeenCalledExactlyOnceWith(local.configuration.name)
})


it("a recreated computer cannot inherit a legacy route in navigation history", async () => {
  const source = structuredClone(applicationSourceForScenario("running"))
  const local = source.computers[0]
  source.computers = [local]
  const user = userEvent.setup()
  const { rerender } = render(<ApplicationPreview source={source} initialRoute={{ computer: local.configuration.name }} />)
  const sections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))
  await user.click(sections.getByRole("button", { name: "Files" }))
  rerender(<ApplicationPreview source={{ ...source, computers: [{ ...local, configuration: { ...local.configuration, id: "replacement-vm" } }] }} />)
  await user.click(screen.getByRole("button", { name: "Go back" }))
  expect(within(appPanel("Computers")).getByRole("list", { name: "Configured computers" })).toBeVisible()
  expect(screen.getByRole("button", { name: "Go back" })).toBeDisabled()
})
