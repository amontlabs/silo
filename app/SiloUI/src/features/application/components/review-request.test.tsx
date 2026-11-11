import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { createReviewRequestFixtureActions, reviewRequestFixtures } from "@/fixtures/review-requests"
import { keepRequestResultSchema } from "@/desktop/production-schemas"
import contracts from "@/test/contracts/review-requests.json"
import { ReviewRequest, staleReason, type ReviewRequestActions } from "./review-request"

async function keepConfirmed() {
  await userEvent.click(await screen.findByRole("button", { name: "Merge" }))
  const popover = document.querySelector<HTMLElement>("[data-slot=popover-content]")!
  expect(popover).toHaveTextContent("Merge into main?")
  await userEvent.click(within(popover).getByRole("button", { name: "Merge" }))
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
  expect(screen.getByRole("button", { name: "Merge" })).toBeEnabled()
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

it("disables Merge with the reason when the request is stale", async () => {
  const keep = vi.fn()
  setup("req-stale", { keep })
  await screen.findByText("billing-fix → main")
  expect(screen.getByRole("button", { name: "Merge" })).toBeDisabled()
  expect(screen.getByRole("alert")).toHaveTextContent(staleReason)
  expect(staleReason).toMatch(/changed since.*ask again/)
  expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull()
  expect(screen.getByRole("button", { name: "Delete fork" })).toBeEnabled()
  expect(keep).not.toHaveBeenCalled()
})

it("reports a merged request with the undo command and disables Merge and Delete fork", async () => {
  const keep = vi.fn().mockImplementation(createReviewRequestFixtureActions().keep)
  setup("req-fresh", { keep })
  await keepConfirmed()
  expect(await screen.findByRole("status")).toHaveTextContent("Merged into main. Undo with silo undo main.")
  expect(keep).toHaveBeenCalledWith({ requestId: "req-fresh", deviceId: "dev-2" })
  expect(screen.getByRole("button", { name: "Delete fork" })).toBeDisabled()
  expect(screen.getByRole("button", { name: "Merge" })).toBeDisabled()
  expect(screen.getByText(/created .* · Merged/)).toBeInTheDocument()
})

it("shows a refusal verbatim without offering the data option", async () => {
  setup("req-fresh", { keep: refusal("divergent", "main changed README.md. main was not changed.") })
  await keepConfirmed()
  expect(await screen.findByRole("alert")).toHaveTextContent("main changed README.md. main was not changed.")
  expect(screen.queryByRole("button", { name: "Merge with data changes" })).toBeNull()
})

it("offers dataOk as a second confirmation when the refusal is about data", async () => {
  const keep = refusal("database", "Database changed (postgres app) and no migration covers it. main was not changed.")
  setup("req-fresh", { keep })
  await keepConfirmed()
  await userEvent.click(await screen.findByRole("button", { name: "Merge with data changes" }))
  expect(keep).toHaveBeenCalledTimes(1)
  const popover = document.querySelector<HTMLElement>("[data-slot=popover-content]")!
  await userEvent.click(within(popover).getByRole("button", { name: "Merge anyway" }))
  await waitFor(() => expect(keep).toHaveBeenLastCalledWith({ requestId: "req-fresh", deviceId: "dev-2", dataOk: true }))
})

it("confirms Delete fork by saying it deletes the fork, then goes back", async () => {
  const onBack = vi.fn()
  const throwAway = vi.fn().mockResolvedValue({ request: reviewRequestFixtures["req-fresh"].request, dropped: "login-fix" })
  setup("req-fresh", { throwAway }, onBack)
  await userEvent.click(await screen.findByRole("button", { name: "Delete fork" }))
  const popover = document.querySelector<HTMLElement>("[data-slot=popover-content]")!
  expect(popover).toHaveTextContent("Delete the fork login-fix?")
  expect(popover).toHaveTextContent("deletes the fork and everything in it")
  expect(throwAway).not.toHaveBeenCalled()
  await userEvent.click(within(popover).getByRole("button", { name: "Delete fork" }))
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

const conflictsRefusal = () => vi.fn().mockImplementationOnce(async () => keepRequestResultSchema.parse(contracts.choicesNeeded)).mockImplementation(async () => keepRequestResultSchema.parse(contracts.fastForwardKeep))

it("lists the conflicts, asks for one choice each, and merges again with the choices", async () => {
  const keep = conflictsRefusal()
  setup("req-fresh", { keep })
  await keepConfirmed()
  const conflicts = await screen.findByRole("region", { name: "Conflicts" })
  expect(conflicts).toHaveTextContent("changed the port")
  expect(conflicts).toHaveTextContent("changed the log level")
  expect(conflicts).toHaveTextContent("3 new commits")
  const merge = within(conflicts).getByRole("button", { name: "Merge with these choices" })
  expect(merge).toBeDisabled()
  expect(conflicts).toHaveTextContent("Choose for 2 more items.")
  const file = within(conflicts).getByRole("group", { name: "/etc/app.conf" })
  expect(within(file).getAllByRole("radio").map(radio => radio.closest("label")?.textContent)).toEqual(["Use the fork's version", "Keep main's version"])
  await userEvent.click(within(file).getByRole("radio", { name: "Use the fork's version" }))
  const repo = within(conflicts).getByRole("group", { name: "/root/app" })
  expect(within(repo).getByRole("radio", { name: /Leave the fork's commits as a branch \(silo\/login-fix\)/ })).toBeInTheDocument()
  await userEvent.click(within(repo).getByRole("radio", { name: "Keep main's version" }))
  expect(merge).toBeEnabled()
  await userEvent.click(merge)
  await waitFor(() => expect(keep).toHaveBeenLastCalledWith({ requestId: "req-fresh", deviceId: "dev-2", choices: { "file:/etc/app.conf": "fork", "repo:/root/app": "main" } }))
  expect(await screen.findByRole("status")).toHaveTextContent("Merged into main-1.")
})

it("shows what the engine reports for an item no answer settles, and offers no choice for it", async () => {
  const keep = vi.fn().mockResolvedValue(keepRequestResultSchema.parse({
    ...contracts.choicesNeeded,
    refusal: { ...contracts.choicesNeeded.refusal, reason: "credentials", conflicts: [{ kind: "credentials", item: "/root/.npmrc", detail: "A credential. Silo never brings these over.", id: "", allowed: [], forkChange: "added a token", mainChange: "" }] },
  }))
  setup("req-fresh", { keep })
  await keepConfirmed()
  const conflicts = await screen.findByRole("region", { name: "Conflicts" })
  expect(conflicts).toHaveTextContent("A credential. Silo never brings these over.")
  expect(within(conflicts).queryAllByRole("radio")).toHaveLength(0)
  expect(within(conflicts).queryByRole("button", { name: "Merge with these choices" })).toBeNull()
})

it("says when main had not changed so nothing could conflict", async () => {
  setup("req-fresh", { keep: vi.fn().mockResolvedValue(keepRequestResultSchema.parse(contracts.fastForwardKeep)) })
  await keepConfirmed()
  expect(await screen.findByRole("status")).toHaveTextContent("Main had not changed since the fork was made, so nothing could conflict.")
})

it("shows extra header buttons and the intro a caller gives", async () => {
  render(<ReviewRequest requestId="req-fresh" actions={createReviewRequestFixtureActions()} extraActions={<button type="button">Deny</button>} intro={<p>From the agent</p>} />)
  expect(await screen.findByRole("button", { name: "Deny" })).toBeInTheDocument()
  expect(screen.getByText("From the agent")).toBeInTheDocument()
})

it("names the target from the request itself when the list did not", async () => {
  const fixture = createReviewRequestFixtureActions()
  const show = vi.fn().mockImplementation(async (target) => ({ ...(await fixture.show(target)), targetName: "Main computer" }))
  render(<ReviewRequest requestId="req-fresh" actions={createReviewRequestFixtureActions({ show })} />)
  expect(await screen.findByText("login-fix → Main computer")).toBeInTheDocument()
})

it("says what to do in plain words for a refusal, not in the words of the command line", async () => {
  const keep = vi.fn().mockImplementation(async () => ({
    status: "refused",
    report: { fork: "login-fix", target: "main", summary: [], steps: [], notes: [] },
    refusal: {
      reason: "choices-needed", message: "One item changed on both sides and needs an answer before login-fix can be merged into main: /etc/hosts. main was not changed.",
      hint: "Merge again with `choices`: an answer for each listed id. Add an id to `skip` to leave an item that does not conflict.",
      conflicts: [],
    },
    request: reviewRequestFixtures["req-fresh"].request,
  }))
  setup("req-fresh", { keep })
  await keepConfirmed()
  const alert = await screen.findByText(/needs an answer/)
  expect(alert.closest("[role=alert]")).toHaveTextContent("Choose the version to keep for each item, then merge again.")
  expect(alert.closest("[role=alert]")).not.toHaveTextContent("`")
  expect(alert.closest("[role=alert]")).not.toHaveTextContent("skip")
})
