import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import { setupFakeTimerUser } from "@/test/fake-timer-user"

import { FixtureApp } from "./fixture-app"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { StatusBarPreview } from "@/fixtures/status-bar-preview"
import { statusBarFixtureModeFromSearch, statusBarSourceForFixture } from "@/fixtures/status-bar-scenarios"

const originalURL = window.location.href
afterEach(() => {
  window.history.replaceState(null, "", originalURL)
  vi.useRealTimers()
})

/** Every push is confirmed first; the fixture push starts once the confirmation settles. */
async function confirmPush(name: string) {
  fireEvent.click(screen.getByRole("button", { name }))
  fireEvent.click(screen.getByRole("button", { name: "Push" }))
  await act(async () => {})
}

describe("status bar preview", () => {
  it("adds bounded stale, empty, and long-list fixtures without changing the source", () => {
    const source = applicationSourceForScenario("running")
    expect(statusBarFixtureModeFromSearch("?status-bar=unknown")).toBeUndefined()
    expect(statusBarFixtureModeFromSearch("?status-bar=stale")).toBe("stale")
    expect(statusBarSourceForFixture(source, "stale").computers.every(({ freshness }) => freshness === "stale")).toBe(true)
    expect(statusBarSourceForFixture(source, "empty").computers).toHaveLength(0)
    const longList = statusBarSourceForFixture(source, "long-list").computers
    expect(longList).toHaveLength(10)
    expect(new Set(longList.map(({ configuration }) => configuration.id)).size).toBe(10)
    expect(new Set(longList.map(({ configuration }) => configuration.name)).size).toBe(10)
    expect(source.computers).toHaveLength(3)
    expect(source.computers.every(({ freshness }) => freshness === "fresh")).toBe(true)
  })

  it("simulates lifecycle actions and hands the resulting snapshot to the application", async () => {
    vi.useFakeTimers()
    const user = setupFakeTimerUser()
    const source = applicationSourceForScenario("running")
    const onOpenSilo = vi.fn()
    render(<StatusBarPreview source={source} onOpenSilo={onOpenSilo} />)

    await user.click(screen.getByRole("button", { name: "Start playgrounds" }))
    expect(screen.getByRole("listitem", { name: "playgrounds" })).toHaveAttribute("aria-busy", "true")
    await act(async () => { await vi.advanceTimersByTimeAsync(900) })
    expect(within(screen.getByRole("listitem", { name: "playgrounds" })).getByText("Running", { exact: true })).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Actions for playgrounds" }))
    await user.click(screen.getByRole("menuitem", { name: "Stop…" }))
    await user.click(screen.getByRole("button", { name: "Stop" }))
    expect(screen.getByText("Stopping playgrounds…")).toBeVisible()
    await act(async () => { await vi.advanceTimersByTimeAsync(900) })
    expect(within(screen.getByRole("listitem", { name: "playgrounds" })).getByText("Stopped", { exact: true })).toBeVisible()

    await user.click(screen.getByRole("button", { name: "Actions for dev" }))
    await user.click(screen.getByRole("menuitem", { name: "Restart…" }))
    await user.click(screen.getByRole("button", { name: "Restart" }))
    expect(screen.getByText("Restarting dev…")).toBeVisible()
    await act(async () => { await vi.advanceTimersByTimeAsync(900) })
    expect(screen.getByRole("complementary", { name: "Preview feedback" })).toHaveTextContent("Preview: dev restarted.")
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(onOpenSilo).toHaveBeenCalledWith(expect.objectContaining({
      computers: expect.arrayContaining([
        expect.objectContaining({ configuration: expect.objectContaining({ name: "dev" }), state: "running", stateDetail: "Running" }),
        expect.objectContaining({ configuration: expect.objectContaining({ name: "playgrounds" }), state: "stopped", stateDetail: "Stopped" }),
      ]),
    }), undefined)
    expect(source.computers[0].stateDetail).toBe("Running for 2h 18m")
  })

  it("pushes the pending commits, clears success feedback, and hands off the updated repository", async () => {
    vi.useFakeTimers()
    const source = applicationSourceForScenario("running")
    const onOpenSilo = vi.fn()
    const preview = render(<StatusBarPreview source={source} onOpenSilo={onOpenSilo} />)
    try {
      await confirmPush("Push 2 commits for acme/silo in dev")
      expect(screen.getByText("Pushing 2 commits…")).toBeVisible()
      expect(screen.queryByRole("button", { name: "Push 2 commits for acme/silo in dev" })).not.toBeInTheDocument()
      expect(screen.getByRole("dialog", { name: "Silo" })).toBeVisible()

      await act(async () => { await vi.advanceTimersByTimeAsync(900) })
      expect(screen.getByText("Pushed 2 commits.")).toBeVisible()
      expect(screen.queryByText("Pushing 2 commits…")).not.toBeInTheDocument()
      await act(async () => { await vi.advanceTimersByTimeAsync(4_000) })
      expect(screen.queryByText("Pushed 2 commits.")).not.toBeInTheDocument()
      expect(screen.queryByRole("button", { name: "Push 2 commits for acme/silo in dev" })).not.toBeInTheDocument()

      fireEvent.click(screen.getByRole("button", { name: "Open Silo" }))
      const result = onOpenSilo.mock.calls[0][0]
      expect(result.computers[0].repositories).toEqual([
        { ...source.computers[0].repositories[0], ahead: 0 },
        source.computers[0].repositories[1],
      ])
      expect(result.repositoryPushOperations).toEqual([])
      expect(source.computers[0].repositories[0].ahead).toBe(2)
    } finally {
      preview.unmount()
      vi.useRealTimers()
    }
  })

  it.each(["handoff", "quit"])("settles concurrent pushes before an early %s without changing unrelated operations", async (action) => {
    vi.useFakeTimers()
    const base = applicationSourceForScenario("running")
    const unrelated = { computer: "personal", repositoryPath: "taylor/docs-site", commitCount: 1, status: "failed" as const, message: "Remote unavailable." }
    const source = {
      ...base,
      computers: base.computers.map((computer) => computer.configuration.name === "dev" ? {
        ...computer,
        repositories: computer.repositories.map((repository) => repository.path === "acme/design-system" ? { ...repository, ahead: 3 } : repository),
      } : computer),
      repositoryPushOperations: [unrelated],
    }
    const onOpenSilo = vi.fn()
    const preview = render(<StatusBarPreview source={source} onOpenSilo={onOpenSilo} />)
    try {
      await confirmPush("Push 2 commits for acme/silo in dev")
      await confirmPush("Push 3 commits for acme/design-system in dev")
      fireEvent.click(screen.getByRole("button", { name: "Start playgrounds" }))
      expect(screen.getByText("Pushing 2 commits…")).toBeVisible()
      expect(screen.getByText("Pushing 3 commits…")).toBeVisible()
      if (action === "quit") {
        fireEvent.click(screen.getByRole("button", { name: "Quit Silo" }))
        // Running computers ask for confirmation first (I-06).
        const confirmQuit = screen.queryByRole("button", { name: "Stop and quit" })
        if (confirmQuit) fireEvent.click(confirmQuit)
        fireEvent.click(screen.getByRole("button", { name: "Relaunch Silo" }))
        expect(screen.getByText("Pushed 2 commits.")).toBeVisible()
        expect(screen.getByText("Pushed 3 commits.")).toBeVisible()
      }
      fireEvent.click(screen.getByRole("button", { name: "Open Silo" }))
      const result = onOpenSilo.mock.calls[0][0]
      expect(result.computers[0].repositories.map(({ ahead }: { ahead: number }) => ahead)).toEqual([0, 0])
      expect(result.computers[1].state).toBe("running")
      expect(result.computers[2]).toEqual(source.computers[2])
      expect(result.repositoryPushOperations).toEqual([
        unrelated,
        { computer: "dev", repositoryPath: "acme/silo", commitCount: 2, status: "succeeded" },
        { computer: "dev", repositoryPath: "acme/design-system", commitCount: 3, status: "succeeded" },
      ])
      await act(async () => { await vi.advanceTimersByTimeAsync(900) })
      expect(onOpenSilo).toHaveBeenCalledTimes(1)
    } finally {
      preview.unmount()
      vi.useRealTimers()
    }
  })

  it("retries a failed push and replaces the error with progress and success", async () => {
    vi.useFakeTimers()
    const source = applicationSourceForScenario("running", undefined, undefined, undefined, undefined, "failed")
    const onOpenSilo = vi.fn()
    const preview = render(<StatusBarPreview source={source} onOpenSilo={onOpenSilo} />)
    try {
      expect(screen.getByText(/Push failed because the remote branch changed\./)).toBeVisible()
      await confirmPush("Retry push for acme/silo")
      expect(screen.queryByText(/Push failed because the remote branch changed\./)).not.toBeInTheDocument()
      expect(screen.getByText("Pushing 2 commits…")).toBeVisible()
      await act(async () => { await vi.advanceTimersByTimeAsync(900) })
      expect(screen.getByText("Pushed 2 commits.")).toBeVisible()
      fireEvent.click(screen.getByRole("button", { name: "Open Silo" }))
      expect(onOpenSilo.mock.calls[0][0].repositoryPushOperations).toEqual([
        { computer: "dev", repositoryPath: "acme/silo", commitCount: 2, status: "succeeded" },
      ])
      expect(onOpenSilo.mock.calls[0][0].computers[0].repositories[0].ahead).toBe(0)
    } finally {
      preview.unmount()
      vi.useRealTimers()
    }
  })

  it("acknowledges host actions outside the popover and keeps quit local to the preview", async () => {
    const user = userEvent.setup()
    const open = vi.spyOn(window, "open")
    const close = vi.spyOn(window, "close")
    render(<StatusBarPreview source={applicationSourceForScenario("running")} onOpenSilo={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Open dev in Terminal" }))
    expect(screen.queryByRole("dialog", { name: "Silo" })).not.toBeInTheDocument()
    expect(screen.getByRole("complementary", { name: "Preview feedback" })).toHaveTextContent("Preview: open Terminal in dev.")
    await user.click(screen.getByRole("button", { name: "Silo status bar" }))
    await user.click(screen.getByRole("button", { name: "Quit Silo" }))
    const confirmQuit = screen.queryByRole("button", { name: "Stop and quit" })
    if (confirmQuit) await user.click(confirmQuit)
    expect(screen.queryByRole("button", { name: "Silo status bar" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Relaunch Silo" }))
    expect(screen.getByRole("button", { name: "Silo status bar" })).toBeVisible()
    expect(screen.getByRole("dialog", { name: "Silo" })).toBeVisible()
    expect(open).not.toHaveBeenCalled()
    expect(close).not.toHaveBeenCalled()
    open.mockRestore()
    close.mockRestore()
  })

  it("starts clean when a fixture changes after a simulated quit", async () => {
    const user = userEvent.setup()
    const source = applicationSourceForScenario("running")
    const onOpenSilo = vi.fn()
    const preview = render(<StatusBarPreview fixtureKey="running" source={source} mode="stale" onOpenSilo={onOpenSilo} />)
    await user.click(screen.getByRole("button", { name: "Retry dev status" }))
    expect(screen.queryByRole("button", { name: "Retry dev status" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Quit Silo" }))
    preview.rerender(<StatusBarPreview fixtureKey="running" source={source} mode="empty" onOpenSilo={onOpenSilo} />)
    expect(screen.getByRole("button", { name: "Silo status bar" })).toBeVisible()
    expect(screen.getByText("No computers yet")).toBeVisible()
    expect(screen.queryByRole("button", { name: "Relaunch Silo" })).not.toBeInTheDocument()
  })

  it("opens the requested application section with its computer filter and changed state", async () => {
    window.history.replaceState(null, "", "?view=status-bar&scenario=running&repository-push=failed")
    const user = userEvent.setup()
    render(<FixtureApp />)
    await user.click(screen.getByRole("button", { name: "Start playgrounds" }))
    await user.click(screen.getByRole("button", { name: "Review push failure for dev, acme/silo" }))

    const navigation = within(screen.getByRole("navigation", { name: "Silo navigation" }))
    expect(navigation.getByRole("button", { name: "Files" })).toHaveAttribute("aria-current", "page")
    expect(screen.getByRole("button", { name: "Remove dev" })).toBeVisible()
    const repositories = within(screen.getByRole("list", { name: "Repositories" }))
    expect(repositories.getByText("silo")).toBeVisible()
    expect(repositories.queryByText("acme/platform-tools")).not.toBeInTheDocument()
    await user.click(navigation.getByRole("button", { name: "All computers" }))
    const playgrounds = within(screen.getByRole("list", { name: "Configured computers" })).getByText("playgrounds").closest("li")!
    expect(within(playgrounds).getByText("Running", { exact: true })).toBeVisible()
    expect(new URL(window.location.href).searchParams.get("view")).toBe("app")
  })

  it("opens runtime repair in the application's existing System issue destination", async () => {
    window.history.replaceState(null, "", "?view=status-bar&scenario=running&system-issue=needed")
    const user = userEvent.setup()
    render(<FixtureApp />)
    await user.click(screen.getByRole("button", { name: "View issue" }))
    expect(screen.getByRole("region", { name: "System issue" })).toBeVisible()
    expect(screen.queryByRole("main", { name: "Status bar preview" })).not.toBeInTheDocument()
  })

  it("opens the Logs item filtered to the computer with an error", async () => {
    window.history.replaceState(null, "", "?view=status-bar&scenario=bootstrap-failure")
    const user = userEvent.setup()
    render(<FixtureApp />)
    await user.click(screen.getByRole("button", { name: "See logs for dev" }))
    const navigation = within(screen.getByRole("navigation", { name: "Silo navigation" }))
    expect(navigation.getByRole("button", { name: "Logs" })).toHaveAttribute("aria-current", "page")
    expect(screen.getByRole("button", { name: "Remove dev" })).toBeVisible()
    expect(screen.queryByRole("main", { name: "Status bar preview" })).not.toBeInTheDocument()
  })
})
