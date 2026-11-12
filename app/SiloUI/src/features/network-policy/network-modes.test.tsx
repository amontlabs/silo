import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { NetworkPage } from "@/features/application/pages/network-page"
import type { ApplicationActions, ApplicationComputer } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createNetworkFixture } from "@/fixtures/network-policy"

const source = applicationSourceForScenario("complete")
const local = source.computers.filter(computer => !computer.device).slice(0, 3)

function setup(computers: ApplicationComputer[] = local, read?: ApplicationActions["readNetworkPolicy"]) {
  const fixture = createNetworkFixture({ mode: "mixed" })
  fixture.setComputers(computers)
  const onOpenComputer = vi.fn()
  const actions = { refreshNetwork: vi.fn(async () => {}), readNetworkPolicy: read ?? fixture.readNetworkPolicy } as unknown as ApplicationActions
  render(<NetworkPage computers={computers} browser="Safari" actions={actions} active onOpenComputer={onOpenComputer} />)
  return { onOpenComputer, user: userEvent.setup() }
}

it("shows each computer's network mode on the Network page, with a link to its Network section", async () => {
  expect(local.length).toBeGreaterThanOrEqual(2)
  const { onOpenComputer, user } = setup()
  const section = await screen.findByRole("region", { name: "Network access" })
  expect(await within(section).findByText("Open · Can reach any public address.")).toBeVisible()
  expect(await within(section).findByText("Allowlist · Only the hosts you list.")).toBeVisible()
  await user.click(within(section).getByRole("button", { name: `Open the network of ${local[1]!.configuration.name}` }))
  expect(onOpenComputer).toHaveBeenCalledExactlyOnceWith(local[1]!.configuration.id)
})

it("lists no mode for a computer on another device", () => {
  const remote = { ...local[0]!, device: { id: "office", name: "Office Mac", address: "office.local", connected: true, computerId: "x" } } as ApplicationComputer
  setup([remote])
  expect(screen.queryByRole("region", { name: "Network access" })).not.toBeInTheDocument()
})

it("says so when a computer's policy cannot be read", async () => {
  setup([local[0]!], vi.fn().mockRejectedValue(new Error("no")))
  expect(await screen.findByText("Could not read the network policy.")).toBeVisible()
})
