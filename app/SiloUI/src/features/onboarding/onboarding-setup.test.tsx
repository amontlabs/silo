import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { OnboardingPreview } from "@/fixtures/onboarding-preview"
import { FixtureApp } from "@/fixtures/fixture-app"
import type { GitHubConnectionState } from "@/features/onboarding/model/onboarding-source"
import { projectOnboarding } from "@/features/onboarding/model/onboarding-state"
import { onboardingScenarios, repositoryFixtures } from "@/fixtures/scenarios"

function renderScenario(name: keyof typeof onboardingScenarios = "running", githubState?: GitHubConnectionState) {
  return render(<OnboardingPreview source={onboardingScenarios[name]} initialGitHubConnectionState={githubState} repositoryOptions={repositoryFixtures} actions={{
    saveComputerConfiguration: vi.fn(),
    retryComputerSetup: vi.fn(),
    finishSetup: vi.fn(),
  }} />)
}

function expectHiddenPanelHeading(name: string) {
  const heading = screen.getByRole("heading", { name, level: 2 })
  expect(heading).toHaveAttribute("data-visual-heading", "hidden")
  expect(heading.parentElement?.tagName).toBe("SECTION")
}

function expectDisclosureIndicator(trigger: HTMLElement) {
  const indicator = trigger.querySelector("svg.lucide-chevron-down")
  expect(indicator).not.toBeNull()
  expect(indicator).toHaveAttribute("aria-hidden", "true")
}


it("expands the consolidated runtime failure and exposes non-repair remediation", async () => {
  const user = userEvent.setup()
  renderScenario("dependency-failure")

  expect(screen.getByText("Computer runtime")).toBeVisible()
  expect(screen.getByText("Reinstall this Silo build from a trusted package.")).toBeVisible()
  expect(screen.queryByText("msb")).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: /Repair/ })).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled()
  const disclosure = screen.getByRole("button", { name: /Bundled tools/ })
  expectDisclosureIndicator(disclosure)
  await user.click(disclosure)
  expect(screen.queryByText("Computer runtime")).not.toBeInTheDocument()
  await user.click(disclosure)
  expect(screen.getByText("Computer runtime")).toBeVisible()
})


it("updates native dependency reports through the real fixture wrapper without discarding local state", async () => {
  const pendingChecks = onboardingScenarios.complete.preflightChecks.map((check) => ({
      ...check,
      status: "pending" as const,
      detail: `Checking ${check.title}…`,
    }))
  const retry = vi.fn()
  const { rerender } = render(<FixtureApp nativeDependencies={{ checks: pendingChecks, retry }} />)

  expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled()
  expect(screen.getAllByLabelText("Checking")).not.toHaveLength(0)

  rerender(<FixtureApp nativeDependencies={{ checks: onboardingScenarios.complete.preflightChecks, retry }} />)
  expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled()
  expect(screen.getAllByLabelText("All checks passed")).toHaveLength(2)

  const unavailableChecks = onboardingScenarios.complete.preflightChecks.map((check) => check.id === "system-virtualization" ? {
      ...check,
      status: "unavailable" as const,
      detail: "Virtualization checks are unavailable.",
    } : check)
  rerender(<FixtureApp nativeDependencies={{ checks: unavailableChecks, retry }} />)
  expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled()
  expect(screen.getByText("Checks unavailable")).toBeVisible()
})


it("keeps native dependency failures authoritative after computer Retry", async () => {
  const user = userEvent.setup()
  window.history.replaceState(null, "", "/?view=onboarding&scenario=bootstrap-failure")
  const checks = onboardingScenarios.complete.preflightChecks.map((check) => check.id === "system-virtualization" ? {
      ...check,
      status: "unavailable" as const,
      detail: "Virtualization checks are unavailable.",
    } : check)
  render(<FixtureApp nativeDependencies={{ checks, retry: vi.fn() }} />)

  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  await user.click(screen.getByRole("button", { name: "Retry" }))
  await user.click(screen.getByRole("tab", { name: /Dependencies/ }))

  expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled()
  expect(screen.getByText("Checks unavailable")).toBeVisible()
  window.history.replaceState(null, "", "/")
})


