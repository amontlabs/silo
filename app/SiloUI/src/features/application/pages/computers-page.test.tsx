import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions, ApplicationActivity, ApplicationSource } from "../model/application-source"
import { createDirectoryStore, type DirectoryPage } from "../model/directory-store"
import { computerTarget } from "../model/connections"
import { ComputersPage } from "./computers-page"
import { createFixtureMacosComputersStore } from "@/fixtures/macos-computers"
import { MacosComputersContext } from "@/features/macos-computers/model/macos-computers"

function activity(id: string, computer: string): ApplicationActivity {
  return { id, category: "computer", title: `Event ${id}`, detail: "", occurredAt: "2026-09-29T10:00:00.000Z", time: "10:00", tone: "danger", status: "completed", computer }
}

function renderActivity(selectedComputerIds: ReadonlySet<string>) {
  const source = structuredClone(applicationSourceForScenario("complete"))
  const kept = source.computers[0]
  const activities = [activity("deleted", "removed-computer"), activity("kept", computerTarget(kept))]
  render(<ComputersPage
    source={source} section="activity" computers={source.computers} activities={activities} selectedComputerIds={selectedComputerIds}
    networkActions={{} as ApplicationActions} onSectionChange={vi.fn()} editor="Editor" onOpenEditor={vi.fn()}
    directoryStore={createDirectoryStore(vi.fn())} active logQuery="" repositoryPushOperations={[]} browser="Browser"
    onComputerFilterChange={vi.fn()} onLogQueryChange={vi.fn()} onPushRepository={vi.fn()} onDismissRepositoryPush={vi.fn()}
  />)
  return kept
}

it("keeps a deleted computer's activity when no computer filter is selected", () => {
  renderActivity(new Set())
  const list = screen.getByRole("list", { name: "Recent activity" })
  expect(within(list).getByText("Event deleted")).toBeVisible()
  expect(within(list).getByLabelText("Computer: removed-computer")).toBeVisible()
  expect(within(list).getByText("Event kept")).toBeVisible()
})

it.each([false, true])("keeps a failed activity's runtime output behind Details (separate diagnostic: %s)", async (separate) => {
  const source = structuredClone(applicationSourceForScenario("complete"))
  const diagnostic = `${Array.from({ length: 20 }, (_, index) => `stderr line ${index}`).join("\n")}\n[Diagnostic truncated]`
  const failed = { ...activity("failed", computerTarget(source.computers[0])), title: "Start failed", detail: separate ? "The computer did not boot." : `The computer did not boot.\n${diagnostic}`, diagnostic: separate ? diagnostic : undefined }
  render(<ComputersPage
    source={source} section="activity" computers={source.computers} activities={[failed]} selectedComputerIds={new Set()}
    networkActions={{} as ApplicationActions} onSectionChange={vi.fn()} editor="Editor" onOpenEditor={vi.fn()}
    directoryStore={createDirectoryStore()} active logQuery="" repositoryPushOperations={[]} browser="Browser"
    onComputerFilterChange={vi.fn()} onLogQueryChange={vi.fn()} onPushRepository={vi.fn()} onDismissRepositoryPush={vi.fn()}
  />)
  const list = within(screen.getByRole("list", { name: "Recent activity" }))
  expect(list.getByText("The computer did not boot.")).toBeVisible()
  expect(list.queryByText(/stderr line 19/)).not.toBeInTheDocument()
  expect(list.getByRole("button", { name: "Show details" })).toBeVisible()
  await userEvent.setup().click(list.getByRole("button", { name: "Show details" }))
  expect(list.getByLabelText("Error details")).toHaveTextContent("stderr line 19")
})

it("hides other computers' activity when a computer filter is selected", () => {
  const source = applicationSourceForScenario("complete")
  renderActivity(new Set([source.computers[0].configuration.id]))
  const list = screen.getByRole("list", { name: "Recent activity" })
  expect(within(list).queryByText("Event deleted")).not.toBeInTheDocument()
  expect(within(list).getByText("Event kept")).toBeVisible()
})

function renderFiles(source: ApplicationSource, onPushRepository = vi.fn()) {
  render(<ComputersPage
    source={source} section="files" computers={source.computers} activities={[]} selectedComputerIds={new Set()}
    networkActions={{} as ApplicationActions} onSectionChange={vi.fn()} editor="Editor" onOpenEditor={vi.fn()}
    directoryStore={createDirectoryStore(() => new Promise<DirectoryPage>(() => {}))} active logQuery="" repositoryPushOperations={[]} browser="Browser"
    onComputerFilterChange={vi.fn()} onLogQueryChange={vi.fn()} onPushRepository={onPushRepository} onDismissRepositoryPush={vi.fn()}
  />)
  return onPushRepository
}

function filesSource() {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.repositoryPushOperations = []
  return source
}

it("names each Push button by repository and computer and enables it only for an available computer", async () => {
  const source = filesSource()
  const playgrounds = source.computers.find(({ configuration }) => configuration.name === "playgrounds")!
  playgrounds.state = "stopped"
  playgrounds.repositories = [{ ...playgrounds.repositories[0], path: "acme/silo", ahead: 2 }]
  const dev = source.computers.find(({ configuration }) => configuration.name === "dev")!
  const target = { repository: "acme/silo", branch: "main", commit: "b".repeat(40) }
  dev.repositories = dev.repositories.map(repository => repository.path === "acme/silo" ? { ...repository, branch: target.branch, repository: target.repository, head: target.commit } : repository)
  const onPush = renderFiles(source)
  const repositories = within(screen.getByRole("list", { name: "Repositories" }))
  const running = repositories.getByRole("button", { name: "Push 2 commits for acme/silo in dev" })
  expect(running).toBeEnabled()
  expect(repositories.getByRole("button", { name: "Push 2 commits for acme/silo in playgrounds" })).toBeDisabled()
  const user = userEvent.setup()
  await user.click(running)
  // Pushing asks to confirm the repository, branch and commit first (B-01).
  await user.click(await screen.findByRole("button", { name: "Push" }))
  expect(onPush).toHaveBeenCalledWith("dev", "acme/silo", 2, target)
})

