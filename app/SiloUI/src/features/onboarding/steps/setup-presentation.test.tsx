import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { Button } from "@/components/ui/button"
import { SetupNotice } from "@/features/onboarding/components/setup-notice"
import { fixtureComputerDefaults } from "@/fixtures/computer-configurations"
import type { ReviewQueueItemView, ComputerProgressView } from "@/features/onboarding/model/onboarding-state"
import { ReviewStep } from "@/features/onboarding/steps/review-step"
import { ComputersStep } from "@/features/onboarding/steps/computers-step"
import { projectOnboarding } from "@/features/onboarding/model/onboarding-state"
import { onboardingScenarios } from "@/fixtures/scenarios"

const progress: ComputerProgressView = {
  status: "running", elapsedSeconds: 83, currentComputer: "playgrounds", currentMessage: "Checking computer connectivity",
  completedOperations: 4, totalOperations: 9, fraction: 4 / 9,
  computers: [
    { name: "dev", status: "ready", detail: "Ready" },
    { name: "playgrounds", status: "working", detail: "Checking computer connectivity" },
    { name: "personal", status: "waiting", detail: "Waiting" },
  ],
  visibleEvents: [], readyCount: 1, workingCount: 1, waitingCount: 1, failedCount: 0, retryable: false,
}

const queueItems: ReviewQueueItemView[] = [
  { id: "computerRun", label: "Create computers", status: "succeeded" },
  { id: "computerVerify", label: "Verify computers", status: "running" },
  { id: "githubRun", label: "Save GitHub", status: "queued" },
  { id: "githubVerify", label: "Verify GitHub", status: "queued" },
  { id: "identityRun", label: "Save Git identities", status: "queued" },
  { id: "identityVerify", label: "Verify Git identities", status: "queued" },
  { id: "completion", label: "Finish setup", status: "queued" },
]

function renderReview(onEditStep = vi.fn()) {
  render(<ReviewStep configurations={fixtureComputerDefaults} queueItems={queueItems} computers={progress.computers} computerRetryable={false} identitySummary="Alex · alex@example.com" githubSummary="2 repositories selected" onRetryComputerSetup={vi.fn()} onEditStep={onEditStep} />)
  return onEditStep
}

