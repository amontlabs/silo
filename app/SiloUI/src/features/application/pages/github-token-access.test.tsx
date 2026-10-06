import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createApplicationActionsMock } from "@/test/application-actions"
import { GitHubPage } from "./github-page"

it.each([
  { state: "disconnected", enabled: false },
  { state: "disconnected", enabled: true },
  { state: "connecting", enabled: false },
  { state: "connecting", enabled: true },
] as const)("toggles token access independently of OAuth ($state, enabled=$enabled)", async ({ state, enabled }) => {
  const user = userEvent.setup()
  const source = applicationSourceForScenario("running", "connected")
  source.github.state = state
  source.github.accessEnabled = enabled
  source.github.personalToken = { state: "connected", saved: true, account: "token-user" }
  source.github.computers = source.github.computers!.map(policy => ({ ...policy, authenticationMethod: "token" }))
  const actions = createApplicationActionsMock()
  const view = render(<GitHubPage source={source} actions={actions} />)
  expect(screen.getByRole("radio", { name: "Use token for dev" })).toBeChecked()
  await user.click(screen.getByRole("button", { name: enabled ? "Disable for all computers" : "Enable for all computers" }))
  expect(actions.setGitHubAccessEnabled).toHaveBeenCalledExactlyOnceWith(!enabled)
  expect(actions.connectGitHub).not.toHaveBeenCalled()
  view.rerender(<GitHubPage source={{ ...source, github: { ...source.github, accessEnabled: !enabled } }} actions={actions} />)
  expect(screen.getByRole("button", { name: enabled ? "Enable for all computers" : "Disable for all computers" })).toBeEnabled()
})
