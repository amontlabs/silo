import { act, render, screen, waitFor } from "@testing-library/react"
import type userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"

import { setupFakeTimerUser } from "@/test/fake-timer-user"
import { Toaster } from "@/components/ui/sonner"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions, ApplicationSource, GitHubComputerOperation } from "../model/application-source"
import { GitHubPage } from "./github-page"

const base = applicationSourceForScenario("complete")
const computer = base.computers.find((w) => !w.device)!.configuration.name

function sourceWith(operations: GitHubComputerOperation[], revision = 1): ApplicationSource {
  return { ...base, github: { ...base.github, state: "connected", account: "taylor", policyRevision: revision, computerOperations: operations } }
}

function page(source: ApplicationSource, actions: Partial<ApplicationActions> = {}) {
  return <><Toaster /><GitHubPage source={source} actions={actions as ApplicationActions} /></>
}

beforeEach(() => { vi.useFakeTimers() })
afterEach(() => { toast.dismiss() })

const advanceTime = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms) })

async function startUserChange(user: ReturnType<typeof userEvent.setup>, actions: Partial<ApplicationActions> = {}) {
  await user.click(screen.getByRole("button", { name: "Disable for all computers" }))
  expect(actions.setGitHubAccessEnabled).toHaveBeenCalled()
}

describe("GitHub operation notifications", () => {
  it("shows a loading toast while a user change applies, not an inline block", async () => {
    const user = setupFakeTimerUser()
    const actions = { setGitHubAccessEnabled: vi.fn() }
    const view = render(page(sourceWith([]), actions))
    await startUserChange(user, actions)
    view.rerender(page(sourceWith([{ computer, status: "applying", message: "Applying repository access…" }], 2), actions))
    await advanceTime(0)
    expect(screen.getByText("Applying repository access…")).toBeInTheDocument()
    expect(document.querySelector("[role=status][aria-live=polite].border-border")).toBeNull()
  })

  it("keeps a success toast for a user change until it is closed", async () => {
    const user = setupFakeTimerUser()
    const actions = { setGitHubAccessEnabled: vi.fn() }
    const view = render(page(sourceWith([]), actions))
    await startUserChange(user, actions)
    view.rerender(page(sourceWith([{ computer, status: "applying", message: "Applying repository access…" }], 2), actions))
    view.rerender(page(sourceWith([{ computer, status: "succeeded", message: "Repository access applied." }], 3), actions))
    await advanceTime(0)
    expect(screen.getByText("GitHub settings applied")).toBeInTheDocument()
    await advanceTime(4_500)
    expect(screen.getByText("GitHub settings applied")).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Close toast" }))
    await advanceTime(200)
    expect(screen.queryByText("GitHub settings applied")).not.toBeInTheDocument()
  })

  it("never notifies for background applying then succeeded", async () => {
    const view = render(page(sourceWith([])))
    view.rerender(page(sourceWith([{ computer, status: "applying", message: "Applying GitHub settings." }], 2)))
    view.rerender(page(sourceWith([{ computer, status: "succeeded", message: "GitHub access verified." }], 3)))
    await advanceTime(300)
    expect(screen.queryByText("GitHub settings applied")).not.toBeInTheDocument()
    expect(screen.queryByText("Applying GitHub settings.")).not.toBeInTheDocument()
  })

  it("shows a failure toast with Retry for a user change and keeps an inline Not applied label", async () => {
    const retry = vi.fn()
    const user = setupFakeTimerUser()
    const actions = { retryGitHubConfiguration: retry, setGitHubAccessEnabled: vi.fn() }
    const view = render(page(sourceWith([]), actions))
    await startUserChange(user, actions)
    view.rerender(page(sourceWith([{ computer, status: "applying", message: "Applying repository access…" }]), actions))
    view.rerender(page(sourceWith([{ computer, status: "failed", message: "runtime output", canRetry: true }], 2), actions))
    await advanceTime(0)
    expect(screen.getByText("GitHub settings could not be applied.")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: new RegExp(`not applied for ${computer}`, "i") })).toBeInTheDocument()
    expect(document.body).not.toHaveTextContent("runtime output")
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(retry).toHaveBeenCalledExactlyOnceWith(computer)
    await waitFor(() => expect(screen.getAllByText("Retrying GitHub access…").length).toBeGreaterThan(0))
  })

  it("shows only the inline label for a background failure", async () => {
    const view = render(page(sourceWith([{ computer, status: "succeeded", message: "GitHub access verified." }])))
    view.rerender(page(sourceWith([{ computer, status: "failed", message: "background failure", canRetry: true }], 2)))
    await advanceTime(300)
    expect(screen.getByRole("button", { name: new RegExp(`not applied for ${computer}`, "i") })).toBeInTheDocument()
    expect(screen.queryByText("GitHub settings could not be applied.")).not.toBeInTheDocument()
  })

  it("shows only the inline label for a failure already present on load", () => {
    render(page(sourceWith([{ computer, status: "failed", message: "old failure", canRetry: true }])))
    expect(screen.getByRole("button", { name: new RegExp(`not applied for ${computer}`, "i") })).toBeInTheDocument()
    expect(screen.queryByText("GitHub settings could not be applied.")).not.toBeInTheDocument()
  })
})

describe("GitHub repository clear confirmation", () => {
  it("closes on one Escape after hovering the clear button", async () => {
    const user = setupFakeTimerUser()
    const actions = {}
    render(page(sourceWith([]), actions))
    const trigger = screen.getAllByRole("button", { name: /^Clear repositories from / })[0]
    await user.hover(trigger)
    await user.click(trigger)
    expect(await screen.findByText(/^Remove all repositories from /)).toBeVisible()
    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByText(/^Remove all repositories from /)).not.toBeInTheDocument())
    expect(trigger).toHaveFocus()
  })
})