describe("setup progress and review presentation", () => {
  it("shows elapsed computer setup time when the start timestamp is zero", () => {
    const source = onboardingScenarios.running
    const { computerProgress } = projectOnboarding({ ...source, bootstrapState: { ...source.bootstrapState, startedAt: 0, updatedAt: 83 } }, "connected")
    render(<ComputersStep configurations={fixtureComputerDefaults} progress={computerProgress} onConfigurationsChange={vi.fn()} onRetry={vi.fn()} />)
    expect(screen.getByLabelText("Elapsed time").textContent).toBe("01:23")
  })
  it("keeps activity collapsed until requested and preserves the list while opening it", async () => {
    const user = userEvent.setup()
    render(<ComputersStep configurations={fixtureComputerDefaults} progress={progress} onConfigurationsChange={vi.fn()} onRetry={vi.fn()} />)
    const configurations = screen.getByRole("list", { name: "Configured computers" })
    expect(screen.queryByLabelText("Computer activity")).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Expand activity" }))
    expect(screen.getByLabelText("Computer activity")).toHaveTextContent("No activity yet.")
    expect(screen.getByRole("list", { name: "Configured computers" })).toBe(configurations)
    expect(within(configurations).getAllByRole("listitem")).toHaveLength(3)
    await user.click(screen.getByRole("button", { name: "Collapse activity" }))
    expect(screen.queryByLabelText("Computer activity")).not.toBeInTheDocument()
  })

  it("distinguishes each computer status without replacing the busy configuration icon", () => {
    render(<ComputersStep configurations={fixtureComputerDefaults} progress={progress} onConfigurationsChange={vi.fn()} onRetry={vi.fn()} />)
    const rows = within(screen.getByRole("list", { name: "Configured computers" })).getAllByRole("listitem")
    expect(within(rows[0]).getByText("Complete")).toBeVisible()
    expect(within(rows[1]).getByText("In progress")).toBeVisible()
    expect(rows[1]).toHaveAttribute("aria-busy", "true")
    expect(rows[1].querySelector("svg.lucide-monitor")).not.toBeNull()
    expect(rows[1].querySelector("svg.lucide-loader-circle")).not.toBeNull()
    expect(within(rows[2]).getByText("Waiting")).toBeVisible()
    expect(screen.getByLabelText("Elapsed time")).toHaveTextContent("01:23")
  })

  it("keeps recovery and retry beside the failed operation", async () => {
    const user = userEvent.setup()
    const retry = vi.fn()
    render(<ComputersStep configurations={fixtureComputerDefaults} progress={{ ...progress, status: "failed", retryable: true, currentMessage: "The computer could not be reached.", recovery: "Check the network connection, then retry setup.", computers: [{ name: "playgrounds", status: "failed", detail: "The computer could not be reached." }] }} onConfigurationsChange={vi.fn()} onRetry={retry} />)
    const status = screen.getByRole("alert")
    expect(status).toHaveTextContent("The computer could not be reached.")
    expect(status.parentElement).toHaveTextContent("Check the network connection, then retry setup.")
    await user.click(within(status).getByRole("button", { name: "Retry" }))
    expect(retry).toHaveBeenCalledOnce()
    expect(within(screen.getByRole("list", { name: "Configured computers" })).getByText("Failed")).toBeVisible()
  })

  it("shows validation on existing cards and preserves computer order and resources", () => {
    renderReview()
    expect(screen.getAllByRole("list").map(list => list.getAttribute("aria-label"))).toEqual(["Computers"])
    expect(screen.queryByText("queued")).not.toBeInTheDocument()
    expect(screen.queryByText("succeeded")).not.toBeInTheDocument()
    const computers = within(screen.getByRole("list", { name: "Computers" })).getAllByRole("listitem")
    expect(computers.map((row) => row.querySelector("[title]")?.getAttribute("title"))).toEqual(["dev", "playgrounds", "personal"])
    expect(computers[0]).toHaveTextContent("Complete")
    expect(computers[1]).toHaveTextContent("In progress")
    expect(computers[1]).toHaveAttribute("aria-busy", "true")
    expect(computers[2]).toHaveTextContent("Waiting")
    expect(computers[0]).toHaveTextContent("CPUs: 8 · Memory: 32 GiB · Disk: 120 GiB")
    expect(screen.getByText("2 repositories selected")).toBeVisible()
    expect(screen.getByText("Alex · alex@example.com")).toBeVisible()
  })

  it.each([
    ["idle", "Not started"],
    ["queued", "Waiting"],
    ["running", "In progress"],
    ["succeeded", "Complete"],
    ["failed", "Failed"],
  ] as const)("shows %s Git validation on the author card", (status, label) => {
    render(<ReviewStep configurations={fixtureComputerDefaults} computers={progress.computers} queueItems={[
      { id: "githubRun", label: "Save GitHub", status: "succeeded" },
      { id: "githubVerify", label: "Verify GitHub", status },
      { id: "identityRun", label: "Save Git identities", status: "succeeded" },
      { id: "identityVerify", label: "Verify Git identities", status, failure: status === "failed" ? "Git identity could not be verified." : undefined },
    ]} computerRetryable={false} identitySummary="Alex · alex@example.com" githubSummary="GitHub not connected" onRetryComputerSetup={vi.fn()} />)
    const author = screen.getByRole("group", { name: "Git identity" })
    expect(author).toHaveTextContent(label)
    expect(screen.getByRole("group", { name: "GitHub access" })).toHaveTextContent(label)
    expect(author).toHaveTextContent("Alex · alex@example.com")
    for (const row of [author, screen.getByRole("group", { name: "GitHub access" })]) {
      expect(row.classList.contains("bg-success/[0.035]")).toBe(status === "succeeded")
    }
    if (status === "failed") expect(author).toHaveTextContent("Git identity could not be verified.")
  })

  it("keeps computer failures on the affected computer and never validates missing results", () => {
    render(<ReviewStep configurations={fixtureComputerDefaults} computers={[
      { name: "dev", status: "failed", detail: "Computer could not be verified." },
    ]} queueItems={[]} computerRetryable={false} identitySummary="No Git identity" githubSummary="GitHub not connected" onRetryComputerSetup={vi.fn()} />)
    const computers = within(screen.getByRole("list", { name: "Computers" })).getAllByRole("listitem")
    expect(computers[0]).toHaveTextContent("Failed")
    expect(computers[0]).toHaveTextContent("Computer could not be verified.")
    expect(computers[1]).not.toHaveTextContent("Complete")
    expect(screen.queryByText("Complete")).not.toBeInTheDocument()
  })

  it("routes review edit shortcuts to their corresponding steps", async () => {
    const user = userEvent.setup()
    const edit = renderReview()
    await user.click(screen.getByRole("button", { name: "Edit computers" }))
    expect(edit).toHaveBeenLastCalledWith("computers")
    await user.click(screen.getByRole("button", { name: "Edit GitHub and Git identity" }))
    expect(edit).toHaveBeenLastCalledWith("github")
    expect(screen.getAllByRole("button", { name: /^Edit / }).map(button => button.getAttribute("aria-label"))).toEqual(["Edit computers", "Edit GitHub and Git identity"])
  })

  it("keeps recovery visible while technical evidence stays optional", async () => {
    const user = userEvent.setup()
    const repair = vi.fn()
    render(<SetupNotice title="Setup could not finish" detail="The helper is unavailable." recovery="Repair the installation to continue." technicalDetails="Helper connection timed out after 30 seconds." action={<Button onClick={repair}>Repair</Button>} />)
    expect(screen.getByRole("alert")).toHaveTextContent("Repair the installation to continue.")
    expect(screen.queryByText("Helper connection timed out after 30 seconds.")).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Show technical details" }))
    expect(screen.getByText("Helper connection timed out after 30 seconds.")).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Repair" }))
    expect(repair).toHaveBeenCalledOnce()
  })
})

it("offers the real device connection flow during production computer setup", async () => {
  const user = userEvent.setup()
  const onConnectDevice = vi.fn()
  render(<ComputersStep configurations={fixtureComputerDefaults} progress={progress} onConfigurationsChange={vi.fn()} onRetry={vi.fn()} onConnectDevice={onConnectDevice} />)
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(screen.getByRole("menuitem", { name: "Connect device…" }))
  expect(onConnectDevice).toHaveBeenCalledOnce()
})
