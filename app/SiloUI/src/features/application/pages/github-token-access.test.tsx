import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import type { ApplicationSecret } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createApplicationActionsMock } from "@/test/application-actions"
import { GitHubPage } from "./github-page"

const token = (id: string, name: string, extra: Partial<ApplicationSecret> = {}): ApplicationSecret => ({
  id, name, kind: "github-token", computers: [], allowedDomains: [], state: "active",
  account: `${name}-user`, connection: { state: "connected" }, ...extra,
})

function tokenSource(state: "disconnected" | "connecting" | "connected", enabled: boolean) {
  const source = applicationSourceForScenario("running", "connected")
  source.github.state = state
  source.github.accessEnabled = enabled
  source.secrets = [token("token-work", "work"), token("token-home", "home")]
  source.github.computers = source.github.computers!.map(policy => ({ ...policy, authenticationMethod: "token", tokenSecretId: "token-work" }))
  return source
}

it.each([
  { state: "disconnected", enabled: false },
  { state: "disconnected", enabled: true },
  { state: "connecting", enabled: false },
  { state: "connecting", enabled: true },
] as const)("toggles token access independently of OAuth ($state, enabled=$enabled)", async ({ state, enabled }) => {
  const user = userEvent.setup()
  const source = tokenSource(state, enabled)
  const actions = createApplicationActionsMock()
  const view = render(<GitHubPage source={source} actions={actions} />)
  expect(screen.getByRole("radio", { name: "Use token for dev" })).toBeChecked()
  await user.click(screen.getByRole("button", { name: enabled ? "Disable for all computers" : "Enable for all computers" }))
  expect(actions.setGitHubAccessEnabled).toHaveBeenCalledExactlyOnceWith(!enabled)
  expect(actions.connectGitHub).not.toHaveBeenCalled()
  view.rerender(<GitHubPage source={{ ...source, github: { ...source.github, accessEnabled: !enabled } }} actions={actions} />)
  expect(screen.getByRole("button", { name: enabled ? "Enable for all computers" : "Disable for all computers" })).toBeEnabled()
})

it("names the token each computer uses and saves a change of token for that computer only", async () => {
  const user = userEvent.setup()
  const source = tokenSource("connected", true)
  const actions = createApplicationActionsMock()
  render(<GitHubPage source={source} actions={actions} />)
  const choice = screen.getByRole("combobox", { name: "GitHub token for dev" })
  expect(choice).toHaveValue("token-work")
  expect(within(choice).getByRole("option", { name: "home · @home-user" })).toBeInTheDocument()
  await user.selectOptions(choice, "token-home")
  expect(actions.saveGitHubConfiguration).toHaveBeenCalledTimes(1)
  const saved = lastConfiguration(actions)
  expect(saved.computers).toHaveLength(1)
  expect(saved.computers[0]).toMatchObject({ computer: "dev", authenticationMethod: "token", tokenSecretId: "token-home" })
})

it("has no separate connection panel for a single token", () => {
  render(<GitHubPage source={tokenSource("connected", true)} actions={createApplicationActionsMock()} />)
  expect(screen.queryByRole("button", { name: /Add token|Replace token|Remove token/ })).not.toBeInTheDocument()
  expect(screen.getByText(/Personal tokens are added in Secrets/)).toBeInTheDocument()
})

it("does not offer a disconnected or removed token as a new choice", () => {
  const source = tokenSource("connected", true)
  source.secrets = [token("token-work", "work", { connection: { state: "disconnected", message: "GitHub rejected it." } }), token("token-gone", "gone", { removing: true })]
  render(<GitHubPage source={source} actions={createApplicationActionsMock()} />)
  expect(screen.getAllByRole("option", { name: "work · @work-user (disconnected)" }).length).toBeGreaterThan(0)
  expect(screen.queryByRole("option", { name: /gone/ })).not.toBeInTheDocument()
})

function lastConfiguration(actions: ReturnType<typeof createApplicationActionsMock>) {
  const calls = (actions.saveGitHubConfiguration as unknown as { mock: { calls: unknown[][] } }).mock.calls
  return calls[calls.length - 1][0] as { computers: Array<Record<string, unknown>> }
}
