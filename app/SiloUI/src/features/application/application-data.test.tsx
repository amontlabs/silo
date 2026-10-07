import { createApplicationActionsMock } from "@/test/application-actions"
import { render, screen, within } from "@testing-library/react"
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


it("uses one global computer filter across Files, Logs, Network, and Activity", async () => {
  const { actions, user } = renderApplication()
  const navigation = within(appNavigation())
  const computerSections = within(navigation.getByRole("group", { name: "Computer sections" }))

  await user.click(computerSections.getByRole("button", { name: "Files" }))
  const panel = within(appPanel("Computers"))
  const filters = within(panel.getByRole("group", { name: "Computer filters" }))
  expect(panel.queryByRole("heading", { name: "Computers" })).not.toBeInTheDocument()
  expect(panel.queryByText("Inspect state, files, logs, networking, and recent activity.")).not.toBeInTheDocument()
  expect(filters.getByRole("combobox", { name: "Filter computers" })).toHaveAttribute("placeholder", "Filter computers…")
  expect(filters.queryByRole("button", { name: /^Remove / })).not.toBeInTheDocument()
  expect(filters.getByRole("button", { name: "Clear" })).toBeDisabled()
  expect(filters.queryByRole("button", { name: "All" })).not.toBeInTheDocument()

  const filesLayout = panel.getByRole("button", { name: "Collapse repositories" }).closest("[data-files-layout]") as HTMLElement
  const repositoriesPane = panel.getByRole("button", { name: "Collapse repositories" }).closest('[data-files-pane="repositories"]') as HTMLElement
  const fileTreePane = panel.getByRole("button", { name: "Collapse file tree" }).closest('[data-files-pane="file-tree"]') as HTMLElement
  expect(filesLayout).toHaveClass("h-full", "min-h-0", "flex-col", "justify-between", "lg:grid", "lg:grid-cols-2", "lg:grid-rows-1")
  expect(filesLayout).toHaveAttribute("data-file-tree-state", "open")
  expect(repositoriesPane).toHaveAttribute("data-pane-position", "top")
  expect(fileTreePane).toHaveAttribute("data-pane-position", "bottom")
  const repositoryPaneControls = within(repositoriesPane).getByRole("group", { name: "Repository pane controls" })
  const fileTreePaneControls = within(fileTreePane).getByRole("group", { name: "File tree pane controls" })
  expect(repositoryPaneControls).toContainElement(panel.getByRole("button", { name: "Collapse repositories" }))
  expect(fileTreePaneControls).toContainElement(panel.getByRole("button", { name: "Collapse file tree" }))
  expect(panel.getByRole("button", { name: "Collapse repositories" })).toHaveTextContent("Repositories")
  expect(panel.getByRole("button", { name: "Collapse file tree" })).toHaveTextContent("File tree")
  expect(panel.getByRole("button", { name: "Collapse repositories" }).querySelector("svg")).toHaveClass("lucide-chevron-down", "disclosure-caret")
  expect(panel.getByRole("button", { name: "Collapse file tree" }).querySelector("svg")).toHaveClass("lucide-chevron-down", "disclosure-caret")
  expect(repositoriesPane).toHaveClass("max-h-[50%]", "shrink-0")
  expect(fileTreePane).toHaveClass("flex-1")
  expect(repositoriesPane).toHaveClass("collapsible-motion")
  expect(fileTreePane).toHaveClass("collapsible-motion")
  expect(repositoriesPane.querySelector('[data-files-pane-content="repositories"]')).toHaveClass("file-pane-content-motion", "min-h-0", "flex-1")
  expect(fileTreePane.querySelector('[data-files-pane-content="file-tree"]')).toHaveClass("file-pane-content-motion", "min-h-0", "flex-1")
  expect(repositoriesPane.querySelector('[data-files-pane-scroll="repositories"]')).toHaveClass("h-full", "overflow-y-auto")
  expect(fileTreePane.querySelector('[data-files-pane-scroll="file-tree"]')).toHaveClass("h-full", "overflow-y-auto")

  await user.click(panel.getByRole("button", { name: "Collapse repositories" }))
  expect(panel.getByRole("button", { name: "Expand repositories" })).toHaveAttribute("aria-expanded", "false")
  expect(panel.queryByRole("list", { name: "Repositories" })).not.toBeInTheDocument()
  expect(repositoriesPane).toHaveClass("max-h-8", "shrink-0", "transition-[max-height]")
  expect(fileTreePane).toHaveClass("flex-1")
  await user.click(panel.getByRole("button", { name: "Expand repositories" }))

  await user.click(panel.getByRole("button", { name: "Collapse file tree" }))
  expect(panel.getByRole("button", { name: "Expand file tree" })).toHaveAttribute("aria-expanded", "false")
  expect(filesLayout).toHaveAttribute("data-file-tree-state", "closed")
  expect(panel.queryByRole("list", { name: "File tree" })).not.toBeInTheDocument()
  expect(repositoriesPane).toHaveClass("flex-1")
  expect(fileTreePane).toHaveClass("max-h-8", "flex-1", "transition-[max-height]")
  await user.click(panel.getByRole("button", { name: "Expand file tree" }))

  const repositories = panel.getByRole("list", { name: "Repositories" })
  const fileTree = panel.getByRole("list", { name: "File tree" })
  const repositoriesSection = panel.getByRole("region", { name: "Repositories" })
  const fileTreeSection = panel.getByRole("region", { name: "File tree" })
  expect(repositoriesSection.closest('[data-slot="card"]')).toBeNull()
  expect(fileTreeSection.closest('[data-slot="card"]')).toBeNull()
  expect(fileTreePane).toHaveClass("lg:border-l", "lg:pl-5")
  expect(fileTreePane).not.toHaveClass("border-l")
  const devRepository = within(repositories).getByText("silo").closest('[role="listitem"]') as HTMLElement
  const playgroundsRepository = within(repositories).getByText("platform-tools").closest('[role="listitem"]') as HTMLElement
  expect(within(devRepository).queryByText("acme/silo")).not.toBeInTheDocument()
  await user.hover(within(devRepository).getByText("silo"))
  expect(await screen.findByRole("tooltip")).toHaveTextContent("acme/silo")
  await user.unhover(within(devRepository).getByText("silo"))
  const devBadge = within(devRepository).getByLabelText("dev, Running")
  expect(devBadge).toBeVisible()
  expect(devBadge).toHaveAttribute("data-slot", "status-badge")
  expect(devBadge).toHaveClass("h-5", "items-center", "justify-center", "text-[10px]")
  expect(devBadge.querySelector('[data-slot="status-badge-indicator"]')).toHaveClass("grid", "size-2", "place-items-center")
  expect(devBadge.querySelector('[data-slot="status-badge-label"]')).toHaveTextContent("dev")
  expect(devRepository.querySelector('[data-computer-state-dot="running"]')).toHaveClass("bg-success")
  expect(within(playgroundsRepository).getByLabelText("playgrounds, Stopped")).toBeVisible()
  expect(playgroundsRepository.querySelector('[data-computer-state-dot="stopped"]')).toHaveClass("bg-muted-foreground/55")
  const repositoryHeader = devRepository.querySelector("[data-repository-header]") as HTMLElement
  const repositoryActions = devRepository.querySelector("[data-repository-actions]") as HTMLElement
  const pushButton = within(repositoryActions).getByRole("button", { name: "Push 2 commits for acme/silo in dev" })
  expect(repositoryActions).toHaveClass("flex", "min-h-6", "items-start")
  expect(pushButton).toHaveClass("h-6")
  expect(repositoryHeader).toContainElement(devBadge)
  expect(repositoryHeader).not.toContainElement(pushButton)
  expect(within(playgroundsRepository).queryByRole("button", { name: /^Push / })).not.toBeInTheDocument()

  await user.click(pushButton)
  expect(actions.pushRepository).not.toHaveBeenCalled()
  expect(within(screen.getByRole("dialog")).getByText("Push to acme/silo?")).toBeVisible()
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Push" }))
  expect(actions.pushRepository).toHaveBeenCalledWith("dev", "acme/silo", { repository: "acme/silo", branch: "main", commit: "4f1c2d9e8b7a6c5d4e3f2a1b0c9d8e7f6a5b4c3d" })
  expect(devRepository).toHaveAttribute("aria-busy", "true")
  expect(within(devRepository).getByRole("status")).toHaveClass("h-6")
  expect(within(devRepository).getByRole("status")).toHaveTextContent("Pushing 2 commits…")
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument()

  const devFolder = within(fileTree).getByRole("button", { name: "dev" })
  expect(devFolder).toHaveAttribute("aria-expanded", "true")
  expect(panel.getByRole("list", { name: "Files in dev" })).toBeVisible()
  await user.click(devFolder)
  expect(devFolder).toHaveAttribute("aria-expanded", "false")
  expect(panel.queryByRole("list", { name: "Files in dev" })).not.toBeInTheDocument()
  await user.click(devFolder)

  await user.click(filters.getByRole("combobox", { name: "Filter computers" }))
  await user.click(screen.getByRole("option", { name: "dev" }))
  expect(filters.getByRole("button", { name: "Remove dev" })).toBeVisible()
  expect(within(repositories).getByText("silo")).toBeVisible()
  expect(within(fileTree).getByRole("button", { name: "dev" })).toBeVisible()
  expect(within(repositories).queryByText("acme/platform-tools")).not.toBeInTheDocument()
  expect(within(fileTree).queryByRole("button", { name: "personal" })).not.toBeInTheDocument()

  await user.click(filters.getByRole("button", { name: "Remove dev" }))
  expect(within(repositories).getByText("platform-tools")).toBeVisible()
  expect(within(fileTree).getByRole("button", { name: "personal" })).toBeVisible()

  await user.click(filters.getByRole("combobox", { name: "Filter computers" }))
  await user.click(screen.getByRole("option", { name: "playgrounds" }))
  await user.click(filters.getByRole("combobox", { name: "Filter computers" }))
  await user.click(screen.getByRole("option", { name: "personal" }))
  await user.click(computerSections.getByRole("button", { name: "Logs" }))
  const logs = await panel.findByRole("table", { name: "Logs" })
  expect(logs.closest('[data-slot="card"]')).toBeNull()
  expect(logs).toHaveClass("max-h-full", "min-h-0", "overflow-hidden")
  expect(logs).not.toHaveClass("flex-1")
  expect(logs.querySelector('[data-table-scroll="logs"]')).toHaveClass("min-h-0", "overflow-y-auto")
  expect(logs.querySelector('[data-table-scroll="logs"]')).not.toHaveClass("flex-1")
  expect(within(logs).getAllByRole("columnheader")[0].parentElement).toHaveClass("shrink-0")
  expect(panel.queryByRole("heading", { name: "Logs" })).not.toBeInTheDocument()
  expect(within(logs).queryByText("dev")).not.toBeInTheDocument()
  expect(within(logs).getAllByRole("columnheader").map((header) => header.textContent)).toEqual(["Time", "Message", "Computer", "Source", "Actions"])
  expect(within(logs).getByLabelText("playgrounds, Stopped")).toBeVisible()
  expect(within(logs).getByLabelText("personal, Stopped")).toBeVisible()

  const playgroundsRow = within(logs).getByLabelText("playgrounds, Stopped").closest('[role="row"]') as HTMLElement
  expect(playgroundsRow).toHaveClass("row-hover")
  const playgroundsCells = within(playgroundsRow).getAllByRole("cell")
  expect(playgroundsCells[0]).toHaveTextContent("17:02:11")
  expect(playgroundsCells[1]).toHaveTextContent("silo Computer stopped cleanly")
  expect(playgroundsCells[2]).toContainElement(within(playgroundsRow).getByLabelText("playgrounds, Stopped"))
  const copyLine = within(playgroundsRow).getByRole("button", { name: "Copy log line from playgrounds at 17:02:11" })
  expect(copyLine).toHaveClass("opacity-0", "group-hover/log-row:opacity-100", "group-focus-within/log-row:opacity-100")
  const copy = vi.spyOn(navigator.clipboard, "writeText")
  await user.click(copyLine)
  expect(copy).toHaveBeenCalledWith("2026-09-03T17:02:11Z\tThis device (local)\tplaygrounds (00000000-0000-4000-8000-000000000002)\toutput\t\t17:02:11  silo  Computer stopped cleanly")
  const copiedLine = within(playgroundsRow).getByRole("button", { name: "Log line copied" })
  expect(copiedLine).toHaveAttribute("data-copy-status", "copied")
  expect(copiedLine.querySelector("svg")).toHaveClass("lucide-check")

  await user.click(panel.getByRole("button", { name: "Copy logs" }))
  expect(copy).toHaveBeenLastCalledWith(expect.stringMatching(/2026-09-03T17:02:11Z\tThis device \(local\)\tplaygrounds .*\n.*09:41:02.*\tThis device \(local\)\tpersonal .*Computer stopped cleanly/))
  const copiedLogs = panel.getByRole("button", { name: "Logs copied" })
  expect(copiedLogs).toHaveTextContent("Copied")
  expect(copiedLogs.querySelector("svg")).toHaveClass("lucide-check")

  await user.click(computerSections.getByRole("button", { name: "Network" }))
  expect(panel.queryByRole("region", { name: "Network for dev" })).not.toBeInTheDocument()
  expect(await panel.findByText("No ports")).toBeVisible()

  await user.click(computerSections.getByRole("button", { name: "Activity" }))
  const activity = panel.getByRole("list", { name: "Recent activity" })
  expect(activity.closest('[data-slot="card"]')).toBeNull()
  expect(activity).toHaveClass("max-h-full", "min-h-0", "overflow-y-auto")
  expect(activity).not.toHaveClass("flex-1")
  expect(panel.queryByRole("heading", { name: "Activity" })).not.toBeInTheDocument()
  expect(within(activity).queryByText("dev")).not.toBeInTheDocument()
  expect(within(activity).getByText("playgrounds")).toBeVisible()
  expect(within(activity).getByText("Export completed")).toBeVisible()
  const activityRows = within(activity).getAllByRole("listitem")
  expect(activityRows).toHaveLength(2)
  expect(within(activityRows[0]).getByText("Stop verified")).toBeVisible()

  expect(within(activityRows[0]).getByText("A fresh observation confirmed that the computer is stopped.")).toBeVisible()
  expect(within(activityRows[0]).getByLabelText("playgrounds, Stopped")).toBeVisible()
  const activityContent = activityRows[0].querySelector('[data-activity-content]') as HTMLElement
  const activityMeta = activityRows[0].querySelector('[data-activity-meta]') as HTMLElement
  const activityTime = activityMeta.querySelector("time")!
  expect(activityTime).toHaveTextContent(new Date(activityTime.dateTime).toLocaleString(undefined, { dateStyle: "short", timeStyle: "medium" }))
  expect(activityContent).not.toContainElement(activityTime)
  expect(activityContent).not.toContainElement(within(activityRows[0]).getByLabelText("Category: Computer"))
  expect(activityMeta).toHaveClass("items-end")
  expect(activityMeta).toContainElement(within(activityRows[0]).getByLabelText("Category: Computer"))

  const categoryFilters = within(panel.getByRole("group", { name: "Activity category filters" }))
  const categoryCombobox = categoryFilters.getByRole("combobox", { name: "Add category filter" })
  expect(categoryCombobox).toHaveClass("h-7", "w-36")
  expect(categoryFilters.queryByRole("button", { name: /^Remove / })).not.toBeInTheDocument()
  expect(categoryFilters.getByRole("button", { name: "Clear" })).toBeDisabled()
  expect(categoryFilters.getByRole("button", { name: "Clear" })).toBe(categoryFilters.getByRole("button", { name: "Clear" }).parentElement?.lastElementChild)
  expect(categoryFilters.queryByRole("button", { name: "All" })).not.toBeInTheDocument()

  await user.click(categoryCombobox)
  await user.click(screen.getByRole("option", { name: "Export & import" }))
  expect(categoryFilters.getByRole("button", { name: "Remove Export & import" })).toBeVisible()
  expect(within(activity).getAllByRole("listitem")).toHaveLength(1)
  expect(within(activity).getByText("Export completed")).toBeVisible()

  await user.click(categoryCombobox)
  await user.click(screen.getByRole("option", { name: "Computer" }))
  expect(within(activity).getAllByRole("listitem")).toHaveLength(2)
  await user.click(categoryFilters.getByRole("button", { name: "Remove Export & import" }))
  expect(within(activity).getAllByRole("listitem")).toHaveLength(1)
  expect(within(activity).getByText("Stop verified")).toBeVisible()
  await user.click(categoryFilters.getByRole("button", { name: "Clear" }))
  expect(within(activity).getAllByRole("listitem")).toHaveLength(2)

  await user.click(filters.getByRole("button", { name: "Clear" }))
  expect(filters.getByRole("button", { name: "Clear" })).toBeDisabled()
  expect(filters.queryByRole("button", { name: /^Remove / })).not.toBeInTheDocument()
  const allActivity = panel.getByRole("list", { name: "Recent activity" })
  expect(allActivity).toBeVisible()
  expect(within(allActivity).getAllByRole("listitem").map((row) => row.textContent)).toEqual([
    expect.stringContaining("Start verified"),
    expect.stringContaining("Stop verified"),
    expect.stringContaining("Push completed"),
    expect.stringContaining("Export completed"),
  ])
  expect(filters.queryByRole("button", { name: "All" })).not.toBeInTheDocument()
})


