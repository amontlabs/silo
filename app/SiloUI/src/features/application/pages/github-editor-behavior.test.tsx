import { createApplicationActionsMock } from "@/test/application-actions"
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { GitHubPage } from "./github-page"

function GitHubPanel({ source, actions }: { source: ApplicationSource; actions: ApplicationActions }) {
  return <TooltipProvider delayDuration={0}><Toaster /><section aria-label="GitHub"><GitHubPage source={source} actions={actions} /></section></TooltipProvider>
}
function renderGitHub(scenario: Parameters<typeof applicationSourceForScenario>[0] = "running", source?: ApplicationSource) {
  const actions = createApplicationActionsMock()
  return { actions, user: userEvent.setup(), ...render(<GitHubPanel source={source ?? applicationSourceForScenario(scenario)} actions={actions} />) }
}
function appPanel(name: string) { return screen.getByRole("region", { name }) }



it("reuses the compact onboarding GitHub editor without redundant page framing", async () => {
  const { user } = renderGitHub()

  const github = within(appPanel("GitHub"))
  expect(github.queryByRole("heading", { name: "GitHub" })).not.toBeInTheDocument()
  expect(github.queryByText("Manage the account and repository access available inside each computer.")).not.toBeInTheDocument()
  expect(github.queryByRole("heading", { name: "Repository access" })).not.toBeInTheDocument()
  expect(github.getByText("Connected as @taylor")).toBeVisible()
  expect(github.getByRole("button", { name: "Disable for all computers" })).toBeVisible()
  expect(github.getByRole("button", { name: "Disconnect" })).toBeVisible()
  expect(github.queryByRole("button", { name: "Clear repositories" })).not.toBeInTheDocument()
  const clearDevRepositories = github.getByRole("button", { name: "Clear repositories from dev" })
  expect(clearDevRepositories).toBeVisible()
  await user.hover(clearDevRepositories)
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Clear repositories from dev")
  await user.unhover(clearDevRepositories)
  expect(github.queryByRole("button", { name: "Save changes" })).not.toBeInTheDocument()
  expect(github.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument()

  const editor = github.getByRole("region", { name: "Computer Git identity and repository access" })
  expect(editor).toBeVisible()
  expect(editor).toHaveClass("min-h-0", "flex-1")
  expect(editor.querySelector(".divide-y")).not.toBeNull()

  for (const computer of ["dev", "playgrounds", "personal"]) {
    const identity = github.getByRole("group", { name: `Git identity for ${computer}` })
    expect(identity.closest('[data-slot="card"]')).toBeNull()
    expect(github.getByLabelText(`Git name for ${computer}`)).toBeVisible()
    expect(github.getByLabelText(`Git email for ${computer}`)).toBeVisible()
    expect(github.getByRole("checkbox", { name: `Apply Git identity to ${computer}` })).toBeVisible()
    expect(github.getByRole("button", { name: `Reset Git identity for ${computer}` })).toBeVisible()
    expect(github.getByRole("combobox", { name: `Add repository to ${computer}` })).toBeVisible()
  }

  expect(github.getByLabelText("Git name for dev")).toHaveValue("Taylor Example")
  expect(github.getByLabelText("Git email for dev")).toHaveValue("taylor@example.com")
  expect(github.getByRole("checkbox", { name: "Apply Git identity to dev" })).toBeChecked()
  const repositories = github.getByRole("table", { name: "Selected repositories for dev" })
  expect(within(repositories).getAllByRole("columnheader").map(({ textContent }) => textContent)).toEqual([
    "Repository",
    "Allow GitHub changes",
    "",
  ])
  expect(within(repositories).getByRole("checkbox", { name: "Allow GitHub changes for acme/silo" })).toBeChecked()
  expect(within(repositories).getByRole("checkbox", { name: "Allow GitHub changes for acme/design-system" })).not.toBeChecked()
  expect(within(repositories).getByRole("button", { name: "Remove acme/silo from dev" })).toBeVisible()

  const devDisclosure = github.getByRole("button", { name: "Collapse dev" })
  await user.click(devDisclosure)
  expect(devDisclosure).toHaveAttribute("aria-expanded", "false")
  expect(github.queryByRole("group", { name: "Git identity for dev" })).not.toBeInTheDocument()
  expect(github.getByRole("group", { name: "Git identity for playgrounds" })).toBeVisible()
})



it.each([false, true])("uses the detected host author for each computer missing a policy (partial=%s)", async (partial) => {
  const source = applicationSourceForScenario("running")
  const existing = source.github.computers![0]
  source.github.computers = partial ? [existing] : []
  source.github.deviceIdentity = { name: "Local Author", email: "local@example.test" }
  const { actions, user } = renderGitHub("running", source)

  const github = within(appPanel("GitHub"))
  expect(github.getByLabelText("Git name for playgrounds")).toHaveValue("Local Author")
  expect(github.getByLabelText("Git email for playgrounds")).toHaveValue("local@example.test")
  if (partial) expect(github.getByLabelText("Git name for dev")).toHaveValue(existing.identity.name)
  await user.click(github.getByRole("checkbox", { name: "All repositories for playgrounds" }))
  expect(actions.saveGitHubConfiguration).toHaveBeenLastCalledWith(expect.objectContaining({
    computers: expect.arrayContaining([expect.objectContaining({ computer: "playgrounds", repositoryMode: "all", identity: { name: "Local Author", email: "local@example.test", apply: true } })]),
  }))
})



it("allows repository selection without inventing a missing Git identity", async () => {
  const source = applicationSourceForScenario("running")
  source.github.computers = []
  source.github.deviceIdentity = null
  const { actions, user } = renderGitHub("running", source)

  const github = within(appPanel("GitHub"))
  expect(github.getByLabelText("Git name for dev")).toHaveValue("")
  await user.click(github.getByRole("checkbox", { name: "All repositories for dev" }))
  expect(actions.saveGitHubConfiguration).toHaveBeenLastCalledWith(expect.objectContaining({
    computers: expect.arrayContaining([expect.objectContaining({ computer: "dev", repositoryMode: "all", identity: { name: "", email: "", apply: false } })]),
  }))
})



it("does not submit an incomplete author edit with repository changes", async () => {
  const source = applicationSourceForScenario("running")
  const { actions, user } = renderGitHub("running", source)

  const github = within(appPanel("GitHub"))
  await user.clear(github.getByLabelText("Git name for dev"))
  await user.click(github.getByRole("checkbox", { name: "All repositories for playgrounds" }))
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledOnce()
  // Only the changed computer is saved, so dev's unfinished author is not submitted.
  const [saved] = vi.mocked(actions.saveGitHubConfiguration!).mock.calls[0]
  expect(saved.computers.map(({ computer }) => computer)).toEqual(["playgrounds"])
  expect(github.getByLabelText("Git name for dev")).toHaveValue("")
})

it.each(["name", "email"] as const)("saves an explicit Git identity disable even when its %s is unfinished", async (field) => {
  const source = applicationSourceForScenario("running")
  const { actions, user } = renderGitHub("running", source)
  await user.clear(screen.getByLabelText(`Git ${field} for dev`))
  expect(actions.saveGitHubConfiguration).not.toHaveBeenCalled()

  await user.click(screen.getByRole("checkbox", { name: "Apply Git identity to dev" }))

  expect(screen.getByRole("checkbox", { name: "Apply Git identity to dev" })).not.toBeChecked()
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
    computers: [expect.objectContaining({ computer: "dev", identity: expect.objectContaining({ [field]: "", apply: false }) })],
  }))
})



