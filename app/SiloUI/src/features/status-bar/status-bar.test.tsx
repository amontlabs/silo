import { act, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import type { ApplicationSource } from "@/features/application/model/application-source"
import { remoteComputerTarget } from "@/features/application/model/connections"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { fixtureDirectoryLoader } from "@/fixtures/directory-loader"
import { StatusBar } from "./status-bar-popover"
import type { StatusBarActions } from "./status-bar-types"

function setup(overrides: Partial<ApplicationSource> = {}) {
  const source = { ...applicationSourceForScenario("complete"), activities: [], ...overrides }
  const actions: StatusBarActions = {
    listComputerDirectory: fixtureDirectoryLoader(source.computers),
    openSilo: vi.fn(), quit: vi.fn(), refresh: vi.fn(), pushRepository: vi.fn(), dismissRepositoryPush: vi.fn(),
    startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn(),
    openTerminal: vi.fn(), openEditor: vi.fn(), openSite: vi.fn(),
  }
  return { user: userEvent.setup(), source, actions, ...render(<StatusBar source={source} actions={actions} defaultOpen />) }
}

describe("status bar", () => {
  it.each(["row", "menu"])("guards a memory-pressure Start from the %s (I-04)", async (surface) => {
    const base = applicationSourceForScenario("complete")
    const { user, actions } = setup({
      computers: base.computers.map(computer => ({ ...computer, state: "stopped" })),
      resourceNotice: { kind: "start-memory", computer: "dev", memoryGiB: 32 },
    })
    if (surface === "menu") {
      await user.click(screen.getByRole("button", { name: "Actions for dev" }))
      await user.click(screen.getByRole("menuitem", { name: "Start" }))
    } else await user.click(screen.getByRole("button", { name: "Start dev" }))
    expect(actions.startComputer).not.toHaveBeenCalled()
    const prompt = screen.getByRole("group", { name: "Starting dev may slow this device" })
    expect(prompt).toHaveTextContent("32 GiB")
    await user.click(within(prompt).getByRole("button", { name: "Start anyway" }))
    expect(actions.startComputer).toHaveBeenCalledExactlyOnceWith("dev")
  })

  it("reports unavailable computer operations at the tray control (I-04)", async () => {
    const base = applicationSourceForScenario("complete")
    const { user, actions } = setup({
      computers: base.computers.map(computer => ({ ...computer, state: "stopped" })),
      computerOperationsUnavailable: "This build cannot run local computers.",
    })
    await user.click(screen.getByRole("button", { name: "Start dev" }))
    expect(actions.startComputer).not.toHaveBeenCalled()
    expect(screen.getByRole("alert", { name: "Computer operation unavailable" })).toHaveTextContent("This build cannot run local computers.")
  })

  it("rechecks current availability before Start anyway (I-04)", async () => {
    const base = applicationSourceForScenario("complete")
    const { user, actions, source, rerender } = setup({
      computers: base.computers.map(computer => ({ ...computer, state: "stopped" })),
      resourceNotice: { kind: "start-memory", computer: "dev", memoryGiB: 32 },
    })
    await user.click(screen.getByRole("button", { name: "Start dev" }))
    rerender(<StatusBar source={{ ...source, computers: source.computers.map(computer => ({ ...computer, freshness: "stale" })) }} actions={actions} defaultOpen />)
    expect(screen.getByRole("button", { name: "Start anyway" })).toBeDisabled()
    expect(actions.startComputer).not.toHaveBeenCalled()
  })

  it.each(["Stop", "Restart"])("guards confirmed %s when local operations become unavailable (I-04)", async (action) => {
    const { user, actions, source, rerender } = setup()
    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    await user.click(screen.getByRole("menuitem", { name: `${action}…` }))
    rerender(<StatusBar source={{ ...source, computerOperationsUnavailable: "Local computers unavailable." }} actions={actions} defaultOpen />)
    await user.click(screen.getByRole("button", { name: action }))
    expect(actions.stopComputer).not.toHaveBeenCalled()
    expect(actions.restartComputer).not.toHaveBeenCalled()
    expect(screen.getByRole("alert", { name: "Computer operation unavailable" })).toHaveTextContent("Local computers unavailable.")
  })

  it("names the device in a remote computer's Stop confirmation", async () => {
    const base = applicationSourceForScenario("complete")
    const device = { id: "office", name: "office-mac", address: "office.local", connected: true, computerId: "vm-1" }
    const { user } = setup({ computers: [{ ...base.computers[0]!, device, state: "running" }] })
    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    await user.click(screen.getByRole("menuitem", { name: "Stop…" }))
    expect(screen.getByText(/Stop dev on office-mac\?/)).toBeVisible()
  })

  it("keeps a remote Start independent of same-named local guards (I-04)", async () => {
    const base = applicationSourceForScenario("complete")
    const device = { id: "office", name: "office-mac", address: "office.local", connected: true, computerId: "vm-1" }
    const { user, actions } = setup({
      computers: [{ ...base.computers[0]!, device, state: "stopped" }],
      computerOperationsUnavailable: "Local computers unavailable.",
      resourceNotice: { kind: "start-memory", computer: "dev", memoryGiB: 32 },
    })
    await user.click(screen.getByRole("button", { name: "Start dev" }))
    expect(actions.startComputer).toHaveBeenCalledExactlyOnceWith(remoteComputerTarget("office", "vm-1"))
    expect(screen.queryByRole("button", { name: "Start anyway" })).not.toBeInTheDocument()
  })

  it("pushes the selected repository and shows source-confirmed progress and success", async () => {
    const { user, actions, source, rerender } = setup()
    const row = within(screen.getByRole("listitem", { name: "dev" }))
    const push = row.getByRole("button", { name: "Push 2 commits for acme/silo in dev" })
    expect(row.queryByText("acme/design-system")).not.toBeInTheDocument()
    await user.click(push)
    // The push names its repository, branch and commit before anything is sent.
    expect(actions.pushRepository).not.toHaveBeenCalled()
    expect(screen.getByText("Push to acme/silo?")).toBeVisible()
    expect(screen.getByText("Branch main · 2 commits · 4f1c2d9")).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Push" }))
    expect(actions.pushRepository).toHaveBeenCalledExactlyOnceWith("dev", "acme/silo", { repository: "acme/silo", branch: "main", commit: "4f1c2d9e8b7a6c5d4e3f2a1b0c9d8e7f6a5b4c3d" })
    expect(screen.getByRole("dialog", { name: "Silo" })).toBeVisible()
    const operation = { computer: "dev", repositoryPath: "acme/silo", commitCount: 2, status: "pushing" as const }
    rerender(<StatusBar source={{ ...source, repositoryPushOperations: [operation] }} actions={actions} defaultOpen />)
    expect(row.getByRole("status")).toHaveTextContent("Pushing 2 commits…")
    expect(row.queryByRole("button", { name: /^Push / })).not.toBeInTheDocument()
    rerender(<StatusBar source={{ ...source, computers: source.computers.map((computer) => ({ ...computer, repositories: computer.repositories.map((repository) => ({ ...repository, ahead: 0 })) })), repositoryPushOperations: [{ ...operation, status: "succeeded" }] }} actions={actions} defaultOpen />)
    expect(row.getByRole("status")).toHaveTextContent("Pushed 2 commits.")
    expect(row.queryByRole("button", { name: /^Push / })).not.toBeInTheDocument()
  })

  it("names a pushable repository by its full path with hidden characters revealed", async () => {
    const source = applicationSourceForScenario("complete")
    const spoofed = "acme/evil\u202Eolis"
    const { user, actions } = setup({ computers: source.computers.map((computer) => ({ ...computer, repositories: [{ ...computer.repositories[0]!, path: spoofed, ahead: 1 }] })) })
    const row = screen.getByRole("group", { name: "acme/evil⟨U+202E⟩olis in dev" })
    expect(row).toHaveTextContent("acme/evil⟨U+202E⟩olis")
    expect(row).not.toHaveTextContent("\u202E")
    await user.click(within(row).getByRole("button", { name: "Push 1 commit for acme/evil⟨U+202E⟩olis in dev" }))
    expect(actions.pushRepository).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Push" }))
    // The confirmed action still targets the repository's real path.
    expect(actions.pushRepository).toHaveBeenCalledExactlyOnceWith("dev", spoofed, { repository: "acme/silo", branch: "main", commit: source.computers[0]!.repositories[0]!.head })
  })

  it("identifies multiple repositories and dispatches only the selected one", async () => {
    const source = applicationSourceForScenario("complete")
    const { user, actions } = setup({ computers: source.computers.map((computer) => ({ ...computer, repositories: computer.repositories.map((repository) => ({ ...repository, ahead: 1 })) })) })
    await user.click(screen.getByRole("button", { name: "Push 1 commit for acme/design-system in dev" }))
    expect(screen.getByText("Branch next · 1 commit · 9a8b7c6")).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Push" }))
    expect(actions.pushRepository).toHaveBeenCalledExactlyOnceWith("dev", "acme/design-system", { repository: "acme/design-system", branch: "next", commit: "9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b" })
    expect(screen.getByRole("button", { name: "Push 1 commit for acme/silo in dev" })).toBeEnabled()
  })

  it("cannot push a repository whose GitHub destination is unknown", () => {
    const source = applicationSourceForScenario("complete")
    setup({ computers: source.computers.map((computer) => ({ ...computer, repositories: computer.repositories.map((repository) => ({ ...repository, repository: null })) })) })
    expect(screen.getByRole("button", { name: "Push 2 commits for acme/silo in dev" })).toBeDisabled()
  })

  it.each(["stopped", "starting", "failed", "stale", "repair"])("keeps the commit count visible but blocks push when %s", (state) => {
    const source = applicationSourceForScenario("complete")
    setup({
      computers: source.computers.map((computer) => ({ ...computer,
        state: state === "stopped" || state === "starting" || state === "failed" ? state : computer.state,
        freshness: state === "stale" ? "stale" : computer.freshness,
      })),
      runtimeRepair: state === "repair" ? { status: "needed", reason: "Runtime not verified" } : null,
    })
    expect(screen.getByRole("button", { name: "Push 2 commits for acme/silo in dev" })).toBeDisabled()
  })

  it("requires acknowledgement of an unknown result without retrying the push", async () => {
    const { user, actions } = setup({ repositoryPushOperations: [{ computer: "dev", repositoryPath: "acme/silo", commitCount: 2, status: "unknown", message: "Silo restarted before recording the result. Check this branch on GitHub before retrying." }] })
    expect(screen.getByRole("button", { name: "Silo status bar" })).toHaveAccessibleDescription("Check push result")
    expect(screen.queryByRole("button", { name: "Retry push for acme/silo" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "I’ve checked GitHub" }))
    expect(actions.dismissRepositoryPush).toHaveBeenCalledExactlyOnceWith("dev", "acme/silo")
    expect(actions.pushRepository).not.toHaveBeenCalled()
  })

  it("keeps a failed push pinned with a retry action", async () => {
    const { user, actions } = setup({ repositoryPushOperations: [{ computer: "dev", repositoryPath: "acme/silo", commitCount: 2, status: "failed", message: "Remote unavailable." }] })
    const issue = screen.getByRole("alert", { name: "Push failed · dev" })
    expect(issue).toHaveTextContent("Remote unavailable.")
    await user.click(within(issue).getByRole("button", { name: "Retry push for acme/silo" }))
    expect(screen.getByText("Push to acme/silo?")).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Push" }))
    expect(actions.pushRepository).toHaveBeenCalledExactlyOnceWith("dev", "acme/silo", { repository: "acme/silo", branch: "main", commit: "4f1c2d9e8b7a6c5d4e3f2a1b0c9d8e7f6a5b4c3d" })
    expect(screen.getByRole("dialog", { name: "Silo" })).toBeVisible()
  })

  it("names a remote computer's failed push by computer and device, not its internal target", async () => {
    const base = applicationSourceForScenario("complete")
    const device = { id: "office", name: "office-mac", address: "office.local", connected: true, computerId: "vm-1" }
    const remote = { ...base.computers[0]!, device }
    const target = remoteComputerTarget("office", "vm-1")
    const { user, actions } = setup({
      computers: [remote],
      repositoryPushOperations: [
        { computer: target, repositoryPath: "acme/silo", commitCount: 2, status: "failed", message: "Remote unavailable." },
        { computer: remoteComputerTarget("office", "gone"), repositoryPath: "acme/old", commitCount: 1, status: "failed", message: "Remote unavailable." },
      ],
      devices: [device],
    })
    const issue = screen.getByRole("alert", { name: "Push failed · dev on office-mac" })
    expect(issue).not.toHaveTextContent("silo-remote")
    // A push whose computer is no longer listed still names its device.
    expect(screen.getByRole("alert", { name: "Push failed · a computer on office-mac" })).not.toHaveTextContent("silo-remote")
    await user.click(within(issue).getByRole("button", { name: "Review push failure for dev on office-mac, acme/silo" }))
    expect(actions.openSilo).toHaveBeenCalledWith({ computer: target, computerSection: "files" })
  })

  it("shows a failed start as an error instead of a neutral stopped computer", () => {
    const source = applicationSourceForScenario("complete")
    setup({ computers: source.computers.map((computer) => ({ ...computer, state: "stopped" as const, lifecycleFailure: "Not enough memory to start dev.", lifecycleFailureAction: "start" as const })) })
    expect(screen.getByRole("button", { name: "Silo status bar" })).toHaveAccessibleDescription("Computer error")
    const row = screen.getByRole("listitem", { name: "dev" })
    expect(row).toHaveTextContent("Start failed · Not enough memory to start dev.")
    expect(row.querySelector("[data-computer-row-tone]")).toHaveAttribute("data-computer-row-tone", "error")
    // Start stays available as the retry.
    expect(within(row).getByRole("button", { name: "Start dev" })).toBeEnabled()
  })

  it("keeps a cancelled start neutral", () => {
    const source = applicationSourceForScenario("complete")
    setup({ computers: source.computers.map((computer) => ({ ...computer, state: "stopped" as const, lifecycleFailure: "Start was cancelled.", lifecycleFailureAction: "start" as const, lifecycleFailureCancelled: true })) })
    expect(screen.getByRole("button", { name: "Silo status bar" })).toHaveAccessibleDescription("All computers stopped")
    const row = screen.getByRole("listitem", { name: "dev" })
    expect(row).toHaveTextContent("Start cancelled")
    expect(row.querySelector("[data-computer-row-tone]")).not.toHaveAttribute("data-computer-row-tone", "error")
  })

  it("shows computer changes waiting for approval with a way to review them", async () => {
    const operation = { id: "op", status: "awaiting-approval" as const, candidate: { schemaVersion: 1 as const, computers: [] }, progressEvents: [], error: null, result: { resumed: false, phase: "computers", requiresApproval: true, vmsStarted: false, message: "Approve the new computer to finish setting it up." } }
    const { user, actions } = setup({ computerConfigurationOperation: operation })
    expect(screen.getByRole("button", { name: "Silo status bar" })).toHaveAccessibleDescription("Approval needed")
    const notice = screen.getByRole("status", { name: "Computer changes need approval" })
    expect(notice).toHaveTextContent("Approve the new computer to finish setting it up.")
    await user.click(within(notice).getByRole("button", { name: "Review computer changes" }))
    expect(actions.openSilo).toHaveBeenCalledWith({ computerSection: "overview" })
  })

  it("updates the menu bar icon from loading to warning, error, and ready", () => {
    const { source, actions, rerender } = setup()
    const trigger = screen.getByRole("button", { name: "Silo status bar" })
    const pushing = { ...source, repositoryPushOperations: [{ computer: "dev", repositoryPath: "acme/silo", commitCount: 1, status: "pushing" as const }] }
    rerender(<StatusBar source={pushing} actions={actions} />)
    expect(trigger).toHaveAccessibleDescription("Working…")
    expect(trigger.querySelector(".animate-spin")).toBeInTheDocument()
    const warning = { ...pushing, computers: source.computers.map((computer) => ({ ...computer, freshness: "stale" as const })) }
    rerender(<StatusBar source={warning} actions={actions} />)
    expect(trigger).toHaveAccessibleDescription("Last known status")
    expect(trigger.querySelector(".lucide-triangle-alert")).toBeInTheDocument()
    expect(trigger.querySelector(".animate-spin")).not.toBeInTheDocument()
    const failed = { ...warning, computers: warning.computers.map((computer) => ({ ...computer, state: "failed" as const })) }
    rerender(<StatusBar source={failed} actions={actions} />)
    expect(trigger.querySelector(".lucide-circle-alert")).toBeInTheDocument()
    expect(trigger.querySelector(".lucide-triangle-alert")).not.toBeInTheDocument()
    rerender(<StatusBar source={source} actions={actions} />)
    expect(trigger).toHaveAccessibleDescription("Ready")
    expect(trigger.querySelectorAll("svg")).toHaveLength(1)
  })

  it("keeps the runtime failure visible while retrying checks", () => {
    const fixture = applicationSourceForScenario("running", undefined, undefined, undefined, "checking")
    const { source, actions, rerender } = setup({ ...fixture, preferences: { ...fixture.preferences, reduceMotion: true } })
    const trigger = screen.getByRole("button", { name: "Silo status bar" })
    expect(trigger).toHaveAccessibleDescription("System issue")
    expect(trigger.querySelector(".lucide-circle-alert")).toBeInTheDocument()
    expect(trigger.querySelector(".animate-spin")).not.toBeInTheDocument()
    rerender(<StatusBar source={{ ...source, runtimeRepair: null, computers: [] }} actions={actions} />)
    expect(trigger).toHaveAccessibleDescription("No computers")
  })

  it("shows shared computer status and opens the configured terminal, dismissing the popover", async () => {
    const { user, actions } = setup()
    expect(screen.getByRole("dialog", { name: "Silo" })).toBeInTheDocument()
    expect(screen.getByRole("dialog", { name: "Silo" })).toHaveFocus()
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "View error details" })).not.toBeInTheDocument()
    expect(screen.getByRole("note", { name: "Restart required for dev" })).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Open dev in Terminal" }))
    expect(actions.openTerminal).toHaveBeenCalledWith("dev")
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Silo status bar" })).toHaveFocus()
  })

  it("requires a deliberate confirmation before stop and rechecks availability", async () => {
    const { user, actions, source, rerender } = setup()
    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    await user.click(screen.getByRole("menuitem", { name: "Stop…" }))
    const confirmation = screen.getByRole("group", { name: "Stop dev?" })
    expect(actions.stopComputer).not.toHaveBeenCalled()
    await user.click(within(confirmation).getByRole("button", { name: "Cancel" }))
    expect(screen.queryByRole("group", { name: "Stop dev?" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    await user.click(screen.getByRole("menuitem", { name: "Stop…" }))
    const stale = { ...source, computers: source.computers.map((computer) => ({ ...computer, freshness: "stale" as const })) }
    rerender(<StatusBar source={stale} actions={actions} defaultOpen />)
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled()
    rerender(<StatusBar source={source} actions={actions} defaultOpen />)
    await user.click(screen.getByRole("button", { name: "Stop" }))
    expect(actions.stopComputer).toHaveBeenCalledExactlyOnceWith("dev")
  })

  it("blocks terminal and lifecycle actions for stale status and offers retry", async () => {
    const source = applicationSourceForScenario("complete")
    const { user, actions } = setup({ computers: source.computers.map((computer) => ({ ...computer, freshness: "stale" })) })
    expect(screen.getByRole("listitem", { name: "dev" })).toHaveTextContent("Last known status")
    expect(screen.queryByRole("button", { name: "Open dev in Terminal" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Retry dev status" }))
    expect(actions.refresh).toHaveBeenCalledOnce()
    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    expect(screen.getByRole("menuitem", { name: "Open in Terminal" })).toHaveAttribute("data-disabled")
    expect(screen.getByRole("menuitem", { name: "Restart…" })).toHaveAttribute("data-disabled")
  })

  it("opens repair in the app and prevents actions while repair is pending", async () => {
    const { user, actions } = setup({ runtimeRepair: { status: "needed", reason: "Runtime not verified" } })
    expect(screen.getByText("System issue")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Open dev in Terminal" })).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: /^See logs for / })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "View issue" }))
    expect(actions.openSilo).toHaveBeenCalledWith({ tab: "system" })
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("explains a failed configuration for a computer that is not in the committed list", async () => {
    const fixture = applicationSourceForScenario("running", undefined, undefined, "computer-error")
    const { user, actions } = setup({ computerConfigurationOperation: fixture.computerConfigurationOperation })
    expect(screen.queryByRole("listitem", { name: "scratch" })).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "View error details" })).not.toBeInTheDocument()
    const issue = screen.getByRole("alert", { name: "Computer changes failed" })
    expect(issue).toHaveTextContent("Networking failed for 'scratch'.")
    await user.click(within(issue).getByRole("button", { name: "Review computer changes" }))
    expect(actions.openSilo).toHaveBeenCalledWith({ computerSection: "overview" })
  })

  it("keeps setup diagnostics behind the tray's error disclosure", async () => {
    const fixture = structuredClone(applicationSourceForScenario("running", undefined, undefined, "computer-error"))
    const operation = fixture.computerConfigurationOperation!
    operation.progressEvents = [...operation.progressEvents, { schemaVersion: 1, type: "progress", requestId: operation.id, phase: "computers", step: "setup-failed", computer: "scratch", safeForDisplay: true, message: "Setup failed. Retry setup.", diagnostic: "Exit code 13\nPermission denied" }]
    const { user } = setup({ computerConfigurationOperation: operation })
    const issue = screen.getByRole("alert", { name: "Computer changes failed" })
    expect(issue).toHaveTextContent("Networking failed for 'scratch'.")
    expect(within(issue).queryByText(/Exit code 13/)).not.toBeInTheDocument()
    await user.click(within(issue).getByRole("button", { name: "Show details" }))
    expect(within(issue).getByLabelText("Error details")).toHaveTextContent("Exit code 13")
    expect(within(issue).getByRole("button", { name: "Copy details" })).toBeVisible()
  })

  it("shows a failed push beside its details action and clears it when the source resolves", async () => {
    const operation = { computer: "dev", repositoryPath: "acme/silo", commitCount: 2, status: "failed" as const, message: "The remote branch changed." }
    const { user, actions, source, rerender } = setup({ repositoryPushOperations: [operation] })
    const issue = screen.getByRole("alert", { name: "Push failed · dev" })
    expect(issue).toHaveTextContent("acme/silo · The remote branch changed.")
    expect(screen.queryByRole("button", { name: "View error details" })).not.toBeInTheDocument()
    await user.click(within(issue).getByRole("button", { name: "Review push failure for dev, acme/silo" }))
    expect(actions.openSilo).toHaveBeenCalledWith({ computer: "dev", computerSection: "files" })
    rerender(<StatusBar source={{ ...source, repositoryPushOperations: [{ ...operation, status: "succeeded" }] }} actions={actions} defaultOpen />)
    await user.click(screen.getByRole("button", { name: "Silo status bar" }))
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })

  it("keeps the configuration icon during progress and blocks repeated actions", () => {
    const source = applicationSourceForScenario("complete")
    setup({ computers: source.computers.map((computer) => ({ ...computer, state: "starting", stateDetail: "Starting…" })) })
    const row = screen.getByRole("listitem", { name: "dev" })
    expect(row).toHaveAttribute("aria-busy", "true")
    expect(row.querySelector(".lucide-monitor")).toBeInTheDocument()
    expect(row.querySelector(".animate-spin")).toBeInTheDocument()
    expect(within(row).queryByRole("button", { name: "Start dev" })).not.toBeInTheDocument()
  })

  it("navigates folders, filters within the current folder and opens the exact path", async () => {
    const { user, actions } = setup()
    await user.click(screen.getByRole("button", { name: "Open dev in Visual Studio Code" }))
    expect(screen.queryByText(".gitconfig")).not.toBeInTheDocument()
    await user.type(screen.getByRole("textbox", { name: "Filter folders" }), "missing")
    expect(screen.getByRole("status")).toHaveTextContent("No matching folders")
    await user.clear(screen.getByRole("textbox", { name: "Filter folders" }))
    await user.click(await screen.findByRole("button", { name: "projects" }))
    await user.click(await screen.findByRole("button", { name: "silo" }))
    await user.click(screen.getByRole("button", { name: "Open in Visual Studio Code" }))
    expect(actions.openEditor).toHaveBeenCalledWith("dev", "/workspace/projects/silo")
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("supports keyboard navigation to the editor picker", async () => {
    const { user } = setup()
    screen.getByRole("button", { name: "Actions for dev" }).focus()
    await user.keyboard("{Enter}")
    expect(screen.queryByRole("menuitem", { name: "Files" })).not.toBeInTheDocument()
    expect(screen.queryByRole("menuitem", { name: "Open Silo" })).not.toBeInTheDocument()
    await user.keyboard("{End}{ArrowUp}{Enter}")
    expect(screen.getByRole("heading", { name: "dev folders" })).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Back to computers" })).toHaveFocus()
  })

  it("offers only listening sites in numeric order and opens the selected port", async () => {
    const source = applicationSourceForScenario("complete")
    const { user, actions } = setup({ computers: source.computers.map((computer) => ({ ...computer, ports: [{ port: 8080, listening: true, configured: true, hostPort: 18080, scheme: "http" }, { port: 3000, listening: true, configured: true, hostPort: 13000, scheme: "http" }, { port: 5173, listening: false }] })) })
    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    act(() => screen.getByRole("menuitem", { name: "Open in browser" }).focus())
    await user.keyboard("{ArrowRight}")
    const ports = screen.getAllByRole("menuitem", { name: /^Port / })
    expect(ports.map((port) => port.textContent)).toEqual(["Port 3000", "Port 8080"])
    expect(screen.queryByRole("menuitem", { name: "Port 5173" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("menuitem", { name: "Port 8080" }))
    expect(actions.openSite).toHaveBeenCalledWith("dev", 8080)
  })

  it("copies the base URL without a port and keeps copy feedback in the site menu", async () => {
    const { user, actions } = setup()
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined)
    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    act(() => screen.getByRole("menuitem", { name: "Open in browser" }).focus())
    await user.keyboard("{ArrowRight}")
    expect(screen.queryByRole("menuitem", { name: "Choose port…" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("menuitem", { name: "Copy port 3000 address" }))
    expect(writeText).toHaveBeenCalledExactlyOnceWith("http://127.0.0.1:3000")
    expect(screen.getByRole("menuitem", { name: "Port 3000 address copied" })).toHaveTextContent("Copied")
    expect(actions.openSilo).not.toHaveBeenCalled()
    expect(actions.openSite).not.toHaveBeenCalled()
    writeText.mockRestore()
  })

  it("lets the user retry a failed URL copy with the keyboard", async () => {
    const { user } = setup()
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockRejectedValueOnce(new Error("Clipboard unavailable")).mockResolvedValue(undefined)
    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    act(() => screen.getByRole("menuitem", { name: "Open in browser" }).focus())
    await user.keyboard("{ArrowRight}{End}{Enter}")
    expect(screen.getByRole("menuitem", { name: "Could not copy port 3000 address" })).toHaveTextContent("Copy failed")
    await user.keyboard("{Enter}")
    expect(writeText).toHaveBeenNthCalledWith(2, "http://127.0.0.1:3000")
    expect(screen.getByRole("menuitem", { name: "Port 3000 address copied" })).toHaveFocus()
    writeText.mockRestore()
  })

  it("supports escape dismissal and an empty computer list", async () => {
    const { user, actions } = setup({ computers: [] })
    expect(screen.getByText("No computers yet")).toBeInTheDocument()
    await user.keyboard("{Escape}")
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Silo status bar" }))
    await user.click(screen.getByRole("button", { name: "Quit Silo" }))
    expect(actions.quit).toHaveBeenCalledOnce()
  })
})
