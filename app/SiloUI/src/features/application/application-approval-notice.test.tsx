import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, expect, it, vi } from "vitest"

const notices = vi.hoisted(() => ({ handler: undefined as undefined | ((notice: unknown) => void) }))
vi.mock("@/desktop/notices", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/desktop/notices")>()),
  deliverNotice: vi.fn(),
  listenForNotices: (handler: (notice: unknown) => void) => { notices.handler = handler; return () => { notices.handler = undefined } },
}))

import { Toaster } from "@/components/ui/sonner"
import { ApplicationPreview } from "@/fixtures/application-preview"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"

afterEach(() => { toast.dismiss() })

const navigation = () => within(screen.getByRole("navigation", { name: "Silo navigation" }))

it("shows a toast for a request from an agent, and Review opens that request on the Pending page", async () => {
  const user = userEvent.setup()
  render(<><Toaster /><ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" /></>)
  await navigation().findByRole("button", { name: "Pending" })
  act(() => notices.handler?.({ category: "approvals", key: "approval:ap-ssh", title: "main asks: Allow SSH into main", body: "", computer: null }))
  expect(await screen.findByText("main asks: Allow SSH into main")).toBeInTheDocument()
  await user.click(screen.getAllByRole("button", { name: "Review" })[0]!)
  expect(await screen.findByLabelText("What is asked")).toHaveTextContent("Allow SSH into main from outside this device on port 2222")
  expect(navigation().getByRole("button", { name: "Pending" })).toHaveAttribute("aria-current", "page")
})

it("drops the toast of a request once it is answered", async () => {
  const user = userEvent.setup()
  render(<><Toaster /><ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" /></>)
  await user.click(await navigation().findByRole("button", { name: "Pending" }))
  act(() => notices.handler?.({ category: "approvals", key: "approval:ap-port", title: "main asks: Publish port 8080", body: "", computer: null }))
  expect(await screen.findAllByText("main asks: Publish port 8080")).toHaveLength(1)
  await user.click((await screen.findAllByRole("button", { name: "dev" }))[2]!)
  await user.click(await screen.findByRole("button", { name: "Allow" }))
  await waitFor(() => expect(screen.queryByText("main asks: Publish port 8080")).toBeNull(), { timeout: 3000 })
})

it("does not open an approval detail for an approval notice with an empty id", async () => {
  const user = userEvent.setup()
  render(<><Toaster /><ApplicationPreview source={applicationSourceForScenario("running")} approvals="waiting" /></>)
  await navigation().findByRole("button", { name: "Pending" })
  act(() => notices.handler?.({ category: "approvals", key: "approval:", title: "Empty approval", body: "", computer: null }))
  expect(await screen.findByText("Empty approval")).toBeInTheDocument()
  await user.click(screen.getAllByRole("button", { name: "Review" })[0]!)
  expect(navigation().getByRole("button", { name: "Pending" })).toHaveAttribute("aria-current", "page")
  expect(screen.queryByLabelText("What is asked")).not.toBeInTheDocument()
})
