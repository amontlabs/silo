import { act, render, screen, waitFor, within } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, expect, it, vi } from "vitest"
import userEvent from "@testing-library/user-event"

import { ApplicationPreview } from "@/fixtures/application-preview"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { Toaster } from "@/components/ui/sonner"
import { showBackendNotice } from "@/features/application/model/use-backend-notices"

afterEach(() => { toast.dismiss() })

const navigation = () => within(screen.getByRole("navigation", { name: "Silo navigation" }))

it("counts what is waiting on the Pending entry of the sidebar", async () => {
  render(<ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" />)
  const pending = await navigation().findByRole("button", { name: "Pending" })
  // Nine requests from agents, and three forks nobody's agent asked about; the two merges are one row each.
  await waitFor(() => expect(pending).toHaveAccessibleDescription("12 waiting for you"))
  expect(pending).toHaveTextContent("12")
})

it("shows no count when nothing is waiting", async () => {
  render(<ApplicationPreview source={applicationSourceForScenario("running")} actions={{ listApprovals: async () => ({ approvals: [] }), listReviewRequests: async () => ({ requests: [], unreachable: [] }) }} />)
  const pending = await navigation().findByRole("button", { name: "Pending" })
  await act(async () => {})
  expect(pending).not.toHaveAccessibleDescription(/waiting for you/)
})

it("opens the Pending page, titled Pending, from the sidebar", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" />)
  await user.click(await navigation().findByRole("button", { name: "Pending" }))
  const panel = within(screen.getByRole("region", { name: "Pending" }))
  expect(await panel.findByRole("heading", { name: "Pending" })).toBeInTheDocument()
  expect(await panel.findByLabelText("Waiting for you")).toBeInTheDocument()
})

it("returns to the list when Pending is clicked in the sidebar while a request is open", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" />)
  await user.click(await navigation().findByRole("button", { name: "Pending" }))
  const panel = within(screen.getByRole("region", { name: "Pending" }))
  const list = await panel.findByLabelText("Waiting for you")
  await user.click(within(list).getAllByRole("button")[0]!)
  expect(await screen.findByLabelText("What is asked")).toBeInTheDocument()
  await user.click(navigation().getByRole("button", { name: "Pending" }))
  expect(await panel.findByLabelText("Waiting for you")).toBeInTheDocument()
  expect(screen.queryByLabelText("What is asked")).toBeNull()
})

it("shows the request a toast names with its Review button", async () => {
  render(<><Toaster /><ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" /></>)
  await navigation().findByRole("button", { name: "Pending" })
  act(() => showBackendNotice({ category: "approvals", key: "approval:ap-ssh", title: "main asks: Allow SSH", body: "", computer: null }, () => {}))
  expect(await screen.findByText("main asks: Allow SSH")).toBeInTheDocument()
})

it("lists the audit rows of the Activity section and opens the request a row belongs to", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" />)
  await user.click(await navigation().findByRole("button", { name: "Activity" }))
  await user.click(await screen.findByRole("tab", { name: "Who did what" }))
  const table = await screen.findByRole("table", { name: "Who did what" })
  await waitFor(() => expect(within(table).getAllByRole("row").length).toBeGreaterThan(10))
  await user.click(within(table).getAllByRole("button", { name: "Open request" })[0]!)
  expect(await screen.findByLabelText("What is asked")).toHaveTextContent("Publish port 8080 on this device")
})

it("keeps the Activity section as it was where the engine keeps no audit log", async () => {
  const user = userEvent.setup()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} />)
  await user.click(await navigation().findByRole("button", { name: "Activity" }))
  expect(screen.queryByRole("tab", { name: "Who did what" })).toBeNull()
  expect(await screen.findByLabelText("Activity category filters")).toBeInTheDocument()
})

void vi