it("saves only the edited computer against the shown revision and never turns access on", async () => {
  const source = applicationSourceForScenario("running", "connected")
  source.github.policyRevision = 10
  source.github.accessEnabled = false
  const { actions, user } = renderGitHub("running", source)

  const github = within(appPanel("GitHub"))
  const name = github.getByLabelText("Git name for playgrounds")
  await user.clear(name)
  await user.type(name, "Morgan Example")
  await user.tab()
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledOnce()
  const [saved] = vi.mocked(actions.saveGitHubConfiguration!).mock.calls[0]
  // Other computers (such as a fork's copied assignment) keep their saved choices, and a
  // save right after Disable access cannot re-enable it.
  expect(saved).toEqual({
    baseRevision: 10,
    deviceIdentity: source.github.deviceIdentity ?? null,
    computers: [expect.objectContaining({ computer: "playgrounds", identity: expect.objectContaining({ name: "Morgan Example" }) })],
  })
  expect(saved).not.toHaveProperty("accessEnabled")
})



it("settles a newer GitHub revision even when its completion matches the previous save", async () => {
  const source = applicationSourceForScenario("running", "connected")
  source.github.policyRevision = 10
  source.github.computerOperations = [{ computer: "dev", status: "succeeded", message: "GitHub access verified." }]
  const application = renderGitHub("running", source)

  const github = within(appPanel("GitHub"))
  await application.user.click(github.getByRole("checkbox", { name: "All repositories for dev" }))
  expect(await screen.findByText("Applying repository access…")).toBeVisible()
  const completed = structuredClone(source)
  completed.github.policyRevision = 11
  completed.github.computers = vi.mocked(application.actions.saveGitHubConfiguration!).mock.calls[0][0].computers
  application.rerender(<GitHubPanel source={completed} actions={application.actions} />)
  expect(await screen.findByText("GitHub settings applied")).toBeVisible()
  expect(screen.queryByText("Applying repository access…")).not.toBeInTheDocument()
  expect(github.getByRole("button", { name: "Disable for all computers" })).toBeEnabled()
})



