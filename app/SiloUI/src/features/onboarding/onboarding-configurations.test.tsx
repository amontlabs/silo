import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { OnboardingPreview } from "@/fixtures/onboarding-preview"
import * as computerConfiguration from "./model/computer-configuration"

import type { GitHubConnectionState } from "@/features/onboarding/model/onboarding-source"

import { onboardingScenarios, repositoryFixtures } from "@/fixtures/scenarios"

function renderScenario(name: keyof typeof onboardingScenarios = "running", githubState?: GitHubConnectionState) {
  return render(<OnboardingPreview source={onboardingScenarios[name]} initialGitHubConnectionState={githubState} repositoryOptions={repositoryFixtures} actions={{
    saveComputerConfiguration: vi.fn(),
    retryComputerSetup: vi.fn(),
    finishSetup: vi.fn(),
  }} />)
}

function computerEditor() {
  return within(screen.getByTestId(/^computer-editor-/))
}

function computerPanel() {
  return within(screen.getByRole("tabpanel"))
}

function configuredComputers() {
  return within(screen.getByRole("list", { name: "Configured computers" }))
}

async function renderComputerScenario() {
  const saveComputerConfiguration = vi.fn()
  const user = userEvent.setup()
  render(<OnboardingPreview
    source={onboardingScenarios.running}
    repositoryOptions={repositoryFixtures}
    actions={{
      saveComputerConfiguration,
      retryComputerSetup: vi.fn(),
      finishSetup: vi.fn(),
    }}
  />)
  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  return { user, saveComputerConfiguration }
}


it("starts from the exact three production configuration defaults instead of the activity stress fixture", async () => {
  await renderComputerScenario()
  const list = screen.getByRole("list", { name: "Configured computers" })
  const rows = within(list).getAllByRole("listitem")

  expect(rows).toHaveLength(3)
  expect(rows.map((row) => within(row).getByText(/^(dev|playgrounds|personal)$/).textContent)).toEqual(["dev", "playgrounds", "personal"])
  expect(rows[0]).toHaveTextContent("CPUs: 8 · Memory: 32 GiB · Disk: 120 GiB")
  expect(rows[1]).toHaveTextContent("CPUs: 4 · Memory: 32 GiB · Disk: 60 GiB")
  expect(rows[2]).toHaveTextContent("CPUs: 6 · Memory: 16 GiB · Disk: 100 GiB")
  expect(within(list).queryByText("docs-build")).not.toBeInTheDocument()
})


it("adds, cancels, and saves a virtual configuration through the typed configuration action", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()

  await user.click(computerPanel().getByRole("button", { name: "Add" }))
  await user.click(within(screen.getByRole("menu", { name: "Add computer" })).getByRole("menuitem", { name: "New computer" }))
  const draftName = computerEditor().getByRole("textbox", { name: "Computer name" })
  expect(draftName).toHaveValue("computer-4")
  expect(draftName).toHaveFocus()
  expect(computerEditor().getByRole("combobox", { name: "CPUs at start" })).toHaveValue("8")
  await user.click(computerEditor().getByRole("button", { name: "Cancel" }))
  expect(screen.queryByDisplayValue("computer-4")).not.toBeInTheDocument()
  expect(saveComputerConfiguration).not.toHaveBeenCalled()

  await user.click(computerPanel().getByRole("button", { name: "Add" }))
  await user.click(within(screen.getByRole("menu", { name: "Add computer" })).getByRole("menuitem", { name: "New computer" }))
  await user.clear(computerEditor().getByRole("textbox", { name: "Computer name" }))
  await user.type(computerEditor().getByRole("textbox", { name: "Computer name" }), "build")
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "CPUs at start" }), "4")
  await user.click(computerEditor().getByRole("button", { name: "Create" }))

  expect(saveComputerConfiguration).toHaveBeenCalledOnce()
  expect(saveComputerConfiguration.mock.lastCall?.[0]).toMatchObject({
    schemaVersion: 1,
    computers: [
      { name: "dev" },
      { name: "playgrounds" },
      { name: "personal" },
      { name: "build", cpus: 4, maxCPUs: 12, memoryGiB: 32, maxMemoryGiB: 48, workspaceStorageGiB: 120, runtimeStorageGiB: 100 },
    ],
  })
  expect(screen.getByRole("list", { name: "Configured computers" })).toHaveTextContent("build")
})