it("disables Push while the computer is stale", () => {
  const source = filesSource()
  source.computers.find(({ configuration }) => configuration.name === "dev")!.freshness = "stale"
  renderFiles(source)
  expect(screen.getByRole("button", { name: "Push 2 commits for acme/silo in dev" })).toBeDisabled()
})

it.each(["files", "logs", "network"] as const)("offers to create a computer on %s when there are none, instead of asking to select one", async (section) => {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.computers = []
  const onCreateComputer = vi.fn()
  render(<ComputersPage
    source={source} section={section} computers={[]} activities={[]} selectedComputerIds={new Set()}
    networkActions={{} as ApplicationActions} onSectionChange={vi.fn()} editor="Editor" onOpenEditor={vi.fn()}
    directoryStore={createDirectoryStore()} active logQuery="" repositoryPushOperations={[]} browser="Browser"
    onComputerFilterChange={vi.fn()} onLogQueryChange={vi.fn()} onPushRepository={vi.fn()} onDismissRepositoryPush={vi.fn()}
    onCreateComputer={onCreateComputer}
  />)
  expect(screen.getByText("No computers yet")).toBeVisible()
  expect(screen.queryByText(/No computers selected|Select at least one computer/)).not.toBeInTheDocument()
  expect(screen.queryByRole("combobox", { name: "Filter computers" })).not.toBeInTheDocument()
  await userEvent.setup().click(screen.getByRole("button", { name: "New computer" }))
  expect(onCreateComputer).toHaveBeenCalledOnce()
})

it("keeps showing activity when every computer has been deleted", () => {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.computers = []
  render(<ComputersPage
    source={source} section="activity" computers={[]} activities={[activity("deleted", "removed-computer")]} selectedComputerIds={new Set()}
    networkActions={{} as ApplicationActions} onSectionChange={vi.fn()} editor="Editor" onOpenEditor={vi.fn()}
    directoryStore={createDirectoryStore()} active logQuery="" repositoryPushOperations={[]} browser="Browser"
    onComputerFilterChange={vi.fn()} onLogQueryChange={vi.fn()} onPushRepository={vi.fn()} onDismissRepositoryPush={vi.fn()}
  />)
  expect(within(screen.getByRole("list", { name: "Recent activity" })).getByText("Event deleted")).toBeVisible()
  expect(screen.queryByText("No computers yet")).not.toBeInTheDocument()
})

it("names a remote computer's Push button by its device", () => {
  const source = filesSource()
  const dev = source.computers.find(({ configuration }) => configuration.name === "dev")!
  dev.device = { id: "office", name: "Office Mac", address: "office.local", connected: true, computerId: "remote-dev" }
  renderFiles(source)
  expect(screen.getByRole("button", { name: "Push 2 commits for acme/silo in dev on Office Mac" })).toBeEnabled()
})

it("lists a macOS computer on the Logs page and reads its logs by id", async () => {
  const source = structuredClone(applicationSourceForScenario("complete"))
  const queryLogs = vi.fn(async () => ({ entries: [], nextCursor: null, oldestAvailableTimestamp: null, newestAvailableTimestamp: null, totalMatches: 0, timestampEstimated: false }))
  render(
    <MacosComputersContext.Provider value={createFixtureMacosComputersStore()}>
      <ComputersPage
        source={source} section="logs" computers={source.computers} activities={[]} selectedComputerIds={new Set(["mac-running"])}
        networkActions={{ queryLogs } as unknown as ApplicationActions} onSectionChange={vi.fn()} editor="Editor" onOpenEditor={vi.fn()}
        directoryStore={createDirectoryStore()} active logQuery="" repositoryPushOperations={[]} browser="Browser"
        onComputerFilterChange={vi.fn()} onLogQueryChange={vi.fn()} onPushRepository={vi.fn()} onDismissRepositoryPush={vi.fn()}
      />
    </MacosComputersContext.Provider>,
  )
  await waitFor(() => expect(queryLogs).toHaveBeenCalledWith(expect.objectContaining({ computerId: "mac-running" })))
  expect(queryLogs).not.toHaveBeenCalledWith(expect.objectContaining({ computerId: source.computers[0].configuration.id }))
})

it("ignores a macOS computer selected on the Logs page in the other sections", async () => {
  const source = structuredClone(applicationSourceForScenario("complete"))
  render(
    <MacosComputersContext.Provider value={createFixtureMacosComputersStore()}>
      <ComputersPage
        source={source} section="activity" computers={source.computers} activities={[activity("kept", computerTarget(source.computers[0]))]} selectedComputerIds={new Set(["mac-running"])}
        networkActions={{} as ApplicationActions} onSectionChange={vi.fn()} editor="Editor" onOpenEditor={vi.fn()}
        directoryStore={createDirectoryStore()} active logQuery="" repositoryPushOperations={[]} browser="Browser"
        onComputerFilterChange={vi.fn()} onLogQueryChange={vi.fn()} onPushRepository={vi.fn()} onDismissRepositoryPush={vi.fn()}
      />
    </MacosComputersContext.Provider>,
  )
  await act(async () => { await Promise.resolve() })
  expect(within(screen.getByRole("list", { name: "Recent activity" })).getByText("Event kept")).toBeVisible()
})
