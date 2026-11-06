import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { createReviewRequestFixtureActions, reviewRequestFixtures } from "@/fixtures/review-requests"
import { ReviewRequest, staleReason, type ReviewRequestActions } from "./review-request"

async function keepConfirmed() {
  await userEvent.click(await screen.findByRole("button", { name: "Keep" }))
  const popover = document.querySelector<HTMLElement>("[data-slot=popover-content]")!
  expect(popover).toHaveTextContent("Keep into main?")
  await userEvent.click(within(popover).getByRole("button", { name: "Keep" }))
}

function setup(requestId: string, overrides: Partial<ReviewRequestActions> = {}, onBack?: () => void) {
  const actions = createReviewRequestFixtureActions(overrides)
  render(<ReviewRequest requestId={requestId} deviceId="dev-2" actions={actions} onBack={onBack} />)
  return actions
}

const refusal = (reason: string, message: string) => vi.fn().mockImplementation(async () => ({
  status: "refused",
  report: { fork: "login-fix", target: "main", summary: [], steps: [], notes: [] },
  refusal: { reason, message, hint: "", conflicts: [] },
  request: reviewRequestFixtures["req-fresh"].request,
}))

it("shows loading, then the header, buckets, diff text and credentials", async () => {
  setup("req-fresh")
  expect(screen.getByRole("status")).toHaveTextContent("Loading request")
  expect(await screen.findByText("login-fix → main")).toBeInTheDocument()
  expect(screen.getByLabelText("Changes by bucket")).toHaveTextContent("Code 1")
  expect(screen.getByLabelText("Diff")).toHaveTextContent("apt: ripgrep, jq (+3 dependencies)")
  expect(screen.getByLabelText("Credentials used")).toHaveTextContent("GITHUB_TOKEN → api.github.com (GET, 4 times)")
  expect(screen.getByLabelText("Credentials used")).toHaveTextContent("files on disk are copied.")
  expect(screen.getByRole("button", { name: "Keep" })).toBeEnabled()
})

it("names the target and status in the header when the list row knows them", async () => {
  render(<ReviewRequest requestId="req-fresh" deviceId="dev-2" deviceName="Mac mini" targetName="Main computer" actions={createReviewRequestFixtureActions()} />)
  expect(await screen.findByText("login-fix → Main computer")).toBeInTheDocument()
  expect(screen.getByText(/on Mac mini · created .* · Open/)).toBeInTheDocument()
})

it("shows an error with a retry, and none for credentials", async () => {
  const show = vi.fn().mockRejectedValueOnce(new Error("device offline")).mockImplementation(createReviewRequestFixtureActions().show)
  setup("req-fresh", { show })
  expect(await screen.findByRole("alert")).toHaveTextContent("device offline")
  await userEvent.click(screen.getByRole("button", { name: "Try again" }))
  expect(await screen.findByText("login-fix → main")).toBeInTheDocument()
})

it("lists blocked credential attempts", async () => {
  setup("req-blocked")
  const section = await screen.findByLabelText("Credentials used")
  expect(section).toHaveTextContent("Blocked: api.stripe.com (POST, 3 attempts)")
})

it("disables Keep with the reason when the request is stale", async () => {
  const keep = vi.fn()
  setup("req-stale", { keep })
  await screen.findByText("billing-fix → main")
  expect(screen.getByRole("button", { name: "Keep" })).toBeDisabled()
  expect(screen.getByRole("alert")).toHaveTextContent(staleReason)
  expect(staleReason).toMatch(/no longer matches.*request again/)
  expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull()
  expect(screen.getByRole("button", { name: "Throw away" })).toBeEnabled()
  expect(keep).not.toHaveBeenCalled()
})

it("reports a kept request with the undo command and disables Keep and Throw away", async () => {
  const keep = vi.fn().mockImplementation(createReviewRequestFixtureActions().keep)
  setup("req-fresh", { keep })
  await keepConfirmed()
  expect(await screen.findByRole("status")).toHaveTextContent("Kept into main. Undo with silo undo main.")
  expect(keep).toHaveBeenCalledWith({ requestId: "req-fresh", deviceId: "dev-2" })
  expect(screen.getByRole("button", { name: "Throw away" })).toBeDisabled()
  expect(screen.getByRole("button", { name: "Keep" })).toBeDisabled()
  expect(screen.getByText(/created .* · Kept/)).toBeInTheDocument()
})

it("shows a refusal verbatim without offering the data option", async () => {
  setup("req-fresh", { keep: refusal("divergent", "main changed README.md. main was not changed.") })
  await keepConfirmed()
  expect(await screen.findByRole("alert")).toHaveTextContent("main changed README.md. main was not changed.")
  expect(screen.queryByRole("button", { name: "Keep with data changes" })).toBeNull()
})

it("offers dataOk as a second confirmation when the refusal is about data", async () => {
  const keep = refusal("database", "Database changed (postgres app) and no migration covers it. main was not changed.")
  setup("req-fresh", { keep })
  await keepConfirmed()
  await userEvent.click(await screen.findByRole("button", { name: "Keep with data changes" }))
  expect(keep).toHaveBeenCalledTimes(1)
  const popover = document.querySelector<HTMLElement>("[data-slot=popover-content]")!
  await userEvent.click(within(popover).getByRole("button", { name: "Keep anyway" }))
  await waitFor(() => expect(keep).toHaveBeenLastCalledWith({ requestId: "req-fresh", deviceId: "dev-2", dataOk: true }))
})

it("confirms Throw away by naming the fork, then goes back", async () => {
  const onBack = vi.fn()
  const throwAway = vi.fn().mockResolvedValue({ request: reviewRequestFixtures["req-fresh"].request, dropped: "login-fix" })
  setup("req-fresh", { throwAway }, onBack)
  await userEvent.click(await screen.findByRole("button", { name: "Throw away" }))
  const popover = document.querySelector<HTMLElement>("[data-slot=popover-content]")!
  expect(popover).toHaveTextContent("Throw away login-fix?")
  expect(throwAway).not.toHaveBeenCalled()
  await userEvent.click(within(popover).getByRole("button", { name: "Throw away" }))
  await waitFor(() => expect(onBack).toHaveBeenCalled())
  expect(throwAway).toHaveBeenCalledWith({ requestId: "req-fresh", deviceId: "dev-2" })
})

it("opens the fork and shows its URLs and SSH name", async () => {
  setup("req-fresh")
  await userEvent.click(await screen.findByRole("button", { name: "Open fork" }))
  const section = await screen.findByLabelText("Open fork", { selector: "section" })
  expect(section).toHaveTextContent("silo-login-fix")
  expect(within(section).getByText("http://login-fix-3000.localhost")).toBeInTheDocument()
  expect(within(section).queryByRole("link")).toBeNull()
  expect(within(section).getByRole("button", { name: "Copy http://login-fix-3000.localhost" })).toBeInTheDocument()
})