it("orders logs newest first independently of computer configuration order", async () => {
  const source = applicationSourceForScenario("running")
  source.computers.reverse()
  const application = renderApplication("running", source)
  const computerSections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))

  await application.user.click(computerSections.getByRole("button", { name: "Logs" }))

  const rows = within(within(appPanel("Computers")).getByRole("table", { name: "Logs" })).getAllByRole("row").slice(1)
  expect(rows.map((row) => row.querySelector("time")?.textContent)).toEqual(["19:18:42", "19:18:40", "19:18:37", "17:02:11", "09:41:02"])
})


it("shows one truthful network table and uses the selected browser for opening ports", async () => {
  const application = renderApplication()
  const navigation = within(appNavigation())

  await application.user.click(navigation.getByRole("button", { name: "Settings" }))
  const settings = within(appPanel("Settings"))
  const browser = settings.getByRole("combobox", { name: "Browser" })
  await application.user.click(browser)
  await application.user.click(screen.getByRole("option", { name: "Firefox" }))

  await application.user.click(navigation.getByRole("button", { name: "Computers" }))
  const computerSections = within(navigation.getByRole("group", { name: "Computer sections" }))
  await application.user.click(computerSections.getByRole("button", { name: "Network" }))

  const panel = within(appPanel("Computers"))
  const network = await panel.findByRole("table", { name: "Network" })
  expect(network.closest('[data-slot="card"]')).toBeNull()
  expect(network).toHaveClass("min-h-0")
  expect(network).not.toHaveClass("flex-1")
  expect(network.parentElement).toHaveClass("max-h-full", "self-start")
  expect(network.querySelector('[data-table-scroll="network"]')).toHaveClass("min-h-0", "overflow-y-auto")
  expect(network.querySelector('[data-table-scroll="network"]')).not.toHaveClass("flex-1")
  expect(within(network).getAllByRole("columnheader")[0].parentElement).toHaveClass("shrink-0")
  expect(network).not.toHaveClass("min-w-[42rem]")
  expect(network.parentElement).not.toHaveClass("overflow-x-auto")
  expect(panel.queryByRole("heading", { name: "Network" })).not.toBeInTheDocument()
  expect(panel.queryByText("Each computer has its own .silo.test address, so the same port can be active in multiple computers.")).not.toBeInTheDocument()
  expect(within(network).queryByText(/^(web|vite|api)$/)).not.toBeInTheDocument()

  const rows = within(network).getAllByRole("row").slice(1)
  expect(rows).toHaveLength(3)
  expect(within(rows[0]).getByText("3000")).toBeVisible()
  expect(within(rows[0]).getByText("Reachable")).toHaveClass("text-success")
  expect(within(rows[0]).getByText("http://127.0.0.1:3000")).toBeVisible()
  expect(within(rows[0]).getByLabelText("dev, Running")).toBeVisible()
  expect(within(rows[1]).getByText("Waiting for service")).toBeVisible()

  expect(rows[0]).toHaveClass("row-hover")
  const open = within(rows[0]).getByRole("button", { name: "Open port 3000 in browser" })
  const actions = open.closest('[role="cell"]') as HTMLElement
  expect(actions).not.toHaveClass("opacity-0", "group-hover/network-row:opacity-100", "group-focus-within/network-row:opacity-100")
  expect(within(rows[1]).getByRole("button", { name: "Copy http://127.0.0.1:5173" })).toBeVisible()
  await application.user.hover(open)
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Open in browser")
  await application.user.unhover(open)

  const copy = vi.spyOn(navigator.clipboard, "writeText")
  await application.user.click(within(rows[0]).getByRole("button", { name: "Copy http://127.0.0.1:3000" }))
  expect(copy).toHaveBeenCalledWith("http://127.0.0.1:3000")
  const copiedURL = within(rows[0]).getByRole("button", { name: "Address copied" })
  expect(copiedURL).toHaveAttribute("data-copy-status", "copied")
  expect(copiedURL.querySelector("svg")).toHaveClass("lucide-check")
})