it("stops applying and permits correction when a native GitHub save rejects", async () => {
  const { actions, user } = renderGitHub()
  vi.mocked(actions.saveGitHubConfiguration!).mockRejectedValueOnce(new Error("Invalid Git identity settings."))

  const github = within(appPanel("GitHub"))
  await user.click(github.getByRole("checkbox", { name: "All repositories for dev" }))
  expect(await screen.findByText(/Invalid Git identity settings\./)).toBeVisible()
  expect(screen.queryByText("Applying repository access…")).not.toBeInTheDocument()
  expect(github.getByRole("button", { name: /GitHub settings not applied for dev/ })).toBeVisible()
  expect(github.getByRole("button", { name: "Disable for all computers" })).toBeEnabled()
})



it("ignores a rejected save once a newer repository change is pending", async () => {
  const { actions, user } = renderGitHub()
  let rejectFirst!: (cause: Error) => void
  vi.mocked(actions.saveGitHubConfiguration!).mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectFirst = reject }))

  const github = within(appPanel("GitHub"))
  await user.click(github.getByRole("checkbox", { name: "All repositories for dev" }))
  await user.click(github.getByRole("checkbox", { name: "All repositories for dev" }))
  await act(async () => rejectFirst(new Error("Older request failed")))
  expect(screen.queryByText(/Older request failed/)).not.toBeInTheDocument()
  expect(github.queryByRole("button", { name: /GitHub settings not applied/ })).not.toBeInTheDocument()
  expect(screen.getByText("Applying repository access…")).toBeVisible()
})



it("settles all pending computer edits when the latest complete save fails, and retries that draft", async () => {
  const { actions, user } = renderGitHub()
  vi.mocked(actions.saveGitHubConfiguration!)
    .mockImplementationOnce(() => new Promise<void>(() => {}))
    .mockRejectedValueOnce(new Error("Settings could not be saved"))

  const github = within(appPanel("GitHub"))
  await user.click(github.getByRole("checkbox", { name: "All repositories for dev" }))
  await user.click(github.getByRole("checkbox", { name: "All repositories for playgrounds" }))
  expect(await github.findAllByRole("button", { name: /GitHub settings not applied/ })).toHaveLength(2)
  expect(screen.queryByText("Applying repository access…")).not.toBeInTheDocument()
  await user.click((await screen.findAllByRole("button", { name: "Retry" }))[0])
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledTimes(3)
  expect(actions.retryGitHubConfiguration).not.toHaveBeenCalled()
  expect(actions.saveGitHubConfiguration).toHaveBeenLastCalledWith(expect.objectContaining({
    computers: expect.arrayContaining([
      expect.objectContaining({ computer: "dev", repositoryMode: "all" }),
      expect.objectContaining({ computer: "playgrounds", repositoryMode: "all" }),
    ]),
  }))
})