it("restores an existing computer exactly on Cancel and persists a valid edit on Save", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev" }))
  const name = computerEditor().getByRole("textbox", { name: "Computer name" })
  expect(name).toHaveFocus()
  expect(name).toHaveAttribute("readonly")
  expect(computerEditor().getByRole("combobox", { name: "Workspace disk" })).toBeDisabled()
  expect(computerEditor().getByRole("combobox", { name: "Runtime disk" })).toBeDisabled()
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "Memory at start" }), "16")
  await user.click(computerEditor().getByRole("button", { name: "Cancel" }))
  expect(saveComputerConfiguration).not.toHaveBeenCalled()
  expect(configuredComputers().getByRole("button", { name: "More actions for dev" })).toBeVisible()

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev" }))
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "Memory at start" }), "16")
  await user.click(computerEditor().getByRole("button", { name: "Save" }))
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers[0]).toMatchObject({ name: "dev", memoryGiB: 16 })
  expect(configuredComputers().getByRole("button", { name: "More actions for dev" })).toBeVisible()
})


it("offers Edit, Duplicate settings and Delete in the actions menu and keeps the drag handle tooltip-free", async () => {
  const { user } = await renderComputerScenario()
  const dragHandle = configuredComputers().getByRole("button", { name: "Reorder dev" })
  expect(dragHandle).toHaveAccessibleName("Reorder dev")
  expect(dragHandle).not.toHaveAttribute("title")
  await user.click(dragHandle)
  await user.keyboard("{ArrowDown}")
  fireEvent.blur(dragHandle)
  await waitFor(() => expect(screen.queryByRole("tooltip")).not.toBeInTheDocument())

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  for (const name of ["Edit dev", "Duplicate settings for dev", "Delete dev"]) {
    expect(screen.getByRole("menuitem", { name })).toBeVisible()
  }
  await user.keyboard("{Escape}")

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete dev" }))
  expect(screen.getByText("Delete dev permanently?")).toBeVisible()
})


it("preserves GitHub policy and identity settings when computer resources change", async () => {
  const user = userEvent.setup()
  renderScenario("running", "connected")
  await user.click(screen.getByRole("tab", { name: /GitHub/ }))
  await user.clear(screen.getByLabelText("Git name for dev"))
  await user.type(screen.getByLabelText("Git name for dev"), "Renamed Author")
  expect(within(screen.getByRole("table", { name: "Selected repositories for dev" })).getByText("acme/silo")).toBeVisible()

  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev" }))
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "CPUs at start" }), "4")
  await user.click(computerEditor().getByRole("button", { name: "Save" }))

  await user.click(screen.getByRole("tab", { name: /GitHub/ }))
  expect(screen.getByLabelText("Git name for dev")).toHaveValue("Renamed Author")
  expect(within(screen.getByRole("table", { name: "Selected repositories for dev" })).getByText("acme/silo")).toBeVisible()
})


it("duplicates after the source, cancels drafts, and generates collision-free copy names", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for dev" }))
  expect(computerEditor().getByRole("textbox", { name: "Computer name" })).toHaveValue("dev-copy")
  await user.click(computerEditor().getByRole("button", { name: "Cancel" }))
  expect(saveComputerConfiguration).not.toHaveBeenCalled()

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for dev" }))
  await user.click(computerEditor().getByRole("button", { name: "Create" }))
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers.map(({ name }: { name: string }) => name)).toEqual(["dev", "dev-copy", "playgrounds", "personal"])

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for dev" }))
  expect(computerEditor().getByRole("textbox", { name: "Computer name" })).toHaveValue("dev-copy-2")
  await user.click(computerEditor().getByRole("button", { name: "Create" }))
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers.map(({ name }: { name: string }) => name)).toEqual(["dev", "dev-copy-2", "dev-copy", "playgrounds", "personal"])
})


it("places a replacement duplicate after its source when another draft is open", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for dev" }))
  await user.click(configuredComputers().getByRole("button", { name: "More actions for playgrounds" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for playgrounds" }))
  expect(computerEditor().getByRole("textbox", { name: "Computer name" })).toHaveValue("playgrounds-copy")
  await user.click(computerEditor().getByRole("button", { name: "Create" }))

  expect(saveComputerConfiguration.mock.lastCall?.[0].computers.map(({ name }: { name: string }) => name)).toEqual([
    "dev", "playgrounds", "playgrounds-copy", "personal",
  ])
})


it("confirms deletion in a popover; Cancel or Escape keeps the computer", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete dev" }))
  expect(screen.getByText("Delete dev permanently?")).toBeVisible()
  expect(screen.getByText("Its files and checkpoints will be deleted. This can't be undone.")).toBeVisible()
  expect(saveComputerConfiguration).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  expect(screen.queryByText("Delete dev permanently?")).not.toBeInTheDocument()
  expect(saveComputerConfiguration).not.toHaveBeenCalled()
  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete dev" }))
  await user.keyboard("{Escape}")
  expect(screen.queryByText("Delete dev permanently?")).not.toBeInTheDocument()
  expect(configuredComputers().getByRole("button", { name: "More actions for dev" })).toBeVisible()
  expect(saveComputerConfiguration).not.toHaveBeenCalled()

  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete dev" }))
  await user.click(screen.getByRole("button", { name: /^Delete permanently$/ }))
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers.map(({ name }: { name: string }) => name)).toEqual(["playgrounds", "personal"])
  expect(screen.queryByRole("button", { name: "More actions for dev" })).not.toBeInTheDocument()
})


