import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createFixtureMacosComputersStore } from "@/fixtures/macos-computers"
import { MacosComputersContext } from "@/features/macos-computers/model/macos-computers"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { withMacosComputers } from "../model/macos-application-computers"
import type { ApplicationActions, ApplicationSource } from "../model/application-source"
import { createDirectoryStore } from "../model/directory-store"
import { ComputersPage } from "./computers-page"
import { GitHubPage } from "./github-page"
import { SecretsPage } from "./secrets-page"

async function mergedSource(): Promise<{ source: ApplicationSource; store: ReturnType<typeof createFixtureMacosComputersStore> }> {
  const store = createFixtureMacosComputersStore()
  await store.refresh()
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  source.github = { ...source.github, state: "connected", account: "taylor", accessEnabled: true }
  return { source: withMacosComputers(source, store.getSnapshot()), store }
}

function renderComputersPage(source: ApplicationSource, store: ReturnType<typeof createFixtureMacosComputersStore>, section: "files" | "network" | "logs", actions: Partial<ApplicationActions> = {}, directoryStore = createDirectoryStore()) {
  return render(
    <SettingsProvider initialSettings={{ theme: "light" }}>
      <MacosComputersContext.Provider value={store}>
        <ComputersPage
          source={source} section={section} computers={source.computers} activities={[]} selectedComputerIds={new Set()}
          network={source.network} networkActions={actions as ApplicationActions} onSectionChange={vi.fn()} editor="Editor" onOpenEditor={vi.fn()}
          directoryStore={directoryStore} active logQuery="" repositoryPushOperations={[]} browser="Browser"
          onComputerFilterChange={vi.fn()} onLogQueryChange={vi.fn()} onPushRepository={vi.fn()} onDismissRepositoryPush={vi.fn()}
        />
      </MacosComputersContext.Provider>
    </SettingsProvider>,
  )
}

it("browses a macOS computer's files from the workspace of its working account", async () => {
  const { source, store } = await mergedSource()
  const listComputerDirectory = vi.fn(async () => ({ snapshotId: "s1", entries: [{ name: "notes.txt", path: "/Users/silo/workspace/notes.txt", kind: "file" as const }], nextOffset: null }))
  const directoryStore = createDirectoryStore(listComputerDirectory)
  renderComputersPage(source, store, "files", { listComputerDirectory }, directoryStore)
  const tree = await screen.findByLabelText("Files in daily")
  expect(await within(tree).findByText("notes.txt")).toBeVisible()
  expect(listComputerDirectory).toHaveBeenCalledWith("daily", "/Users/silo/workspace", 0, undefined)
})

it("lists a macOS computer's published ports on the Network page and publishes one by its name", async () => {
  const { source, store } = await mergedSource()
  const saveNetworkPort = vi.fn(async () => {})
  const network = { computers: [{ computer: "daily", error: null, ports: [{ port: 3000, hostPort: 43000, scheme: "http" as const, state: "reachable" as const, configured: true }] }] }
  renderComputersPage({ ...source, network }, store, "network", { saveNetworkPort, refreshNetwork: vi.fn(async () => {}) })
  expect(await screen.findByText("http://127.0.0.1:43000")).toBeVisible()
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Add port" }))
  expect(screen.getByRole("combobox", { name: "Computer" })).toHaveTextContent(/daily/)
})

it("keeps another device's macOS computers off the Files and Network pages", async () => {
  const { source, store } = await mergedSource()
  const hosted = source.computers.find(computer => computer.macos)!
  const remote = { ...hosted, configuration: { ...hosted.configuration, id: "silo-remote:studio:mac-9", name: "remote-mac" }, device: { id: "studio", computerId: "mac-9", name: "Studio", address: "studio.local", connected: true } }
  const withRemote = { ...source, computers: [...source.computers, remote] }
  const files = renderComputersPage(withRemote, store, "files")
  await userEvent.setup().click(screen.getByRole("combobox", { name: "Filter computers" }))
  expect(screen.getByRole("option", { name: "daily" })).toBeVisible()
  expect(screen.queryByRole("option", { name: "remote-mac" })).not.toBeInTheDocument()
  files.unmount()
  renderComputersPage(withRemote, store, "logs")
  await userEvent.setup().click(screen.getByRole("combobox", { name: "Filter computers" }))
  expect(screen.getByRole("option", { name: "remote-mac" })).toBeVisible()
})

it("offers a macOS computer to a secret", async () => {
  const { source } = await mergedSource()
  const user = userEvent.setup()
  render(<SecretsPage source={source} onSaveSecret={vi.fn()} onRemoveSecret={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: "Add secret" }))
  const form = screen.getByRole("form", { name: "Add secret" })
  await user.click(within(form).getByRole("combobox", { name: "Add computer" }))
  expect(screen.getByRole("option", { name: "daily" })).toBeVisible()
  expect(screen.getByRole("option", { name: "xcode-build" })).toBeVisible()
})

it("lets a macOS computer be granted repositories", async () => {
  const { source } = await mergedSource()
  source.github.repositoryCatalog = ["acme/app"]
  render(<GitHubPage source={source} actions={{} as ApplicationActions} />)
  expect(await screen.findByRole("combobox", { name: "Add repository to daily" })).toBeEnabled()
  expect(screen.getByRole("combobox", { name: "Add repository to xcode-build" })).toBeEnabled()
})
