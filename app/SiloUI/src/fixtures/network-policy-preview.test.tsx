import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { ApplicationPreview } from "@/fixtures/application-preview"

import { networkFixtureModeFromSearch, networkFixtureModes } from "./network-policy"

it("reads the network-policy parameter and ignores a value it does not know", () => {
  for (const mode of networkFixtureModes) expect(networkFixtureModeFromSearch(`?network-policy=${mode}`)).toBe(mode)
  expect(networkFixtureModeFromSearch("?network-policy=nonsense")).toBeUndefined()
  expect(networkFixtureModeFromSearch("")).toBeUndefined()
})

it("shows each computer's mode on the Network page and opens the computer's Network section from it", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} network="mixed" initialRoute={{ computerSection: "network" }} />)
  const modes = await screen.findByRole("region", { name: "Network access" })
  await within(modes).findAllByText(/^(Open|Allowlist|Closed) · /)
  await user.click(within(modes).getAllByRole("button", { name: /^Open the network of / })[0]!)
  expect(await screen.findByRole("region", { name: "Network" })).toBeVisible()
  expect(await screen.findByRole("region", { name: "Silo's tools" })).toBeVisible()
})

it("shows Keep free for this device in Settings, General, with the device's state", async () => {
  render(<ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" initialRoute={{ tab: "settings", settingsSection: "general" }} />)
  const section = await screen.findByRole("region", { name: "Keep free for this device" })
  expect(await within(section).findByText(/Now 6\.4 GiB \(the default\)\./)).toBeVisible()
  expect(within(section).getByRole("status")).toHaveTextContent("Starting another computer may slow this device.")
})