it("persists pointer drag reorder and the quiet keyboard reorder path", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()
  const data = new Map<string, string>()
  const dataTransfer = {
    effectAllowed: "none",
    setData: (type: string, value: string) => data.set(type, value),
    getData: (type: string) => data.get(type) ?? "",
  }
  const target = configuredComputers().getByRole("button", { name: "More actions for personal" }).closest("li")
  expect(target).not.toBeNull()

  fireEvent.dragStart(configuredComputers().getByRole("button", { name: "Reorder dev" }), { dataTransfer })
  fireEvent.dragOver(target!, { dataTransfer })
  fireEvent.drop(target!, { dataTransfer })
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers.map(({ name }: { name: string }) => name)).toEqual(["playgrounds", "personal", "dev"])

  const devHandle = configuredComputers().getByRole("button", { name: "Reorder dev" })
  act(() => devHandle.focus())
  await user.keyboard("{ArrowUp}")
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers.map(({ name }: { name: string }) => name)).toEqual(["playgrounds", "dev", "personal"])
  expect(screen.getByText("dev moved to position 2 of 3.")).toBeInTheDocument()
})


it("saves smaller memory presets and custom whole GiB values", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()
  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for dev" }))
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "Memory at start" }), "12")
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "Maximum memory" }), "12")
  await user.click(computerEditor().getByRole("button", { name: "Create" }))
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers).toEqual(expect.arrayContaining([
    expect.objectContaining({ name: "dev-copy", memoryGiB: 12, maxMemoryGiB: 12 }),
  ]))
  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev-copy" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev-copy" }))
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "Memory at start" }), "custom")
  const input = computerEditor().getByRole("spinbutton", { name: "Memory at start custom (GiB)" })
  await user.clear(input)
  await user.type(input, "10")
  await user.click(computerEditor().getByRole("button", { name: "Save" }))
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers).toEqual(expect.arrayContaining([
    expect.objectContaining({ name: "dev-copy", memoryGiB: 10, maxMemoryGiB: 12 }),
  ]))
  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev-copy" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev-copy" }))
  expect(computerEditor().getByRole("spinbutton", { name: "Memory at start custom (GiB)" })).toHaveValue(10)
  saveComputerConfiguration.mockClear()
  const customInput = computerEditor().getByRole("spinbutton", { name: "Memory at start custom (GiB)" })
  for (const invalid of ["0", "1.5", "13"]) {
    await user.clear(customInput)
    await user.type(customInput, invalid)
    await user.click(computerEditor().getByRole("button", { name: "Save" }))
    expect(saveComputerConfiguration).not.toHaveBeenCalled()
  }
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "Memory at start" }), "8")
  await user.click(computerEditor().getByRole("button", { name: "Save" }))
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers).toEqual(expect.arrayContaining([
    expect.objectContaining({ name: "dev-copy", memoryGiB: 8, maxMemoryGiB: 12 }),
  ]))
})


it("saves custom CPU and disk values and reopens them", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()
  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for dev" }))
  for (const [label, unit, value] of [["CPUs at start", "CPUs", "3"], ["Maximum CPUs", "CPUs", "5"], ["Workspace disk", "GiB", "35"], ["Runtime disk", "GiB", "25"]]) {
    await user.selectOptions(computerEditor().getByRole("combobox", { name: label }), "custom")
    const input = computerEditor().getByRole("spinbutton", { name: `${label} custom (${unit})` })
    await user.clear(input)
    await user.type(input, value)
  }
  await user.click(computerEditor().getByRole("button", { name: "Create" }))
  expect(saveComputerConfiguration.mock.lastCall?.[0].computers).toEqual(expect.arrayContaining([
    expect.objectContaining({ name: "dev-copy", cpus: 3, maxCPUs: 5, workspaceStorageGiB: 35, runtimeStorageGiB: 25 }),
  ]))
  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev-copy" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev-copy" }))
  expect(computerEditor().getByRole("spinbutton", { name: "CPUs at start custom (CPUs)" })).toHaveValue(3)
  expect(computerEditor().getByRole("spinbutton", { name: "Workspace disk custom (GiB)" })).toHaveValue(35)
})