it("shows the newest activity first and keeps failure context in its row", async () => {
  const application = renderApplication("bootstrap-failure")
  const computerSections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))

  await application.user.click(computerSections.getByRole("button", { name: "Activity" }))

  const activity = within(appPanel("Computers")).getByRole("list", { name: "Recent activity" })
  const firstRow = within(activity).getAllByRole("listitem")[0]
  expect(within(firstRow).getByText("Start failed")).toBeVisible()
  expect(within(firstRow).getByText("Candidate networking did not become ready.")).toBeVisible()
  const time = firstRow.querySelector("time")!
  expect(time).toHaveTextContent(new Date(time.dateTime).toLocaleString(undefined, { dateStyle: "short", timeStyle: "medium" }))
  expect(firstRow.querySelector("svg")).toHaveClass("lucide-circle-alert", "text-destructive")
  expect(within(firstRow).getByLabelText("dev, Failed")).toBeVisible()
})


it("shows all six production-backed activity categories", async () => {
  const source = applicationSourceForScenario("running", undefined, undefined, undefined, undefined, undefined, "catalog")
  const application = renderApplication("running", source)
  const computerSections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))

  await application.user.click(computerSections.getByRole("button", { name: "Activity" }))

  const panel = within(appPanel("Computers"))
  const filters = within(panel.getByRole("group", { name: "Activity category filters" }))
  await application.user.click(filters.getByRole("combobox", { name: "Add category filter" }))
  expect(screen.getAllByRole("option").map(({ textContent }) => textContent)).toEqual([
    "Computer",
    "Git",
    "Export & import",
    "Secrets",
    "GitHub",
    "System",
  ])

  const activity = panel.getByRole("list", { name: "Recent activity" })
  for (const category of ["Computer", "Git", "Export & import", "Secrets", "GitHub", "System"]) {
    expect(within(activity).getAllByLabelText(`Category: ${category}`).length).toBeGreaterThan(0)
  }
  expect(within(activity).getByText("Restart outcome unknown")).toBeVisible()
  expect(within(activity).getByText("Push failed")).toBeVisible()
  expect(within(activity).getByText("Export completed · restart required")).toBeVisible()
  expect(within(activity).getByText("Secret verification failed")).toBeVisible()
  expect(within(activity).getByText("GitHub disconnect incomplete")).toBeVisible()
  expect(within(activity).getByText("Deep check failed")).toBeVisible()
})


