import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createFixtureMacosComputersBackend, createFixtureMacosComputersStore, macosComputerFixtures } from "@/fixtures/macos-computers"
import { createComputerUseBridge } from "@/desktop/computer-use-bridge"
import { ComputerUseProvider } from "@/desktop/computer-use-provider"
import { createFixtureComputerUseBackend } from "@/fixtures/computer-use"
import { MacosComputersContext, createMacosComputersStore, type MacosComputersStore } from "@/features/macos-computers/model/macos-computers"
import { useMacosApplication } from "../model/macos-application-computers"
import type { ApplicationActions, ApplicationSource } from "../model/application-source"
import { OverviewPage } from "./overview-page"

function sourceWithoutLinuxNoise(): ApplicationSource {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  source.runtimeRepair = null
  source.computerConfigurationOperation = null
  source.activities = []
  return source
}

function Page({ source, actions }: { source: ApplicationSource; actions: ApplicationActions }) {
  const merged = useMacosApplication(source, actions)
  return <OverviewPage source={merged.source} actions={merged.actions} onConfigurationsChange={vi.fn()} />
}

async function setup(name: string, actions: Partial<ApplicationActions> = {}, store: MacosComputersStore = createFixtureMacosComputersStore()) {
  const source = sourceWithoutLinuxNoise()
  source.github.computers = [{ computer: "daily", identity: { name: "", email: "", apply: false }, repositories: [{ repository: "acme/app", allowPushes: false }] }]
  source.secrets = [{ id: "s1", name: "API_KEY", computers: ["daily"], allowedDomains: ["api.example.com"], state: "active" }]
  const user = userEvent.setup()
  const wired = { openTerminal: vi.fn(), openEditor: vi.fn(), readWorkspaceStorage: vi.fn(), refreshSshAccess: vi.fn(async () => {}), ...actions } as unknown as ApplicationActions
  render(<MacosComputersContext.Provider value={store}><Page source={source} actions={wired} /></MacosComputersContext.Provider>)
  await user.click(await screen.findByRole("button", { name: `Open ${name}` }))
  return { user, store, actions: wired }
}

const stateOf = (store: MacosComputersStore, id: string) => store.getSnapshot().state!.computers.find(computer => computer.id === id)!

it("opens a running macOS computer on the same page as a Linux one, with what its system offers", async () => {
  const { user, store, actions } = await setup("daily")
  expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("daily")
  const tabs = screen.getAllByRole("tab").map(tab => tab.textContent)
  expect(tabs).toEqual(["Overview", "Checkpoints"])
  await user.click(screen.getByRole("button", { name: /^Open daily in .*Terminal/ }))
  expect(actions.openTerminal).toHaveBeenCalledWith("daily")
  expect(screen.getByRole("button", { name: /^Open daily in .*Code/ })).toBeVisible()
  expect(screen.getByRole("button", { name: "Show screen of daily" })).toBeEnabled()
  await user.click(screen.getByRole("button", { name: "Show screen of daily" }))
  await waitFor(() => expect(stateOf(store, "mac-running").displayOpen).toBe(true))
  expect(screen.queryByRole("button", { name: "Restart daily" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "More actions for daily" }))
  expect(screen.getByRole("menuitem", { name: "Force stop daily" })).toBeVisible()
  expect(screen.queryByRole("menuitem", { name: /^Restart/ })).not.toBeInTheDocument()
  expect(screen.queryByRole("menuitem", { name: /^Fork/ })).not.toBeInTheDocument()
  expect(screen.queryByRole("menuitem", { name: /^Edit CPU/ })).not.toBeInTheDocument()
})

it("shows resources, repositories, secrets and ports on the Overview tab", async () => {
  await setup("daily")
  const resources = screen.getByText("Resources").closest("section")!
  expect(within(resources).getByText(/CPUs at start: 4/)).toBeVisible()
  expect(within(resources).getByText("Disk: 64 GiB")).toBeVisible()
  const repositories = screen.getByText("Repositories").closest("section")!
  expect(within(repositories).getByText("acme/app")).toBeVisible()
  expect(within(repositories).getByText("Access granted")).toBeVisible()
  expect(screen.getByRole("list", { name: "Secrets for daily" })).toHaveTextContent("API_KEY")
  expect(screen.getByText("Ports")).toBeVisible()
})