it("keeps stress-fixture activity collapsed until requested and filters unsafe output", async () => {
  const user = userEvent.setup()
  renderScenario("stress-running")
  await user.click(screen.getByRole("tab", { name: /Computers/ }))

  const panel = within(screen.getByRole("tabpanel"))
  expect(panel.queryByLabelText("Computer activity")).not.toBeInTheDocument()
  expect(panel.getByLabelText("Elapsed time")).toHaveTextContent("02:18")
  expect(panel.getByText("27 of 36 operations complete")).toBeVisible()
  expect(panel.getByText("12 computers · 12 on this device · 0 on other devices")).toBeVisible()
  const list = panel.getByRole("list", { name: "Configured computers" })
  expect(within(list).getAllByRole("listitem")).toHaveLength(12)
  expect(within(list).getByText("client-alpha-integration")).toBeVisible()
  const working = within(list).getByText("docs-build").closest("li")!
  expect(within(working).getByText("In progress")).toBeVisible()

  const expand = panel.getByRole("button", { name: "Expand activity" })
  expectDisclosureIndicator(expand)
  expect(expand).toHaveAttribute("aria-expanded", "false")
  await user.click(expand)
  expect(expand).toHaveAttribute("aria-expanded", "true")
  expect(panel.getByLabelText("Computer activity")).toHaveTextContent("Verifying 'docs-build'.")
  expect(panel.queryByText(/Internal verification path/)).not.toBeInTheDocument()

  const controls = panel.getByRole("group", { name: "Live activity controls" })
  expect(within(controls).getAllByRole("button")).toHaveLength(2)
  expect(within(controls).getByRole("button", { name: "Collapse activity" })).toHaveTextContent("Live activity")
  const copy = vi.spyOn(navigator.clipboard, "writeText")
  await user.click(within(controls).getByRole("button", { name: "Copy activity" }))
  expect(copy).toHaveBeenCalledWith(expect.stringContaining("Verifying 'docs-build'."))
  expect(copy).not.toHaveBeenCalledWith(expect.stringContaining("Internal verification path"))
  expect(within(controls).getByRole("button", { name: "Activity copied" })).toHaveAttribute("data-copy-status", "copied")

  await user.click(within(controls).getByRole("button", { name: "Collapse activity" }))
  expect(panel.queryByLabelText("Computer activity")).not.toBeInTheDocument()
  expect(panel.getByRole("button", { name: "Expand activity" })).toHaveAttribute("aria-expanded", "false")
  expect(list).toBeVisible()
  await user.click(panel.getByRole("button", { name: "Expand activity" }))
  expect(panel.getByLabelText("Computer activity")).toBeVisible()
})


it("reports a clipboard denial without an unhandled interaction failure", async () => {
  const user = userEvent.setup()
  vi.spyOn(navigator.clipboard, "writeText").mockRejectedValueOnce(new DOMException("Denied", "NotAllowedError"))
  renderScenario()
  await user.click(screen.getByRole("tab", { name: /Computers/ }))

  await user.click(screen.getByRole("button", { name: "Copy activity" }))

  const failedCopy = screen.getByRole("button", { name: "Copy activity failed" })
  expect(failedCopy).toHaveAttribute("data-copy-status", "failed")
  expect(failedCopy.textContent).toBe("")
  expect(failedCopy.querySelector("svg")).toHaveClass("lucide-circle-alert")
  expect(screen.queryByText("Copy failed")).not.toBeInTheDocument()
})


it("enables Finish only after every queue operation succeeds", async () => {
  const user = userEvent.setup()
  const running = renderScenario()
  await user.click(screen.getByRole("tab", { name: /Review/ }))
  expect(screen.getByRole("button", { name: "Finish" })).toBeDisabled()
  running.unmount()

  const finishSetup = vi.fn()
  render(<OnboardingPreview source={onboardingScenarios.complete} actions={{
    saveComputerConfiguration: vi.fn(),
    retryComputerSetup: vi.fn(),
    finishSetup,
  }} />)
  await user.click(screen.getByRole("tab", { name: /Review/ }))
  expect(screen.getByRole("button", { name: "Finish" })).toBeEnabled()
  expect(screen.getAllByRole("list").map(list => list.getAttribute("aria-label"))).toEqual(["Computers"])
  expect(within(screen.getByRole("list", { name: "Computers" })).getAllByText("Complete")).toHaveLength(3)
  await user.click(screen.getByRole("button", { name: "Finish" }))
  expect(finishSetup).toHaveBeenCalledWith({
    computerConfiguration: {
      schemaVersion: 1,
      computers: onboardingScenarios.complete.computerConfigurations,
    },
    applications: {
      terminal: "Terminal",
      editor: "Visual Studio Code",
      browser: "Safari",
      terminalUseSystemDefault: true,
      editorUseSystemDefault: true,
      browserUseSystemDefault: true,
    },
    github: {
      connectionState: "connected",
      computers: [
        {
          computer: "dev",
          repositories: [{ repository: "acme/silo", allowPushes: false }],
          identity: { name: "Taylor Example", email: "taylor@example.com", apply: true },
        },
        {
          computer: "playgrounds",
          repositories: [],
          identity: { name: "Taylor Example", email: "taylor@example.com", apply: true },
        },
        {
          computer: "personal",
          repositories: [],
          identity: { name: "Taylor Example", email: "taylor@example.com", apply: true },
        },
      ],
    },
  })
  expect(screen.getByRole("status")).toHaveTextContent("Setup complete")
})


