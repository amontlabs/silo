import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { GitHubAccessEditor, type GitHubTokenOption } from "./github-access-editor"

const work: GitHubTokenOption = { id: "token-work", name: "work", account: "alice", connected: true }
const home: GitHubTokenOption = { id: "token-home", name: "home", account: "bob", connected: true }
const stale: GitHubTokenOption = { id: "token-stale", name: "stale", connected: false }

function propsWith(change = vi.fn()) {
  return {
    computers: [{ name: "dev" }], connectionState: "connected" as const,
    tokenSecrets: [] as readonly GitHubTokenOption[], repositoryOptions: [], computerSelections: {}, computerIdentities: {},
    currentDeviceGitIdentity: null, onConnect: vi.fn(), onComputerSelectionsChange: vi.fn(),
    onComputerIdentityChange: vi.fn(), onResetComputerIdentity: vi.fn(),
    onComputerRepositoryAccessChange: change,
  }
}
const tokenAccess = (tokenSecretId?: string) => ({ dev: {
  authenticationMethod: "token" as const, ...(tokenSecretId ? { tokenSecretId } : {}), repositoryMode: "all" as const, allRepositoriesAllowChanges: true,
} })

it("defaults to OAuth, gates each connection independently, and hides OAuth restrictions for tokens", async () => {
  const user = userEvent.setup()
  const change = vi.fn()
  const props = propsWith(change)
  const { rerender } = render(<GitHubAccessEditor {...props} />)
  expect(screen.getByRole("radio", { name: "Use GitHub OAuth for dev" })).toBeChecked()
  expect(screen.getByRole("radio", { name: "Use token for dev" })).toBeDisabled()
  await user.hover(screen.getByText("Use token", { exact: true }))
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Full token access.")
  await user.unhover(screen.getByText("Use token", { exact: true }))
  rerender(<GitHubAccessEditor {...props} connectionState="disconnected" tokenSecrets={[work]} />)
  expect(screen.getByRole("radio", { name: "Use GitHub OAuth for dev" })).toBeDisabled()
  await user.click(screen.getByRole("radio", { name: "Use token for dev" }))
  expect(change).toHaveBeenCalledWith("dev", expect.objectContaining({ authenticationMethod: "token", tokenSecretId: "token-work" }))
  rerender(<GitHubAccessEditor {...props} tokenSecrets={[work]} computerRepositoryAccess={tokenAccess("token-work")} />)
  expect(screen.getByRole("radio", { name: "Use token for dev" })).toBeChecked()
  expect(screen.queryByRole("checkbox", { name: "All repositories for dev" })).not.toBeInTheDocument()
  expect(screen.getByText(/Full token permissions/)).toBeInTheDocument()
  rerender(<GitHubAccessEditor {...props} computerRepositoryAccess={tokenAccess("token-work")} />)
  expect(screen.getByRole("radio", { name: "Use token for dev" })).toBeChecked()
  expect(screen.getByRole("radio", { name: "Use token for dev" })).toBeDisabled()
  expect(screen.getByRole("radio", { name: "Use GitHub OAuth for dev" })).not.toBeChecked()
})

it("chooses one of the token secrets by name for a computer", async () => {
  const user = userEvent.setup()
  const change = vi.fn()
  render(<GitHubAccessEditor {...propsWith(change)} tokenSecrets={[work, home, stale]} computerRepositoryAccess={tokenAccess("token-work")} />)
  const choice = screen.getByRole("combobox", { name: "GitHub token for dev" })
  expect(choice).toHaveValue("token-work")
  expect(screen.getByRole("option", { name: "work · @alice" })).toBeInTheDocument()
  expect(screen.getByRole("option", { name: "home · @bob" })).toBeInTheDocument()
  // A disconnected token is listed but cannot be chosen.
  expect(screen.getByRole("option", { name: "stale (disconnected)" })).toBeDisabled()
  await user.selectOptions(choice, "token-home")
  expect(change).toHaveBeenCalledWith("dev", expect.objectContaining({ authenticationMethod: "token", tokenSecretId: "token-home" }))
})

it("keeps a disconnected or removed choice visible and drops the token when returning to sign-in", async () => {
  const user = userEvent.setup()
  const change = vi.fn()
  const props = propsWith(change)
  const { rerender } = render(<GitHubAccessEditor {...props} tokenSecrets={[stale]} computerRepositoryAccess={tokenAccess("token-stale")} />)
  expect(screen.getByRole("combobox", { name: "GitHub token for dev" })).toHaveValue("token-stale")
  expect(screen.getByRole("option", { name: "stale (disconnected)" })).toBeInTheDocument()
  rerender(<GitHubAccessEditor {...props} tokenSecrets={[work]} computerRepositoryAccess={tokenAccess("token-gone")} />)
  expect(screen.getByRole("combobox", { name: "GitHub token for dev" })).toHaveValue("token-gone")
  expect(screen.getByRole("option", { name: "Removed token" })).toBeDisabled()
  await user.click(screen.getByRole("radio", { name: "Use GitHub OAuth for dev" }))
  expect(change).toHaveBeenCalledWith("dev", expect.objectContaining({ authenticationMethod: "oauth", tokenSecretId: undefined }))
})

it("asks for a token when token access names none", () => {
  render(<GitHubAccessEditor {...propsWith()} tokenSecrets={[work]} computerRepositoryAccess={tokenAccess()} />)
  expect(screen.getByRole("combobox", { name: "GitHub token for dev" })).toHaveValue("")
  expect(screen.getByRole("option", { name: "Choose a token…" })).toBeDisabled()
})

it("says when the token a computer chose no longer exists", () => {
  const props = propsWith()
  const { rerender } = render(<GitHubAccessEditor {...props} computers={[{ name: "dev", tokenMissing: true }]} tokenSecrets={[work]} computerRepositoryAccess={tokenAccess("token-gone")} />)
  expect(screen.getByRole("alert")).toHaveTextContent("The token this computer used was removed, so it has no GitHub access. Choose another token.")
  rerender(<GitHubAccessEditor {...props} computers={[{ name: "dev" }]} tokenSecrets={[work]} computerRepositoryAccess={tokenAccess("token-work")} />)
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})
