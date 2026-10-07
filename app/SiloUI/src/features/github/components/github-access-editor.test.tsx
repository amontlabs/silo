import { fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { GitHubAccessEditor } from "@/features/github/components/github-access-editor"
import { GitHubPage } from "@/features/application/pages/github-page"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"

describe("GitHubAccessEditor", () => {
  it.each(["name", "email"])("does not commit Git %s while confirming an IME candidate", async (field) => {
    const user = userEvent.setup()
    const onCommit = vi.fn()
    render(<GitHubAccessEditor
      computers={[{ name: "dev" }]} connectionState="connected"
      repositoryOptions={[]} computerSelections={{}}
      computerIdentities={{ dev: { name: "Taylor", email: "taylor@example.com", apply: true } }}
      currentDeviceGitIdentity={null} onConnect={vi.fn()}
      onComputerSelectionsChange={vi.fn()} onComputerIdentityChange={vi.fn()} onResetComputerIdentity={vi.fn()}
      onCommitComputerIdentity={onCommit}
    />)
    const input = screen.getByRole("textbox", { name: `Git ${field} for dev` })
    await user.click(input)
    fireEvent.keyDown(input, { key: "Enter", isComposing: true })
    expect(input).toHaveFocus()
    expect(onCommit).not.toHaveBeenCalled()
    await user.keyboard("{Enter}")
    expect(input).not.toHaveFocus()
    expect(onCommit).toHaveBeenCalledOnce()
  })

  it("sizes the computer list to its content instead of filling the page", () => {
    render(<GitHubAccessEditor
      computers={[{ name: "dev" }]} connectionState="disconnected"
      repositoryOptions={[]} computerSelections={{}} computerIdentities={{}}
      currentDeviceGitIdentity={null} onConnect={vi.fn()}
      onComputerSelectionsChange={vi.fn()} onComputerIdentityChange={vi.fn()} onResetComputerIdentity={vi.fn()}
      onComputerRepositoryAccessChange={vi.fn()}
    />)
    const list = screen.getByRole("region", { name: "Computer Git identity and repository access" })
    expect(list).not.toHaveClass("flex-1")
    expect(list).toHaveClass("min-h-0")
  })

  it.each(["ArrowDown", "ArrowUp", "Enter", "Escape"])("keeps repository selection unchanged for composing %s", async (key) => {
    const user = userEvent.setup()
    const onSelections = vi.fn()
    render(<GitHubAccessEditor
      computers={[{ name: "dev" }]} connectionState="connected"
      repositoryOptions={["acme/first", "acme/second"]} computerSelections={{}} computerIdentities={{}}
      currentDeviceGitIdentity={null} onConnect={vi.fn()}
      onComputerSelectionsChange={onSelections} onComputerIdentityChange={vi.fn()} onResetComputerIdentity={vi.fn()}
    />)
    const input = screen.getByRole("combobox", { name: "Add repository to dev" })
    await user.click(input)
    const active = input.getAttribute("aria-activedescendant")
    const event = new KeyboardEvent("keydown", { key, isComposing: true, bubbles: true, cancelable: true })
    fireEvent(input, event)
    expect(event.defaultPrevented).toBe(key === "Escape")
    expect(input).toHaveAttribute("aria-activedescendant", active)
    expect(screen.getByRole("listbox")).toBeInTheDocument()
    expect(onSelections).not.toHaveBeenCalled()
    await user.keyboard("{ArrowDown}{Enter}")
    expect(onSelections).toHaveBeenCalledExactlyOnceWith("dev", [{ repository: "acme/second", allowPushes: false }])
  })

  it("does not rescan an unchanged catalog during unrelated renders and picks from a replacement catalog", async () => {
    const user = userEvent.setup()
    let catalogReads = 0
    const repositoryOptions = new Proxy(Array.from({ length: 1000 }, (_, index) => `acme/repo-${index}`), {
      get(target, property, receiver) {
        if (typeof property === "string" && /^\d+$/.test(property)) catalogReads++
        return Reflect.get(target, property, receiver)
      },
    })
    const onSelections = vi.fn()
    const props = {
      computers: [{ name: "dev" }], connectionState: "connected" as const,
      repositoryOptions, computerSelections: {}, computerIdentities: {},
      currentDeviceGitIdentity: null, onConnect: vi.fn(), onComputerSelectionsChange: onSelections,
      onComputerIdentityChange: vi.fn(), onResetComputerIdentity: vi.fn(),
    }
    const view = render(<GitHubAccessEditor {...props} />)
    catalogReads = 0
    for (let tick = 0; tick < 10; tick++) {
      view.rerender(<GitHubAccessEditor {...props} notice={<p>Refresh {tick}</p>} />)
    }
    expect(catalogReads).toBe(0)

    view.rerender(<GitHubAccessEditor {...props} repositoryOptions={["acme/new"]} />)
    await user.click(screen.getByRole("combobox"))
    await user.keyboard("{Enter}")
    expect(onSelections).toHaveBeenCalledExactlyOnceWith("dev", [{ repository: "acme/new", allowPushes: false }])
  })

  it.each([{ repositoryOptions: [] }, { repositoryOptions: ["acme/silo"] }])("excludes repository suggestions and GitHub authorization from the page Tab order (%j)", async ({ repositoryOptions }) => {
    const user = userEvent.setup()
    render(<GitHubAccessEditor
      computers={[{ name: "dev" }]} connectionState="connected"
      repositoryOptions={repositoryOptions} computerSelections={{}} computerIdentities={{}}
      currentDeviceGitIdentity={null} onConnect={vi.fn()}
      onComputerSelectionsChange={vi.fn()} onComputerIdentityChange={vi.fn()} onResetComputerIdentity={vi.fn()}
      onManageRepositories={vi.fn()}
    />)
    await user.click(screen.getByRole("combobox", { name: "Add repository to dev" }))
    expect(screen.getAllByRole("option")).toHaveLength(repositoryOptions.length + 1)
    await user.tab()
    expect(document.activeElement).toBe(document.body)
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument()
  })

  it("reveals arrow-key repository selections and the authorization action while keeping input focus", async () => {
    const user = userEvent.setup()
    const scroll = vi.spyOn(Element.prototype, "scrollIntoView")
    const onManage = vi.fn()
    render(<GitHubAccessEditor
      computers={[{ name: "dev" }]} connectionState="connected"
      repositoryOptions={Array.from({ length: 30 }, (_, index) => `acme/repo-${index}`)}
      computerSelections={{}} computerIdentities={{}} currentDeviceGitIdentity={null}
      onConnect={vi.fn()} onComputerSelectionsChange={vi.fn()}
      onComputerIdentityChange={vi.fn()} onResetComputerIdentity={vi.fn()} onManageRepositories={onManage}
    />)
    const input = screen.getByRole("combobox", { name: "Add repository to dev" })
    await user.click(input)
    await user.keyboard("{ArrowDown>20/}")
    const repository = screen.getByRole("option", { name: "acme/repo-20", selected: true })
    expect(input).toHaveAttribute("aria-activedescendant", repository.id)
    expect(input).toHaveFocus()
    expect(scroll.mock.contexts.at(-1)).toBe(repository)
    await user.keyboard("{ArrowDown>10/}")
    const authorize = screen.getByRole("option", { name: "Add more repositories on GitHub", selected: true })
    expect(scroll.mock.contexts.at(-1)).toBe(authorize)
    await user.keyboard("{ArrowUp}")
    expect(scroll.mock.contexts.at(-1)).toBe(screen.getByRole("option", { name: "acme/repo-29", selected: true }))
    expect(input).toHaveFocus()
    await user.keyboard("{ArrowDown}{Enter}")
    expect(onManage).toHaveBeenCalledOnce()
  })

  it("uses empty defaults for an incoming computer named constructor", () => {
    const props = {
      computers: [{ name: "dev" }], connectionState: "connected" as const,
      repositoryOptions: ["acme/silo"], computerSelections: { dev: [] }, computerIdentities: {},
      currentDeviceGitIdentity: null, onConnect: vi.fn(), onComputerSelectionsChange: vi.fn(),
      onComputerIdentityChange: vi.fn(), onResetComputerIdentity: vi.fn(),
      onComputerRepositoryAccessChange: vi.fn(),
    }
    const view = render(<GitHubAccessEditor {...props} />)
    view.rerender(<GitHubAccessEditor {...props} computers={[{ name: "dev" }, { name: "constructor" }]} />)
    expect(screen.getByLabelText("Git name for constructor")).toHaveValue("")
    expect(screen.getByLabelText("Git email for constructor")).toHaveValue("")
    expect(screen.getByRole("checkbox", { name: "All repositories for constructor" })).not.toBeChecked()
    expect(screen.getByRole("combobox", { name: "Add repository to constructor" })).toBeEnabled()
    expect(screen.queryByRole("table", { name: "Selected repositories for constructor" })).not.toBeInTheDocument()
  })

  it("uses saved GitHub settings for a computer named constructor", () => {
    render(<GitHubAccessEditor
      computers={[{ name: "constructor" }]} connectionState="connected"
      repositoryOptions={["acme/silo"]}
      computerSelections={{ constructor: [{ repository: "acme/silo", allowPushes: true }] }}
      computerIdentities={{ constructor: { name: "Taylor", email: "taylor@example.com", apply: false } }}
      computerRepositoryAccess={{ constructor: { repositoryMode: "selected" as const, allRepositoriesAllowChanges: false } }}
      currentDeviceGitIdentity={null} onConnect={vi.fn()}
      onComputerSelectionsChange={vi.fn()} onComputerIdentityChange={vi.fn()} onResetComputerIdentity={vi.fn()}
      onComputerRepositoryAccessChange={vi.fn()}
    />)
    expect(screen.getByLabelText("Git name for constructor")).toHaveValue("Taylor")
    expect(screen.getByLabelText("Git email for constructor")).toHaveValue("taylor@example.com")
    expect(screen.getByRole("checkbox", { name: "Apply Git identity to constructor" })).not.toBeChecked()
    expect(screen.getByRole("checkbox", { name: "Allow GitHub changes for acme/silo" })).toBeChecked()
  })

  it("accepts a newly discovered constructor computer before the page draft catches up", () => {
    const source = applicationSourceForScenario("running", "connected")
    const actions = {} as ApplicationActions
    const view = render(<GitHubPage source={source} actions={actions} />)
    const computer = source.computers.find(item => !item.device)!
    const incoming = { ...source, computers: [...source.computers, {
      ...computer, configuration: { ...computer.configuration, name: "constructor", id: "new-constructor" },
    }] }
    view.rerender(<GitHubPage source={incoming} actions={actions} />)
    expect(screen.getByLabelText("Git name for constructor")).toBeInTheDocument()
    expect(screen.getByRole("combobox", { name: "Add repository to constructor" })).toBeEnabled()
  })

  it("keeps the highlighted repository when the catalog order changes", async () => {
    const user = userEvent.setup()
    const onSelections = vi.fn()
    const props = {
      computers: [{ name: "dev" }], connectionState: "connected" as const,
      repositoryOptions: ["acme/base", "acme/silo"], computerSelections: {}, computerIdentities: {},
      currentDeviceGitIdentity: null, onConnect: vi.fn(), onComputerSelectionsChange: onSelections,
      onComputerIdentityChange: vi.fn(), onResetComputerIdentity: vi.fn(),
    }
    const view = render(<GitHubAccessEditor {...props} />)
    await user.click(screen.getByRole("combobox"))
    await user.keyboard("{ArrowDown}")
    expect(screen.getByRole("option", { name: "acme/silo" })).toHaveAttribute("aria-selected", "true")
    view.rerender(<GitHubAccessEditor {...props} repositoryOptions={["acme/base", "acme/other", "acme/silo"]} />)
    await user.keyboard("{Enter}")
    expect(onSelections).toHaveBeenCalledExactlyOnceWith("dev", [{ repository: "acme/silo", allowPushes: false }])
  })

  it("does not add a different repository when the highlighted result disappears", async () => {
    const user = userEvent.setup()
    const onSelections = vi.fn()
    const props = {
      computers: [{ name: "dev" }], connectionState: "connected" as const,
      repositoryOptions: ["acme/silo", "acme/other"], computerSelections: {}, computerIdentities: {},
      currentDeviceGitIdentity: null, onConnect: vi.fn(), onComputerSelectionsChange: onSelections,
      onComputerIdentityChange: vi.fn(), onResetComputerIdentity: vi.fn(),
    }
    const view = render(<GitHubAccessEditor {...props} />)
    await user.click(screen.getByRole("combobox"))
    view.rerender(<GitHubAccessEditor {...props} repositoryOptions={["acme/other"]} />)
    await user.keyboard("{Enter}")
    expect(onSelections).not.toHaveBeenCalled()
    expect(screen.getByRole("combobox")).not.toHaveAttribute("aria-activedescendant")
    await user.keyboard("{ArrowDown}{Enter}")
    expect(onSelections).toHaveBeenCalledExactlyOnceWith("dev", [{ repository: "acme/other", allowPushes: false }])
  })

  it("keeps GitHub authorization highlighted when repository results change", async () => {
    const user = userEvent.setup()
    const onSelections = vi.fn()
    const onManage = vi.fn()
    const props = {
      computers: [{ name: "dev" }], connectionState: "connected" as const,
      repositoryOptions: ["acme/silo"], computerSelections: {}, computerIdentities: {},
      currentDeviceGitIdentity: null, onConnect: vi.fn(), onComputerSelectionsChange: onSelections,
      onComputerIdentityChange: vi.fn(), onResetComputerIdentity: vi.fn(), onManageRepositories: onManage,
    }
    const view = render(<GitHubAccessEditor {...props} />)
    await user.click(screen.getByRole("combobox"))
    await user.keyboard("{ArrowDown}")
    expect(screen.getByRole("option", { name: "Add more repositories on GitHub" })).toHaveAttribute("aria-selected", "true")
    view.rerender(<GitHubAccessEditor {...props} repositoryOptions={["acme/silo", "acme/other"]} />)
    await user.keyboard("{Enter}")
    expect(onManage).toHaveBeenCalledOnce()
    expect(onSelections).not.toHaveBeenCalled()
  })

  it("offers repository authorization as the final search item, including empty results", async () => {
    const user = userEvent.setup()
    const onManageRepositories = vi.fn()
    render(<GitHubAccessEditor
      computers={[{ name: "dev" }]} connectionState="connected"
      repositoryOptions={["acme/silo"]} computerSelections={{}} computerIdentities={{}}
      currentDeviceGitIdentity={null} onConnect={vi.fn()}
      onComputerSelectionsChange={vi.fn()} onComputerIdentityChange={vi.fn()} onResetComputerIdentity={vi.fn()}
      onManageRepositories={onManageRepositories}
    />)
    expect(screen.queryByRole("option", { name: "Add more repositories on GitHub" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("combobox"))
    expect(screen.getAllByRole("option").at(-1)).toHaveTextContent("Add more repositories on GitHub")
    await user.keyboard("{ArrowDown}{Enter}")
    expect(onManageRepositories).toHaveBeenCalledOnce()
    onManageRepositories.mockClear()
    await user.type(screen.getByRole("combobox"), "missing-repository")
    expect(screen.getByText("No repositories found")).toBeVisible()
    await user.click(screen.getByRole("option", { name: "Add more repositories on GitHub" }))
    expect(onManageRepositories).toHaveBeenCalledOnce()
    await user.click(screen.getByRole("combobox"))
    expect(screen.queryByRole("option", { name: "Refresh repositories" })).not.toBeInTheDocument()
  })

  it("chooses all current and future authorized repositories with changes off by default", async () => {
    const user = userEvent.setup()
    const onAccess = vi.fn()
    const props = {
      computers: [{ name: "dev" }], connectionState: "connected" as const,
      repositoryOptions: ["acme/silo"], computerSelections: { dev: [{ repository: "acme/silo", allowPushes: true }] },
      computerIdentities: {}, currentDeviceGitIdentity: null,
      onConnect: vi.fn(), onComputerSelectionsChange: vi.fn(), onComputerIdentityChange: vi.fn(), onResetComputerIdentity: vi.fn(),
      onComputerRepositoryAccessChange: onAccess,
    }
    const { rerender } = render(<GitHubAccessEditor {...props} />)
    await user.click(screen.getByRole("checkbox", { name: "All repositories for dev" }))
    expect(onAccess).toHaveBeenLastCalledWith("dev", { repositoryMode: "all", allRepositoriesAllowChanges: false })
    rerender(<GitHubAccessEditor {...props} computerRepositoryAccess={{ dev: { repositoryMode: "all", allRepositoriesAllowChanges: false } }} />)
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument()
    expect(screen.queryByRole("table")).not.toBeInTheDocument()
    expect(screen.getByText("All repositories authorized on GitHub, including future additions.")).toBeVisible()
    await user.click(screen.getByRole("checkbox", { name: "Allow GitHub changes for all repositories in dev" }))
    expect(onAccess).toHaveBeenLastCalledWith("dev", { repositoryMode: "all", allRepositoriesAllowChanges: true })
    expect(props.onComputerSelectionsChange).not.toHaveBeenCalled()
    rerender(<GitHubAccessEditor {...props} computerRepositoryAccess={{ dev: { repositoryMode: "selected", allRepositoriesAllowChanges: false } }} />)
    expect(screen.getByRole("checkbox", { name: "Allow GitHub changes for acme/silo" })).toBeChecked()
  })

  it("supports app extensions and makes a disabled editor readable but immutable", () => {
    render(
      <GitHubAccessEditor
        computers={[{ name: "dev" }]}
        connectionState="connected"
        repositoryOptions={["acme/silo", "acme/design-system"]}
        computerSelections={{ dev: [{ repository: "acme/silo", allowPushes: true }] }}
        computerIdentities={{ dev: { name: "Taylor Example", email: "taylor@example.com", apply: true } }}
        currentDeviceGitIdentity={{ name: "Taylor Example", email: "taylor@example.com" }}
        onConnect={vi.fn()}
        onComputerSelectionsChange={vi.fn()}
        onComputerIdentityChange={vi.fn()}
        onResetComputerIdentity={vi.fn()}
        connectedTitle="Connected as @taylor"
        connectedDetail="Private repositories are available."
        connectedActions={<button type="button">Disconnect</button>}
        notice={<p>GitHub access is paused.</p>}
        renderComputerActions={({ name }) => <button type="button">Disable {name} access</button>}
        footer={<div role="status">Unsaved changes</div>}
        disabled
      />,
    )

    expect(screen.getByRole("heading", { name: "Connected as @taylor" })).toBeVisible()
    expect(screen.getByText("Private repositories are available.")).toBeVisible()
    expect(screen.getByText("GitHub access is paused.")).toBeVisible()
    expect(screen.getByRole("button", { name: "Disconnect" })).toBeEnabled()
    expect(screen.getByRole("button", { name: "Disable dev access" })).toBeEnabled()
    expect(screen.getByRole("status")).toHaveTextContent("Unsaved changes")

    const editor = screen.getByRole("region", { name: "Computer Git identity and repository access" })
    expect(within(editor).getByLabelText("Git name for dev")).toBeDisabled()
    expect(within(editor).getByLabelText("Git email for dev")).toBeDisabled()
    expect(within(editor).getByRole("checkbox", { name: "Apply Git identity to dev" })).toBeDisabled()
    expect(within(editor).getByRole("button", { name: "Reset Git identity for dev" })).toBeDisabled()
    expect(within(editor).getByRole("combobox", { name: "Add repository to dev" })).toBeDisabled()
    expect(within(editor).getByRole("checkbox", { name: "Allow GitHub changes for acme/silo" })).toBeDisabled()
    expect(within(editor).getByRole("button", { name: "Clear repositories from dev" })).toBeDisabled()
    expect(within(editor).getByRole("button", { name: "Remove acme/silo from dev" })).toBeDisabled()
  })

  it("collapses computer sections independently while leaving them expanded initially", async () => {
    const user = userEvent.setup()
    render(
      <GitHubAccessEditor
        computers={[{ name: "dev" }, { name: "playgrounds" }]}
        connectionState="connected"
        repositoryOptions={["acme/silo"]}
        computerSelections={{ dev: [{ repository: "acme/silo", allowPushes: true }], playgrounds: [] }}
        computerIdentities={{
          dev: { name: "Taylor Example", email: "taylor@example.com", apply: true },
          playgrounds: { name: "Taylor Example", email: "taylor@example.com", apply: true },
        }}
        currentDeviceGitIdentity={{ name: "Taylor Example", email: "taylor@example.com" }}
        onConnect={vi.fn()}
        onComputerSelectionsChange={vi.fn()}
        onComputerIdentityChange={vi.fn()}
        onResetComputerIdentity={vi.fn()}
      />,
    )

    const devDisclosure = screen.getByRole("button", { name: "Collapse dev" })
    const playgroundsDisclosure = screen.getByRole("button", { name: "Collapse playgrounds" })
    expect(devDisclosure).toHaveAttribute("aria-expanded", "true")
    expect(playgroundsDisclosure).toHaveAttribute("aria-expanded", "true")

    await user.click(devDisclosure)
    expect(devDisclosure).toHaveAttribute("aria-expanded", "false")
    expect(devDisclosure).toHaveAccessibleName("Expand dev")
    expect(screen.queryByRole("group", { name: "Git identity for dev" })).not.toBeInTheDocument()
    expect(screen.getByRole("group", { name: "Git identity for playgrounds" })).toBeVisible()

    await user.click(devDisclosure)
    expect(devDisclosure).toHaveAttribute("aria-expanded", "true")
    expect(screen.getByRole("group", { name: "Git identity for dev" })).toBeVisible()
  })
})