it("disconnects the current GitHub account so another account can be connected", async () => {
  const { actions, user } = renderGitHub()

  const github = within(appPanel("GitHub"))

  await user.click(github.getByRole("button", { name: "Disconnect" }))
  expect(actions.disconnectGitHub).not.toHaveBeenCalled()
  expect(github.getByRole("button", { name: "Cancel" })).toBeVisible()
  expect(github.getByRole("button", { name: "Disconnect" })).toBeVisible()
  expect(github.queryByRole("heading", { name: "Not connected" })).not.toBeInTheDocument()
  await user.keyboard("{Escape}")
  expect(github.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument()
  expect(actions.disconnectGitHub).not.toHaveBeenCalled()

  await user.click(github.getByRole("button", { name: "Disconnect" }))
  fireEvent.pointerDown(github.getByLabelText("Git name for dev"))
  expect(github.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument()
  expect(actions.disconnectGitHub).not.toHaveBeenCalled()

  await user.click(github.getByRole("button", { name: "Disconnect" }))
  await user.click(github.getByRole("button", { name: "Cancel" }))
  expect(actions.disconnectGitHub).not.toHaveBeenCalled()
  expect(github.getByText("Connected as @taylor")).toBeVisible()
  await user.click(github.getByRole("button", { name: "Disconnect" }))
  await user.click(github.getByRole("button", { name: "Disconnect" }))
  expect(actions.disconnectGitHub).toHaveBeenCalledOnce()
  // A request alone must not claim that host access was revoked.
  expect(github.getByText("Connected as @taylor")).toBeVisible()
  expect(github.queryByRole("heading", { name: "Not connected" })).not.toBeInTheDocument()

})



it("applies access toggles immediately and confirms clearing one computer's repositories", async () => {
  const { actions, unmount, user } = renderGitHub()

  const github = within(appPanel("GitHub"))

  await user.click(github.getByRole("button", { name: "Clear repositories from dev" }))
  expect(screen.getByText("Remove all repositories from dev?")).toBeVisible()
  expect(screen.getByText("dev loses GitHub access to them.")).toBeVisible()
  expect(github.getByRole("table", { name: "Selected repositories for dev" })).toBeVisible()
  await user.keyboard("{Escape}")
  await waitFor(() => expect(screen.queryByText("Remove all repositories from dev?")).not.toBeInTheDocument())
  expect(github.getByRole("table", { name: "Selected repositories for dev" })).toBeVisible()
  await user.click(github.getByRole("button", { name: "Clear repositories from dev" }))
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  await waitFor(() => expect(screen.queryByText("Remove all repositories from dev?")).not.toBeInTheDocument())
  expect(github.getByRole("table", { name: "Selected repositories for dev" })).toBeVisible()
  await user.click(github.getByRole("button", { name: "Clear repositories from dev" }))
  await user.click(screen.getByRole("button", { name: "Remove all" }))
  expect(github.queryByRole("table", { name: "Selected repositories for dev" })).not.toBeInTheDocument()
  expect(github.getByRole("table", { name: "Selected repositories for playgrounds" })).toBeVisible()
  expect(github.getByRole("table", { name: "Selected repositories for personal" })).toBeVisible()
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledOnce()
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledWith(expect.objectContaining({
    computers: expect.arrayContaining([expect.objectContaining({ computer: "dev", repositories: [] })]),
  }))
  unmount()

  const disabled = renderGitHub(
    "running",
    applicationSourceForScenario("running", "connected", undefined, undefined, undefined, undefined, undefined, 0, "disabled"),
  )

  const disabledPanel = within(appPanel("GitHub"))
  expect(disabledPanel.getByRole("button", { name: "Enable for all computers" })).toBeVisible()
  expect(disabledPanel.getByRole("table", { name: "Selected repositories for dev" })).toBeVisible()
  expect(disabledPanel.getByRole("button", { name: "Clear repositories from dev" })).toBeDisabled()
  await disabled.user.click(disabledPanel.getByRole("button", { name: "Enable for all computers" }))
  expect(disabled.actions.setGitHubAccessEnabled).toHaveBeenCalledWith(true)
})



it("keeps obsolete computer errors compact and copies only safe explanations", async () => {
  const source = applicationSourceForScenario("running", "connected")
  source.github.computerOperations = [{ computer: "dev", status: "failed", canRetry: true,
    message: 'Recreate this development computer to enable the new GitHub integration. Git identity: {"before":"GIT_AUTHOR_EMAIL=private@example.com","disposition":"requires restart"}',
    diagnosticDetails: "secret runtime output",
  }]
  const application = renderGitHub("running", source)

  const label = within(appPanel("GitHub")).getByRole("button", { name: /GitHub settings not applied for dev/ })
  expect(label).toHaveTextContent("Not applied")
  await application.user.click(label)
  const details = within(await screen.findByRole("dialog"))
  expect(details.getByText("This computer needs a new setup for GitHub access.")).toBeVisible()
  expect(details.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument()
  expect(details.getByText(/A restart alone does not resolve/)).toBeVisible()
  expect(details.queryByText(/private@example|secret runtime|GIT_AUTHOR_EMAIL/)).not.toBeInTheDocument()
  const copy = vi.spyOn(navigator.clipboard, "writeText")
  await application.user.click(details.getByRole("button", { name: "Copy details" }))
  expect(copy).toHaveBeenCalledWith(expect.stringContaining("Git identity:"))
  expect(copy.mock.calls.at(-1)?.[0]).not.toMatch(/private@example|secret runtime|GIT_AUTHOR_EMAIL/)
})



it("keeps a successful GitHub apply notification until it is closed", async () => {
  vi.useFakeTimers()
  const application = renderGitHub("running", applicationSourceForScenario("running", "connected"))

  try {

    const succeeded = applicationSourceForScenario("running", "connected", undefined, undefined, undefined, undefined, undefined, 0, "succeeded")
    succeeded.github.policyRevision = 5
    fireEvent.click(screen.getByRole("button", { name: "Disable for all computers" }))
    application.rerender(<GitHubPanel source={succeeded} actions={application.actions} />)
    await act(async () => { await vi.advanceTimersByTimeAsync(50) })
    expect(screen.getByText("GitHub settings applied")).toBeVisible()

    await act(async () => { await vi.advanceTimersByTimeAsync(60_000) })
    expect(screen.getByText("GitHub settings applied")).toBeVisible()

    fireEvent.click(screen.getByRole("button", { name: "Close toast" }))
    await act(async () => { await vi.advanceTimersByTimeAsync(1_000) })
    expect(screen.queryByText("GitHub settings applied")).not.toBeInTheDocument()
  } finally {
    application.unmount()
    vi.useRealTimers()
  }
})



it("allows manual identity without host identity and explains unavailable repository catalog data", async () => {
  const source = applicationSourceForScenario("running", "connected", undefined, undefined, undefined, undefined, undefined, 0, "missing-device-identity")
  const { unmount } = renderGitHub("running", source)

  const github = within(appPanel("GitHub"))

  expect(github.getByRole("button", { name: "Reset Git identity for dev" })).toBeDisabled()
  expect(github.getByLabelText("Git name for dev")).toBeEnabled()
  expect(github.getByLabelText("Git name for dev")).toHaveValue("")
  unmount()

  const connectedEmpty = renderGitHub(
    "running",
    applicationSourceForScenario("running", "connected", undefined, undefined, undefined, undefined, undefined, 0, "connected-empty"),
  )

  const emptyPanel = within(appPanel("GitHub"))
  expect(emptyPanel.queryByRole("table", { name: "Selected repositories for dev" })).not.toBeInTheDocument()
  expect(emptyPanel.getByRole("combobox", { name: "Add repository to dev" })).toBeVisible()
  connectedEmpty.unmount()

  const catalogUnavailable = renderGitHub(
    "running",
    applicationSourceForScenario("running", "connected", undefined, undefined, undefined, undefined, undefined, 0, "catalog-unavailable"),
  )

  const unavailablePanel = within(appPanel("GitHub"))
  expect(unavailablePanel.getByRole("alert")).toHaveTextContent("GitHub repositories could not be loaded.")
  expect(unavailablePanel.queryByRole("combobox", { name: "Add repository to dev" })).not.toBeInTheDocument()
  await catalogUnavailable.user.click(unavailablePanel.getByRole("button", { name: "Retry repositories" }))
  expect(catalogUnavailable.actions.retryGitHubRepositoryCatalog).toHaveBeenCalledOnce()
})



it("keeps failed sign-in retry on Connect GitHub instead of repository refresh", async () => {
  const source = applicationSourceForScenario("running", "disconnected")
  source.github.repositoryCatalogStatus = { status: "unavailable", message: "GitHub connection is not configured in this build.", canRetry: false }
  renderGitHub("running", source)

  const github = within(appPanel("GitHub"))
  expect(github.getByRole("alert")).toHaveTextContent("GitHub connection is not configured in this build.")
  expect(github.getByRole("button", { name: "Connect GitHub" })).toBeEnabled()
  expect(github.queryByRole("button", { name: "Retry repositories" })).not.toBeInTheDocument()
})


it.each(["selected", "all"] as const)("retries rejected repository intent after refresh from %s mode", async (repositoryMode) => {
  const source = applicationSourceForScenario("running", "connected")
  source.github.policyRevision = 10
  source.github.computers = source.github.computers!.map((policy) => ({ ...policy, repositoryMode }))
  const application = renderGitHub("running", source)
  let resolveFirst!: () => void
  let rejectSecond!: (cause: Error) => void
  vi.mocked(application.actions.saveGitHubConfiguration!)
    .mockImplementationOnce(() => new Promise<void>((resolve) => { resolveFirst = resolve }))
    .mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectSecond = reject }))

  const toggle = screen.getByRole("checkbox", { name: "All repositories for dev" })
  await application.user.click(toggle)
  await application.user.click(toggle)
  const first = vi.mocked(application.actions.saveGitHubConfiguration!).mock.calls[0][0]
  await act(async () => {
    resolveFirst()
    rejectSecond(new Error("GitHub settings changed. Try again."))
  })
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible()

  const refreshed = structuredClone(source)
  refreshed.github.policyRevision = 11
  refreshed.github.computers = source.github.computers!.map((policy) => first.computers.find((saved) => saved.computer === policy.computer) ?? policy)
  application.rerender(<GitHubPanel source={refreshed} actions={application.actions} />)
  await application.user.click(screen.getByRole("button", { name: "Retry" }))
  expect(application.actions.saveGitHubConfiguration).toHaveBeenLastCalledWith(expect.objectContaining({
    baseRevision: 11,
    computers: [expect.objectContaining({ computer: "dev", repositoryMode })],
  }))
})