it("updates a live activity in place through progress and completion", async () => {
  const sourceAt = (step: number) => applicationSourceForScenario(
    "running",
    undefined,
    undefined,
    undefined,
    undefined,
    undefined,
    "backup-live",
    step,
  )
  const application = renderApplication("running", sourceAt(0))
  const computerSections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))

  await application.user.click(computerSections.getByRole("button", { name: "Activity" }))

  const panel = within(appPanel("Computers"))
  const firstRow = panel.getByRole("list", { name: "Recent activity" }).querySelector<HTMLElement>('[data-activity-id="live-backup"]')
  expect(firstRow).not.toBeNull()
  expect(firstRow).toHaveAttribute("aria-busy", "true")
  expect(firstRow).toHaveTextContent("Preparing export")
  expect(within(firstRow!).getByRole("progressbar")).toHaveAttribute("aria-valuenow", "10")

  application.rerender(<ApplicationPreview source={sourceAt(2)} actions={application.actions} />)
  const progressingRow = panel.getByRole("list", { name: "Recent activity" }).querySelector<HTMLElement>('[data-activity-id="live-backup"]')
  expect(progressingRow).toBe(firstRow)
  expect(progressingRow).toHaveTextContent("Checksumming export file")
  expect(within(progressingRow!).getByRole("progressbar")).toHaveAttribute("aria-valuenow", "75")

  application.rerender(<ApplicationPreview source={sourceAt(4)} actions={application.actions} />)
  const completedRow = panel.getByRole("list", { name: "Recent activity" }).querySelector<HTMLElement>('[data-activity-id="live-backup"]')
  expect(completedRow).toBe(firstRow)
  expect(completedRow).not.toHaveAttribute("aria-busy")
  expect(completedRow).toHaveTextContent("Export completed")
  expect(completedRow?.querySelector("svg")).toHaveClass("lucide-check")
  expect(within(completedRow!).queryByRole("progressbar")).not.toBeInTheDocument()
  expect(panel.getByRole("list", { name: "Recent activity" }).querySelectorAll('[data-activity-id="live-backup"]')).toHaveLength(1)
  expect(within(panel.getByRole("list", { name: "Recent activity" })).getAllByText("Export completed")).toHaveLength(1)
})