it("stops a running macOS computer through the shared Stop control", async () => {
  const { user, store } = await setup("daily")
  await user.click(screen.getByRole("button", { name: "Stop daily" }))
  await user.click(await screen.findByRole("button", { name: "Stop" }))
  await waitFor(() => expect(stateOf(store, "mac-running").state).toBe("stopped"))
  expect(await screen.findByRole("button", { name: "Start daily" })).toBeEnabled()
})

it("starts a stopped macOS computer, edits its CPU and memory while stopped, and lists its checkpoints", async () => {
  const { user, store } = await setup("xcode-build")
  expect(screen.getByRole("button", { name: "Show screen of xcode-build" })).toBeDisabled()
  await user.click(screen.getByRole("tab", { name: "Checkpoints" }))
  expect(screen.getByText("Xcode installed")).toBeVisible()
  expect(screen.getByText("Fresh install")).toBeVisible()
  await user.click(screen.getByRole("tab", { name: "Overview" }))
  await user.click(screen.getByRole("button", { name: "Edit" }))
  const cpus = await screen.findByLabelText("CPUs")
  await user.clear(cpus)
  await user.type(cpus, "8")
  await user.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(stateOf(store, "mac-stopped").cpus).toBe(8))
  await user.click(screen.getByRole("button", { name: "Start xcode-build" }))
  await waitFor(() => expect(stateOf(store, "mac-stopped").state).toBe("running"))
})

it("deletes a stopped macOS computer from its page and returns to the list", async () => {
  const { user, store } = await setup("xcode-build")
  await user.click(screen.getByRole("button", { name: "More actions for xcode-build" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete xcode-build" }))
  await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
  await waitFor(() => expect(store.getSnapshot().state!.computers.some(computer => computer.id === "mac-stopped")).toBe(false))
  expect(await screen.findByRole("button", { name: "Open daily" })).toBeVisible()
})

it("shows a computer that is still being set up as its progress, without tabs", async () => {
  const { user, store } = await setup("agent-box")
  expect(screen.queryByRole("tab")).not.toBeInTheDocument()
  expect(screen.getByText("Creating the account")).toBeVisible()
  expect(screen.getByRole("progressbar", { name: "agent-box progress" })).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Cancel setting up agent-box" }))
  await user.click(await screen.findByRole("button", { name: "Cancel setup" }))
  await waitFor(() => expect(store.getSnapshot().state!.computers.some(computer => computer.id === "mac-setup")).toBe(false))
})

it("offers a computer whose setup stopped a retry and a way to delete it, without tabs", async () => {
  const stopped = macosComputerFixtures.find(computer => computer.id === "mac-stopped")!
  const store = createMacosComputersStore(createFixtureMacosComputersBackend([{ ...stopped, setupComplete: false, checkpoints: [] }]))
  const { user } = await setup("xcode-build", {}, store)
  expect(screen.queryByRole("tab")).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Retry setup of xcode-build" })).toBeEnabled()
  await user.click(screen.getByRole("button", { name: "More actions for xcode-build" }))
  expect(screen.getByRole("menuitem", { name: "Delete xcode-build" })).toBeVisible()
})

it("shows the computer use of a set-up macOS computer on its Overview tab", async () => {
  const store = createFixtureMacosComputersStore()
  const source = sourceWithoutLinuxNoise()
  const user = userEvent.setup()
  const bridge = createComputerUseBridge(createFixtureComputerUseBackend("ready"))
  render(<ComputerUseProvider bridge={bridge}><MacosComputersContext.Provider value={store}><Page source={source} actions={{ openTerminal: vi.fn() } as unknown as ApplicationActions} /></MacosComputersContext.Provider></ComputerUseProvider>)
  await user.click(await screen.findByRole("button", { name: "Open daily" }))
  expect(await screen.findByRole("region", { name: "Computer use" })).toBeVisible()
})