it("retries interleaved repository and identity intents after an authoritative refresh without submitting unfinished text", async () => {
  const source = applicationSourceForScenario("running", "connected")
  source.github.policyRevision = 10
  const application = renderGitHub("running", source)
  vi.mocked(application.actions.saveGitHubConfiguration!)
    .mockImplementationOnce(() => new Promise<void>(() => {}))
    .mockRejectedValueOnce(new Error("GitHub settings changed. Try again."))
  await application.user.click(screen.getByRole("button", { name: "Remove acme/silo from dev" }))
  const name = screen.getByLabelText("Git name for playgrounds")
  await application.user.clear(name)
  await application.user.type(name, "Submitted Author")
  await application.user.tab()
  expect(await screen.findAllByRole("button", { name: "Retry" })).toHaveLength(2)
  const submitted = vi.mocked(application.actions.saveGitHubConfiguration!).mock.calls[1][0]
  await application.user.clear(name)
  await application.user.type(name, "Unfinished Author")

  const refreshed = structuredClone(source)
  refreshed.github.policyRevision = 11
  refreshed.github.computers = refreshed.github.computers!.map((policy) => ({
    ...policy,
    identity: { ...policy.identity, name: "Saved Author" },
  }))
  application.rerender(<GitHubPanel source={refreshed} actions={application.actions} />)
  expect(screen.getByLabelText("Git name for playgrounds")).toHaveValue("Unfinished Author")
  // Keep the unfinished input focused so retry does not submit it through blur.
  fireEvent.click(screen.getAllByRole("button", { name: "Retry" })[0])
  await waitFor(() => expect(application.actions.saveGitHubConfiguration).toHaveBeenCalledTimes(3))
  expect(application.actions.saveGitHubConfiguration).toHaveBeenLastCalledWith({
    ...submitted,
    baseRevision: 11,
  })
})