it("finishes connected setup with explicit zero repository access and no skip state", async () => {
  const user = userEvent.setup()
  const finishSetup = vi.fn()
  render(<OnboardingPreview
    source={onboardingScenarios.complete}
    initialGitHubConnectionState="connected"
    repositoryOptions={repositoryFixtures}
    actions={{
      saveComputerConfiguration: vi.fn(),
      retryComputerSetup: vi.fn(),
      finishSetup,
    }}
  />)

  await user.click(screen.getByRole("tab", { name: /GitHub/ }))
  await user.click(screen.getByRole("button", { name: "Remove acme/silo from dev" }))
  await user.click(screen.getByRole("button", { name: "Continue" }))
  expect(screen.getByText("0 repositories across 0 of 3 computers · 0 repositories allowing GitHub changes")).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Finish" }))

  expect(finishSetup).toHaveBeenCalledOnce()
  const request = finishSetup.mock.lastCall?.[0]
  expect(request.github.connectionState).toBe("connected")
  expect(request.github.computers.map(({ computer, repositories }: { computer: string; repositories: unknown[] }) => ({ computer, repositories }))).toEqual([
    { computer: "dev", repositories: [] },
    { computer: "playgrounds", repositories: [] },
    { computer: "personal", repositories: [] },
  ])
  expect(request.github).not.toHaveProperty("skipped")
  expect(request).not.toHaveProperty("repositoryAccessSkipped")
})


it("does not treat CLI computer completion as completion of later setup work", () => {
  const source = onboardingScenarios.complete
  const computerOnly = projectOnboarding({
    ...source,
    bootstrapState: {
      ...source.bootstrapState,
      completedPhases: ["preflight", "toolchain", "deviceIntegration", "computers"],
    },
  }, "connected")

  expect(computerOnly.queueItems.find(({ id }) => id === "computerVerify")?.status).toBe("succeeded")
  expect(computerOnly.queueItems.find(({ id }) => id === "githubRun")?.status).toBe("queued")
  expect(computerOnly.finishEnabled).toBe(false)
})


it("presents the bootstrap failure with its exact recovery", async () => {
  const user = userEvent.setup()
  renderScenario("bootstrap-failure")
  await user.click(screen.getByRole("tab", { name: /Review/ }))
  expect(screen.getByRole("alert")).toHaveTextContent("Candidate networking could not become ready for 'playgrounds'.")
  expect(screen.getByRole("alert")).toHaveTextContent("Repair computer startup or SSH forwarding for 'playgrounds', then resume Setup.")
  expect(screen.getByRole("button", { name: "Finish" })).toBeDisabled()
})


it("does not claim computer creation started while dependencies are blocked", async () => {
  const user = userEvent.setup()
  renderScenario("dependency-failure")

  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  expectHiddenPanelHeading("Computers are waiting")
  expect(screen.queryByText("Complete the dependency checks before computer creation starts.")).not.toBeInTheDocument()
})


it("routes retryable computer failures through the narrow action seam", async () => {
  const user = userEvent.setup()
  const retryComputerSetup = vi.fn()
  const actions = { saveComputerConfiguration: vi.fn(), retryComputerSetup, finishSetup: vi.fn() }

  render(<OnboardingPreview source={onboardingScenarios["bootstrap-failure"]} actions={actions} />)
  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  await user.click(screen.getByRole("button", { name: "Retry" }))
  expect(retryComputerSetup).toHaveBeenCalledOnce()
})
