import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { ComputerConfigurationList } from "@/features/computers/components/computer-configuration-list"
import { OperatingSystemField, OsBadge } from "@/features/computers/components/os-badge"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"

const vmComputer = productionComputerDefaults[0]
const containerComputer = { ...vmComputer, id: "6f1d4c1e-2f5e-4a39-9d57-0a3f0b8a5c11", name: "box", runtime: "container" as const }

const linuxHost = { id: "linux-host", name: "Build box", connected: true, address: "build.local", platform: "linux" as const }
const macHost = { id: "mac-host", name: "Studio", connected: true, address: "studio.local", platform: "macos" as const }

function List({ configurations = [vmComputer], devices = [linuxHost, macHost] }: { configurations?: (typeof vmComputer)[]; devices?: (typeof linuxHost | typeof macHost)[] }) {
  return <TooltipProvider><ComputerConfigurationList devices={devices} configurations={configurations} onConfigurationsChange={vi.fn()} /></TooltipProvider>
}

async function startNewComputer(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Add" }))
  await user.click(await screen.findByRole("menuitem", { name: "New computer" }))
  return await screen.findByRole("combobox", { name: "Operating system" })
}

it("labels a container computer and warns that it is less isolated", () => {
  render(<OsBadge os="container" />)
  const badge = screen.getByText("Container")
  expect(badge).toHaveAttribute("title", "Less isolated than a VM")
  expect(badge).toHaveAccessibleName("Container. Less isolated than a VM")
})

it("shows the container label on a container computer's row and not on a VM's", () => {
  render(<List configurations={[vmComputer, containerComputer]} />)
  const badges = document.querySelectorAll("[data-slot='os-badge']")
  expect([...badges].map(badge => badge.getAttribute("data-os"))).toEqual(["linux", "container"])
})

it("does not offer a container computer without a Linux host", () => {
  render(<OperatingSystemField value="linux" macosSupported={false} onChange={vi.fn()} />)
  expect(screen.queryByRole("option", { name: /Container/ })).not.toBeInTheDocument()
})

it("offers a container computer last, and only for a Linux host", async () => {
  const user = userEvent.setup()
  render(<List />)
  const field = await startNewComputer(user)
  expect(within(field).queryByRole("option", { name: /Container/ })).not.toBeInTheDocument()
  expect(field).toHaveValue("linux")
  await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "Studio")
  expect(within(screen.getByRole("combobox", { name: "Operating system" })).queryByRole("option", { name: /Container/ })).not.toBeInTheDocument()
  await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "Build box")
  const options = within(screen.getByRole("combobox", { name: "Operating system" })).getAllByRole("option")
  expect(options.at(-1)).toHaveTextContent("Container (less isolated than a vm)")
  expect(screen.getByRole("combobox", { name: "Operating system" })).toHaveValue("linux")
})

it("drops the container choice when the computer moves to a Mac", async () => {
  const user = userEvent.setup()
  render(<List />)
  await startNewComputer(user)
  await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "Build box")
  await user.selectOptions(screen.getByRole("combobox", { name: "Operating system" }), "container")
  expect(screen.getByRole("combobox", { name: "Operating system" })).toHaveValue("container")
  await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "Studio")
  expect(screen.getByRole("combobox", { name: "Operating system" })).toHaveValue("linux")
})