it("shows repository push progress inside its row", async () => {
  const source = applicationSourceForScenario("running", undefined, undefined, undefined, undefined, "pushing")
  const application = renderApplication("running", source)
  const computerSections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))

  await application.user.click(computerSections.getByRole("button", { name: "Files" }))
  const row = within(appPanel("Computers")).getByText("silo").closest('[role="listitem"]') as HTMLElement
  expect(within(row).getByRole("status")).toHaveClass("h-6")
  expect(within(row).getByRole("status")).toHaveTextContent("Pushing 2 commits…")
  expect(row).toHaveAttribute("aria-busy", "true")
  expect(within(row).queryByRole("button", { name: /^Push / })).not.toBeInTheDocument()
})


it("clears a push that already succeeded without an inline line", async () => {
  const source = applicationSourceForScenario("running", undefined, undefined, undefined, undefined, "succeeded")
  const application = renderApplication("running", source)
  const computerSections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))

  await application.user.click(computerSections.getByRole("button", { name: "Files" }))
  const row = within(appPanel("Computers")).getByText("silo").closest('[role="listitem"]') as HTMLElement
  expect(within(row).queryByRole("status")).not.toBeInTheDocument()
  expect(row).toHaveTextContent("main · 0 ahead, 0 behind")
})


it("sends unknown-result acknowledgement to the host before showing push again", async () => {
  const source = applicationSourceForScenario("running")
  source.repositoryPushOperations = [{ computer: "dev", repositoryPath: source.computers[0].repositories[0].path, commitCount: 2, status: "unknown", message: "Check this branch on GitHub before retrying." }]
  const dismissRepositoryPush = vi.fn()
  const pushRepository = vi.fn()
  render(<ApplicationPreview source={source} actions={{ dismissRepositoryPush, pushRepository }} initialRoute={{ computer: "dev", computerSection: "files" }} />)
  await userEvent.setup().click(screen.getByRole("button", { name: "I’ve checked GitHub" }))
  expect(dismissRepositoryPush).toHaveBeenCalledExactlyOnceWith("dev", source.computers[0].repositories[0].path)
  expect(pushRepository).not.toHaveBeenCalled()
  expect(screen.getByRole("button", { name: "I’ve checked GitHub" })).toBeVisible()
})


