import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { ComputerConfigurationList } from "@/features/computers/components/computer-configuration-list"
import { ComputerEditorDraftsProvider } from "@/features/computers/model/editor-drafts"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { createFixtureMacosComputersBackend, macosComputerFixtures } from "@/fixtures/macos-computers"
import { createMacosComputersStore, MacosComputersContext, type MacosComputer } from "../model/macos-computers"

const linuxComputer = productionComputerDefaults[0]

function List() {
  return <ComputerConfigurationList includeMacosComputers editorDraftKey="computer-list" configurations={[linuxComputer]} onConfigurationsChange={vi.fn()} />
}

function App({ backend }: { backend: ReturnType<typeof createFixtureMacosComputersBackend> }) {
  const [store] = useState(() => createMacosComputersStore(backend))
  const [page, setPage] = useState<"overview" | "files">("overview")
  return <TooltipProvider><ComputerEditorDraftsProvider><MacosComputersContext.Provider value={store}>
    <button type="button" onClick={() => setPage(page === "overview" ? "files" : "overview")}>Switch page</button>
    {page === "overview" ? <List /> : <p>Files</p>}
  </MacosComputersContext.Provider></ComputerEditorDraftsProvider></TooltipProvider>
}

async function openMacosForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Add" }))
  await user.click(await screen.findByRole("menuitem", { name: "New computer" }))
  await user.selectOptions(await screen.findByRole("combobox", { name: "Operating system" }), "macOS")
  return await screen.findByTestId("macos-computer-form")
}

it("restores the operating system and macOS fields after leaving the page", async () => {
  const user = userEvent.setup()
  render(<App backend={createFixtureMacosComputersBackend([])} />)
  const form = await openMacosForm(user)
  await user.type(within(form).getByLabelText("Computer name"), "keep-me")
  await user.clear(within(form).getByLabelText("CPUs"))
  await user.type(within(form).getByLabelText("CPUs"), "6")
  await user.click(screen.getByRole("button", { name: "Switch page" }))
  expect(screen.queryByTestId("macos-computer-form")).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Switch page" }))
  const restored = await screen.findByTestId("macos-computer-form")
  expect(screen.getByRole("combobox", { name: "Operating system" })).toHaveValue("macos")
  expect(within(restored).getByLabelText("Computer name")).toHaveValue("keep-me")
  expect(within(restored).getByLabelText("CPUs")).toHaveValue(6)
})

it("locks the form and the operating system select while a macOS creation is pending, then closes", async () => {
  let finish: (value: MacosComputer) => void = () => {}
  const backend = { ...createFixtureMacosComputersBackend([]), create: vi.fn(() => new Promise<MacosComputer>(resolve => { finish = resolve })) }
  const user = userEvent.setup()
  render(<App backend={backend} />)
  const form = await openMacosForm(user)
  await user.type(within(form).getByLabelText("Computer name"), "slow")
  await user.click(within(form).getByRole("button", { name: "Create" }))
  expect(await within(form).findByRole("button", { name: "Creating…" })).toBeDisabled()
  expect(screen.getByRole("combobox", { name: "Operating system" })).toBeDisabled()
  expect(within(form).getByLabelText("Computer name")).toBeDisabled()
  finish({ ...macosComputerFixtures[0], id: "mac-slow", name: "slow" })
  await waitFor(() => expect(screen.queryByTestId("macos-computer-form")).not.toBeInTheDocument())
})

it("keeps a pending macOS creation locked across navigation and closes when it finishes", async () => {
  let finish: (value: MacosComputer) => void = () => {}
  const backend = { ...createFixtureMacosComputersBackend([]), create: vi.fn(() => new Promise<MacosComputer>(resolve => { finish = resolve })) }
  const user = userEvent.setup()
  render(<App backend={backend} />)
  const form = await openMacosForm(user)
  await user.type(within(form).getByLabelText("Computer name"), "slow")
  await user.click(within(form).getByRole("button", { name: "Create" }))
  expect(await within(form).findByRole("button", { name: "Creating…" })).toBeDisabled()
  await user.click(screen.getByRole("button", { name: "Switch page" }))
  await user.click(screen.getByRole("button", { name: "Switch page" }))
  const restored = await screen.findByTestId("macos-computer-form")
  expect(within(restored).getByRole("button", { name: "Creating…" })).toBeDisabled()
  expect(within(restored).getByLabelText("Computer name")).toBeDisabled()
  expect(screen.getByRole("combobox", { name: "Operating system" })).toBeDisabled()
  expect(backend.create).toHaveBeenCalledTimes(1)
  // Adding is locked while the creation is pending, so no second editor can take over its draft.
  expect(screen.getByRole("button", { name: "Add" })).toBeDisabled()
  finish({ ...macosComputerFixtures[0], id: "mac-slow", name: "slow" })
  await waitFor(() => expect(screen.queryByTestId("macos-computer-form")).not.toBeInTheDocument())
  expect(screen.getByRole("button", { name: "Add" })).toBeEnabled()
  // A new editor starts fresh: Linux, not locked, nothing carried over from the finished creation.
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(await screen.findByRole("menuitem", { name: "New computer" }))
  expect(await screen.findByRole("combobox", { name: "Operating system" })).toHaveValue("linux")
  await user.selectOptions(screen.getByRole("combobox", { name: "Operating system" }), "macOS")
  const next = await screen.findByTestId("macos-computer-form")
  expect(within(next).getByLabelText("Computer name")).toHaveValue("")
  expect(within(next).getByLabelText("Computer name")).toBeEnabled()
})

it("moves focus to the operating system select when it swaps the fields", async () => {
  const user = userEvent.setup()
  render(<App backend={createFixtureMacosComputersBackend([])} />)
  await openMacosForm(user)
  expect(screen.getByRole("combobox", { name: "Operating system" })).toHaveFocus()
  await user.selectOptions(screen.getByRole("combobox", { name: "Operating system" }), "Linux")
  expect(screen.getByRole("combobox", { name: "Operating system" })).toHaveFocus()
})

it("keeps names unique across both kinds", async () => {
  const user = userEvent.setup()
  render(<App backend={createFixtureMacosComputersBackend(macosComputerFixtures)} />)
  await waitFor(() => expect(document.querySelector("[data-macos-computer-id]")).not.toBeNull())
  const form = await openMacosForm(user)
  await user.type(within(form).getByLabelText("Computer name"), linuxComputer.name)
  expect(within(form).getByText("Computer names must be unique.")).toBeVisible()
  expect(within(form).getByRole("button", { name: "Create" })).toBeDisabled()
  await user.selectOptions(screen.getByRole("combobox", { name: "Operating system" }), "Linux")
  const name = await screen.findByLabelText("Computer name")
  await user.clear(name)
  await user.type(name, "daily")
  await user.click(screen.getByRole("button", { name: "Create" }))
  expect(await screen.findByText("Computer names must be unique.")).toBeVisible()
})
