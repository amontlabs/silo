import { createApplicationActionsMock } from "@/test/application-actions"
import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { ApplicationPreview } from "@/fixtures/application-preview"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"



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


it("preserves unsaved Git identity and pending repository choices during source polling", async () => {
  const source = applicationSourceForScenario("running")
  const { actions, user, rerender } = renderApplication("running", source)
  await user.click(within(appNavigation()).getByRole("button", { name: "GitHub" }))
  const github = within(appPanel("GitHub"))
  const name = github.getByLabelText("Git name for dev")
  await user.clear(name)
  await user.type(name, "Unfinished Author")
  rerender(<ApplicationPreview source={structuredClone(source)} actions={actions} />)
  expect(name).toHaveValue("Unfinished Author")
  expect(actions.saveGitHubConfiguration).not.toHaveBeenCalled()
  await user.click(github.getByRole("checkbox", { name: "All repositories for playgrounds" }))
  rerender(<ApplicationPreview source={structuredClone(source)} actions={actions} />)
  expect(github.getByRole("checkbox", { name: "All repositories for playgrounds" })).toBeChecked()
  expect(within(appNavigation()).getByRole("button", { name: "GitHub" })).toHaveAttribute("aria-busy", "true")
})



it("applies repository changes immediately and commits identity fields on blur", async () => {
  const { actions, user } = renderApplication()
  await user.click(within(appNavigation()).getByRole("button", { name: "GitHub" }))
  const github = within(appPanel("GitHub"))

  const name = github.getByLabelText("Git name for playgrounds")
  await user.clear(name)
  await user.type(name, "Morgan Example")
  expect(actions.saveGitHubConfiguration).not.toHaveBeenCalled()
  await user.tab()
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledOnce()
  expect(await screen.findByText("Applying Git identity…")).toBeVisible()
  expect(within(appNavigation()).getByRole("button", { name: "GitHub" })).toHaveAttribute("aria-busy", "true")

  const picker = github.getByRole("combobox", { name: "Add repository to playgrounds" })
  await user.type(picker, "design")
  expect(screen.getByRole("option", { name: "acme/design-system" })).toBeVisible()
  expect(screen.queryByRole("option", { name: "acme/platform-tools" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("option", { name: "acme/design-system" }))
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledTimes(2)
  await user.click(picker)
  expect(screen.queryByRole("option", { name: "acme/design-system" })).not.toBeInTheDocument()

  const selected = github.getByRole("table", { name: "Selected repositories for playgrounds" })
  const pushes = within(selected).getByRole("checkbox", { name: "Allow GitHub changes for acme/platform-tools" })
  await user.click(pushes)
  expect(pushes).toBeChecked()
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledTimes(3)
  await user.click(within(selected).getByRole("button", { name: "Remove acme/design-system from playgrounds" }))
  expect(within(selected).queryByText("acme/design-system")).not.toBeInTheDocument()
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledTimes(4)
  expect(actions.saveGitHubConfiguration).toHaveBeenLastCalledWith(expect.objectContaining({
    computers: expect.arrayContaining([
      expect.objectContaining({
        computer: "playgrounds",
        repositories: [{ repository: "acme/platform-tools", allowPushes: true }],
      }),
    ]),
  }))
  expect(github.queryByRole("button", { name: "Save changes" })).not.toBeInTheDocument()
  expect(github.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument()
})



it("keeps Git identity editable through disconnected and connecting GitHub states", async () => {
  const disconnectedSource = applicationSourceForScenario("running", "disconnected")
  disconnectedSource.github.accessEnabled = false
  const disconnected = renderApplication("running", disconnectedSource)
  await disconnected.user.click(within(appNavigation()).getByRole("button", { name: "GitHub" }))
  let github = within(appPanel("GitHub"))
  expect(github.getByRole("heading", { name: "Not connected" })).toBeVisible()
  expect(github.getByLabelText("Git name for dev")).toBeEnabled()
  expect(github.queryByLabelText("Add repository to dev")).not.toBeInTheDocument()
  await disconnected.user.click(github.getByRole("button", { name: "Connect GitHub" }))
  expect(disconnected.actions.connectGitHub).toHaveBeenCalledOnce()
  expect(within(appNavigation()).getByRole("button", { name: "GitHub" })).toHaveAttribute("aria-busy", "true")
  disconnected.unmount()

  const connecting = renderApplication("running", applicationSourceForScenario("running", "connecting"))
  await connecting.user.click(within(appNavigation()).getByRole("button", { name: "GitHub" }))
  github = within(appPanel("GitHub"))
  expect(github.getByRole("status")).toHaveTextContent("Connecting to GitHub…")
  await connecting.user.click(github.getByRole("button", { name: "Open browser again" }))
  expect(connecting.actions.reopenGitHubAuthorization).toHaveBeenCalledOnce()
  await connecting.user.click(github.getByRole("button", { name: /^Cancel$/ }))
  expect(connecting.actions.cancelGitHubConnection).toHaveBeenCalledOnce()
  expect(github.getByLabelText("Git name for dev")).toBeEnabled()
  expect(github.queryByLabelText("Add repository to dev")).not.toBeInTheDocument()
})



it("shows per-computer GitHub apply progress, success, and actionable failure through notifications", async () => {
  const source = applicationSourceForScenario("running", "connected")
  const application = renderApplication("running", source)
  await application.user.click(within(appNavigation()).getByRole("button", { name: "GitHub" }))
  const github = within(appPanel("GitHub"))
  expect(github.queryByRole("button", { name: /not applied/i })).not.toBeInTheDocument()

  const next = (mode: "applying" | "succeeded" | "failed", revision: number) => {
    const fixture = applicationSourceForScenario("running", "connected", undefined, undefined, undefined, undefined, undefined, 0, mode)
    fixture.github.policyRevision = revision
    application.rerender(<ApplicationPreview source={fixture} actions={application.actions} />)
  }

  // Only changes the user starts notify; background operations never do.
  await application.user.click(github.getByRole("button", { name: "Disable for all computers" }))
  next("applying", 1)
  expect(await screen.findByText("Applying repository access…")).toBeVisible()
  expect(github.getByRole("region", { name: "Computer Git identity and repository access" })).toHaveAttribute("aria-busy", "true")

  next("succeeded", 2)
  expect(await screen.findByText("GitHub settings applied")).toBeVisible()
  expect(screen.queryByText("Applying repository access…")).not.toBeInTheDocument()

  await application.user.click(github.getByRole("button", { name: "Disable for all computers" }))
  next("failed", 3)
  expect(await screen.findByText(/GitHub settings could not be applied\./)).toBeVisible()
  expect(screen.queryByText("GitHub settings applied")).not.toBeInTheDocument()
  expect(github.getByRole("button", { name: /GitHub settings not applied for dev/ })).toHaveTextContent("Not applied")
  await application.user.click(screen.getByRole("button", { name: "Retry" }))
  expect(application.actions.retryGitHubConfiguration).toHaveBeenCalledWith("dev")
})