it("blocks duplicate names and invalid computer resource ranges", async () => {
  const { user, saveComputerConfiguration } = await renderComputerScenario()
  await user.click(configuredComputers().getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Duplicate settings for dev" }))
  await user.clear(computerEditor().getByRole("textbox", { name: "Computer name" }))
  await user.type(computerEditor().getByRole("textbox", { name: "Computer name" }), "personal")
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "CPUs at start" }), "12")
  await user.selectOptions(computerEditor().getByRole("combobox", { name: "Maximum CPUs" }), "4")
  await user.click(computerEditor().getByRole("button", { name: "Create" }))

  expect(screen.getByText("Computer names must be unique.")).toBeVisible()
  expect(screen.getByText("CPUs at start cannot exceed the maximum.")).toBeVisible()
  expect(saveComputerConfiguration).not.toHaveBeenCalled()
})


it("reports the configuration capacity in the draft instead of throwing across the action boundary", async () => {
  // Model tests cover the real 64-slot policy. Here the validator reports a
  // full device while the UI renders only the ordinary three-card fixture.
  vi.spyOn(computerConfiguration, "computerCapacityError").mockReturnValue("Configure no more than 64 computers.")
  const saveComputerConfiguration = vi.fn()
  const user = userEvent.setup()
  render(<OnboardingPreview source={onboardingScenarios.running} actions={{
    saveComputerConfiguration,
    retryComputerSetup: vi.fn(),
    finishSetup: vi.fn(),
  }} />)
  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  await user.click(computerPanel().getByRole("button", { name: "Add" }))
  await user.click(within(screen.getByRole("menu", { name: "Add computer" })).getByRole("menuitem", { name: "New computer" }))
  await user.click(computerEditor().getByRole("button", { name: "Create" }))

  expect(computerEditor().getByRole("alert")).toHaveTextContent("Configure no more than 64 computers.")
  expect(computerConfiguration.computerCapacityError).toHaveBeenCalledExactlyOnceWith(3, undefined)
  expect(saveComputerConfiguration).not.toHaveBeenCalled()
})

it("mirrors final configuration order in Review while preserving activity collapse", async () => {
  const { user } = await renderComputerScenario()
  await user.click(computerPanel().getByRole("button", { name: "Add" }))
  await user.click(within(screen.getByRole("menu", { name: "Add computer" })).getByRole("menuitem", { name: "New computer" }))
  await user.clear(computerEditor().getByRole("textbox", { name: "Computer name" }))
  await user.type(computerEditor().getByRole("textbox", { name: "Computer name" }), "remote")
  await user.click(computerEditor().getByRole("button", { name: "Create" }))
  await user.click(configuredComputers().getByRole("button", { name: "Reorder remote" }))
  await user.keyboard("{ArrowUp}{ArrowUp}{ArrowUp}")

  await user.click(screen.getByRole("button", { name: "Expand activity" }))
  expect(screen.getByLabelText("Computer activity")).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Collapse activity" }))
  expect(screen.queryByLabelText("Computer activity")).not.toBeInTheDocument()
  expect(screen.getByTestId("computer-configuration-list")).toBeVisible()

  await user.click(screen.getByRole("tab", { name: /Review/ }))
  const review = screen.getByRole("list", { name: "Computers" })
  const rows = within(review).getAllByRole("listitem")
  const expected = [
    ["remote", "CPUs: 8 · Memory: 32 GiB"],
    ["dev", "CPUs: 8 · Memory: 32 GiB"],
    ["playgrounds", "CPUs: 4 · Memory: 32 GiB"],
    ["personal", "CPUs: 6 · Memory: 16 GiB"],
  ]
  expect(rows).toHaveLength(expected.length)
  expected.forEach(([name, detail], index) => {
    expect(within(rows[index]).getByText(name)).toBeVisible()
    expect(rows[index]).toHaveTextContent(detail)
  })
  await user.click(screen.getByRole("tab", { name: /Computers/ }))
  expect(screen.getByRole("button", { name: "Expand activity" })).toHaveAttribute("aria-expanded", "false")
  expect(within(screen.getByRole("tabpanel")).queryByLabelText("Computer activity")).not.toBeInTheDocument()
})
