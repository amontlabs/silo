import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { OnboardingPreview } from "@/fixtures/onboarding-preview"

import type { GitHubConnectionState } from "@/features/onboarding/model/onboarding-source"

import { githubStateFromSearch, onboardingScenarios, repositoryFixtures } from "@/fixtures/scenarios"

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

it("opens the deterministic completed presentation on Review without finishing setup", () => {
  const finishSetup = vi.fn()
  render(<OnboardingPreview
    source={onboardingScenarios.running}
    repositoryOptions={repositoryFixtures}
    initialCompleted
    actions={{ finishSetup }}
  />)

  expect(screen.getByRole("tab", { name: /Review/ })).toHaveAttribute("aria-selected", "true")
  expect(screen.getByText("Stay informed")).toBeVisible()
  expect(finishSetup).not.toHaveBeenCalled()
})


it("only applies valid explicit GitHub fixture overrides", () => {
  expect(githubStateFromSearch("")).toBeUndefined()
  expect(githubStateFromSearch("?github=unknown")).toBeUndefined()
  expect(githubStateFromSearch("?github=disconnected")).toBe("disconnected")
  expect(githubStateFromSearch("?github=connecting")).toBe("connecting")
  expect(githubStateFromSearch("?github=connected")).toBe("connected")
})


it("navigates five steps, with the agent chosen before GitHub and GitHub going directly to Review", async () => {
  const user = userEvent.setup()
  renderScenario()

  expectHiddenPanelHeading("Dependencies")
  await user.click(screen.getByRole("button", { name: "Continue" }))
  expectHiddenPanelHeading("Creating your computers")
  await user.click(screen.getByRole("button", { name: "Continue" }))
  expectHiddenPanelHeading("Your agent")
  expect(screen.getAllByRole("tab").map(tab => tab.getAttribute("aria-label"))).toEqual(["Dependencies", "Computers", "Your agent", "GitHub", "Review"])
  // Continue waits for a choice.
  expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled()
  await user.click(screen.getByRole("radio", { name: "Codex" }))
  await user.click(screen.getByRole("button", { name: "Continue" }))
  expectHiddenPanelHeading("GitHub")
  await user.click(screen.getByRole("button", { name: "Continue" }))
  expectHiddenPanelHeading("Review setup")
  await user.click(screen.getByRole("tab", { name: /Review/ }))
  expectHiddenPanelHeading("Review setup")
  await user.click(screen.getByRole("button", { name: "Back" }))
  expectHiddenPanelHeading("GitHub")
})


it("shares the application choices in onboarding and keeps the selected browser", async () => {
  const user = userEvent.setup()
  renderScenario()

  expect(screen.getByRole("combobox", { name: "Terminal" })).toHaveTextContent("Terminal")
  expect(screen.getByRole("combobox", { name: "Code editor" })).toHaveTextContent("Visual Studio Code")
  const browser = screen.getByRole("combobox", { name: "Browser" })
  expect(browser).toHaveTextContent("Safari")

  await user.click(browser)
  await user.click(screen.getByRole("option", { name: "Firefox" }))
  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  await user.click(screen.getByRole("tab", { name: /Dependencies/ }))
  expect(screen.getByRole("combobox", { name: "Browser" })).toHaveTextContent("Firefox")
})


it("renders five borderless setup navigation items", () => {
  renderScenario()

  const navigation = screen.getByRole("navigation", { name: "Setup steps" })
  const tabs = within(navigation).getAllByRole("tab")
  expect(tabs).toHaveLength(5)
  for (const tab of tabs) expect(tab).toHaveAttribute("data-appearance", "borderless")
})


it("supports vertical arrow-key navigation across the setup sidebar", async () => {
  const user = userEvent.setup()
  renderScenario()
  const dependencies = screen.getByRole("tab", { name: /Dependencies/ })

  await user.click(dependencies)
  await user.keyboard("{ArrowDown}")

  expect(screen.getByRole("tab", { name: /Computers/ })).toHaveAttribute("aria-selected", "true")
  expectHiddenPanelHeading("Creating your computers")
})


it("shows running computer feedback only in the sidebar outside Computers", async () => {
  const user = userEvent.setup()
  renderScenario()

  for (const step of ["Your agent", "GitHub", "Review"]) {
    await user.click(screen.getByRole("tab", { name: new RegExp(step) }))
    const panel = screen.getByRole("tabpanel")
    expect(within(panel).queryByRole("progressbar", { name: "Computer setup progress" })).not.toBeInTheDocument()
    expect(screen.getByRole("tab", { name: /Computers/ })).toHaveAccessibleDescription("In progress")
    expect(screen.getByRole("tab", { name: /Computers/ })).toHaveAttribute("aria-busy", "true")
  }
})