it.each(["stopped", "stale", "busy"])("blocks failed-push retry while the computer is %s", async (condition) => {
  const source = structuredClone(applicationSourceForScenario("running", undefined, undefined, undefined, undefined, "failed"))
  const computer = source.computers.find(computer => computer.configuration.name === "dev")!
  if (condition === "stopped") computer.state = "stopped"
  if (condition === "stale") computer.freshness = "stale"
  if (condition === "busy") computer.checkpointOperation = { kind: "capture", status: "running", stage: "Capturing" }
  const application = renderApplication("running", source)
  await application.user.click(within(within(appNavigation()).getByRole("group", { name: "Computer sections" })).getByRole("button", { name: "Files" }))
  await application.user.click(screen.getByRole("button", { name: "Push failed for acme/silo. Show details" }))
  expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Retry push for acme/silo" })).toBeDisabled()
  expect(application.actions.pushRepository).not.toHaveBeenCalled()
})


it("keeps a failed push as a small in-row label with details and retry", async () => {
  const source = applicationSourceForScenario("running", undefined, undefined, undefined, undefined, "failed")
  const application = renderApplication("running", source)
  const computerSections = within(within(appNavigation()).getByRole("group", { name: "Computer sections" }))

  await application.user.click(computerSections.getByRole("button", { name: "Files" }))
  const row = within(appPanel("Computers")).getByText("silo").closest('[role="listitem"]') as HTMLElement
  expect(within(row).queryByText(/no longer matches/)).not.toBeInTheDocument()

  await application.user.click(within(row).getByRole("button", { name: "Push failed for acme/silo. Show details" }))
  const details = within(screen.getByRole("dialog"))
  expect(details.getByText("Push failed because the remote branch changed.")).toBeVisible()
  expect(details.getByText(/no longer matches/)).toBeVisible()

  await application.user.click(details.getByRole("button", { name: "Retry push for acme/silo" }))
  expect(application.actions.pushRepository).not.toHaveBeenCalled()
  await application.user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Push" }))
  expect(application.actions.pushRepository).toHaveBeenCalledWith("dev", "acme/silo", { repository: "acme/silo", branch: "main", commit: "4f1c2d9e8b7a6c5d4e3f2a1b0c9d8e7f6a5b4c3d" })
})


it.each([
  ["running", "Running", "running", "bg-success"],
  ["starting", "Starting", "starting", "bg-warning"],
  ["stopped", "Stopped", "stopped", "bg-muted-foreground/55"],
  ["error", "Failed", "failed", "bg-destructive"],
] as const)("colors repository computer badges for the %s fixture", async (mode, label, state, className) => {
  const application = renderApplication("running", applicationSourceForScenario("running", undefined, mode satisfies ComputerFixtureMode))
  const navigation = within(appNavigation())
  const computerSections = within(navigation.getByRole("group", { name: "Computer sections" }))

  await application.user.click(computerSections.getByRole("button", { name: "Files" }))
  const repositories = within(appPanel("Computers")).getByRole("list", { name: "Repositories" })
  const badge = within(repositories).getAllByLabelText(`dev, ${label}`)[0]
  expect(badge.querySelector(`[data-computer-state-dot="${state}"]`)).toHaveClass(className)
  application.unmount()
})
