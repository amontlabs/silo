import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { createApprovalFixture, type ApprovalFixture } from "@/fixtures/approvals"
import { bucketSummary, PENDING_BACKGROUND_INTERVAL_MS, PENDING_CHANGES_INTERVAL_MS, pendingChangesDelay, pendingRows, usePending } from "../model/use-pending-changes"
import { approvalListSchema } from "@/desktop/production-schemas"
import { PendingChangesPage } from "./pending-changes-page"

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks() })

const NOW = Date.parse("2026-11-11T12:00:00Z")

function Harness({ fixture, active = true, ...props }: { fixture: ApprovalFixture; active?: boolean } & Partial<Parameters<typeof PendingChangesPage>[0]>) {
  const pending = usePending(fixture.actions, active)
  return <PendingChangesPage pending={pending} actions={fixture.actions} reviewActions={fixture.actions.reviewRequestActions} {...props} />
}

it("lists approvals and review requests together, newest first, each with the computer, what is asked, when and the device", async () => {
  const fixture = createApprovalFixture({ now: Date.now() })
  render(<Harness fixture={fixture} />)
  const waiting = await screen.findByLabelText("Waiting for you")
  const titles = within(waiting).getAllByRole("button").map(button => button.textContent)
  // Newest first: the conflicting merge was asked 4 minutes ago, the fast-forward one 20, the port 35.
  expect(titles.slice(0, 3)).toEqual(["main", "main", "main"])
  const summaries = Array.from(waiting.querySelectorAll("[data-slot='list-row-content']")).map(row => row.textContent ?? "")
  expect(summaries[0]).toContain("Merge auth-redirect into main")
  expect(summaries[0]).toContain("4 minutes ago")
  expect(summaries[0]).toContain("asks")
  expect(summaries[1]).toContain("Merge api-docs into main")
  expect(summaries[2]).toContain("Publish port 8080 on this device")
  // A review request nobody's agent asked about stays a row of its own, with its device.
  const remote = summaries.find(row => row.startsWith("cache-tune"))!
  expect(remote).toContain("Mac mini")
  expect(screen.getByText("Office")).toBeInTheDocument()
})

it("shows a merge a computer asked for as one row, not two", async () => {
  render(<Harness fixture={createApprovalFixture({ now: NOW })} />)
  const waiting = await screen.findByLabelText("Waiting for you")
  expect(within(waiting).queryByRole("button", { name: "auth-redirect" })).toBeNull()
  expect(within(waiting).queryByRole("button", { name: "api-docs" })).toBeNull()
  expect(waiting.textContent?.match(/Merge auth-redirect into main/g)).toHaveLength(1)
})

it("keeps a review request whose approval was answered as a row of its own", () => {
  const approvals = approvalListSchema.parse({ approvals: [{ id: "ap-1", kind: "merge", computerId: "c", computerName: "main", summary: "Merge x", state: "denied", createdAt: "2026-11-11T11:00:00Z", reviewRequestId: "req-1" }] }).approvals
  const request = { request: { id: "req-1", forkId: "f", target: "t", createdAt: "2026-11-11T11:00:00Z", reviewDigest: "d", summary: "", status: "open" as const, counts: {}, approvalId: "ap-1", keepId: undefined, resolvedAt: undefined, keepingSinceMs: undefined, closedReason: undefined }, forkName: "x", forkState: undefined, targetName: "main", deviceId: undefined, deviceName: undefined }
  const rows = pendingRows(approvals, [request])
  expect(rows.waiting.map(row => row.kind)).toEqual(["request"])
  expect(rows.answered.map(({ id }) => id)).toEqual(["ap-1"])
})

it("opens an approval: the engine's summary, then the agent's note under a label that names its author", async () => {
  const user = userEvent.setup()
  render(<Harness fixture={createApprovalFixture({ now: NOW })} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[2]!)
  expect(await screen.findByText("main asks")).toBeInTheDocument()
  expect(screen.getByLabelText("What is asked")).toHaveTextContent("Publish port 8080 on this device")
  // That approval has no note; a replace-main one does.
  expect(screen.queryByLabelText("The agent's note")).toBeNull()
  await user.click(screen.getByRole("button", { name: "Back" }))
  await user.click(screen.getAllByRole("button", { name: "docs-build" })[0]!)
  const note = await screen.findByLabelText("The agent's note")
  expect(within(note).getByText("The agent's own words")).toBeInTheDocument()
  expect(note).toHaveTextContent("main is out of date; every change in try-redis is wanted.")
  expect(note).toHaveTextContent("Silo has not checked it.")
  expect(screen.getByLabelText("What is asked")).toHaveTextContent("This restarts main")
})

it("allows an approval and shows the outcome", async () => {
  const user = userEvent.setup()
  const fixture = createApprovalFixture({ now: NOW })
  const decide = vi.spyOn(fixture.actions, "decideApproval")
  render(<Harness fixture={fixture} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[2]!)
  await user.click(await screen.findByRole("button", { name: "Allow" }))
  expect(decide).toHaveBeenCalledWith({ id: "ap-port", decision: "allow" })
  const outcome = await screen.findByLabelText("Outcome")
  expect(outcome).toHaveTextContent("You allowed this")
  expect(outcome).toHaveTextContent("Done (applied).")
  expect(screen.queryByRole("button", { name: "Allow" })).toBeNull()
})

it("denies with a note the agent will read, and shows it was denied", async () => {
  const user = userEvent.setup()
  const fixture = createApprovalFixture({ now: NOW })
  const decide = vi.spyOn(fixture.actions, "decideApproval")
  render(<Harness fixture={fixture} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[2]!)
  await user.click(await screen.findByRole("button", { name: "Deny" }))
  const popover = document.querySelector<HTMLElement>("[data-slot=popover-content]")!
  await user.type(within(popover).getByLabelText("Note for the agent (optional)"), "Use port 8081.")
  await user.click(within(popover).getByRole("button", { name: "Deny" }))
  await waitFor(() => expect(decide).toHaveBeenCalledWith({ id: "ap-port", decision: "deny", note: "Use port 8081." }))
  expect(await screen.findByLabelText("Outcome")).toHaveTextContent("You denied this")
  expect(screen.getByLabelText("Outcome")).toHaveTextContent("Your note: Use port 8081.")
})

it("denies without a note when none is written", async () => {
  const user = userEvent.setup()
  const fixture = createApprovalFixture({ now: NOW })
  const decide = vi.spyOn(fixture.actions, "decideApproval")
  render(<Harness fixture={fixture} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[2]!)
  await user.click(await screen.findByRole("button", { name: "Deny" }))
  await user.click(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Deny" }))
  await waitFor(() => expect(decide).toHaveBeenCalledWith({ id: "ap-port", decision: "deny" }))
})

it("shows the error when an answer cannot be sent", async () => {
  const user = userEvent.setup()
  const fixture = createApprovalFixture({ now: NOW })
  vi.spyOn(fixture.actions, "decideApproval").mockRejectedValue(new Error("Silo is not running."))
  render(<Harness fixture={fixture} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[2]!)
  await user.click(await screen.findByRole("button", { name: "Allow" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("Silo is not running.")
  expect(screen.getByRole("button", { name: "Allow" })).toBeEnabled()
})

it("shows what the approvals answered lately came to: done, failed with the error, running, denied and expired", async () => {
  render(<Harness fixture={createApprovalFixture({ now: NOW })} />)
  const answered = await screen.findByLabelText("Answered")
  const text = answered.textContent ?? ""
  expect(text).toContain("Allow main to reach registry.npmjs.org")
  expect(within(answered).getAllByText("Done (applied).")).toHaveLength(1)
  expect(within(answered).getByText(/Could not run: This device has 3.1 GiB of memory available/)).toHaveClass("text-destructive")
  expect(within(answered).getByText("Running…")).toBeInTheDocument()
  expect(within(answered).getByText("You said no.")).toBeInTheDocument()
  expect(within(answered).getByText("Nobody answered in time.")).toBeInTheDocument()
})

it("follows a job an approval started until it ends", async () => {
  vi.useFakeTimers()
  const fixture = createApprovalFixture({ now: NOW })
  render(<Harness fixture={fixture} />)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  const answered = screen.getByLabelText("Answered")
  expect(within(answered).getByText("Running…")).toBeInTheDocument()
  await act(async () => { await vi.advanceTimersByTimeAsync(5_000) })
  expect(within(answered).queryByText("Running…")).toBeNull()
  expect(within(answered).getAllByText("Done.")).toHaveLength(1)
})

it("opens a merge with the review view and its diff, and merging answers the approval", async () => {
  const user = userEvent.setup()
  const fixture = createApprovalFixture({ now: NOW })
  render(<Harness fixture={fixture} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[1]!)
  expect(await screen.findByText("api-docs → main")).toBeInTheDocument()
  expect(screen.getByLabelText("What is asked")).toHaveTextContent("Merge api-docs into main")
  expect(screen.getByLabelText("Diff")).toHaveTextContent("~/docs: 1a2b3c4..5d6e7f8")
  expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled()
  await user.click(screen.getByRole("button", { name: "Merge" }))
  await user.click(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Merge" }))
  expect(await screen.findByText(/Main had not changed since the fork was made/)).toBeInTheDocument()
  await waitFor(() => expect(fixture.approvals().find(({ id }) => id === "ap-fast-forward")?.state).toBe("allowed"))
})

it("switches from a non-merge approval to a merge approval and shows the review/diff state", async () => {
  const fixture = createApprovalFixture({ now: NOW })
  const { rerender } = render(<Harness fixture={fixture} openApproval={{ id: "ap-port", token: 1 }} />)
  expect(await screen.findByText("Publish port 8080 on this device")).toBeInTheDocument()
  expect(screen.queryByLabelText("Diff")).toBeNull()

  // Switch to a merge approval while an approval is open
  rerender(<Harness fixture={fixture} openApproval={{ id: "ap-fast-forward", token: 2 }} />)
  expect(await screen.findByLabelText("Diff")).toBeInTheDocument()
  expect(screen.getByText("api-docs → main")).toBeInTheDocument()
})

it("asks for a choice per conflict when a merge needs them, then merges with the choices", async () => {
  const user = userEvent.setup()
  const fixture = createApprovalFixture({ now: NOW })
  const keep = vi.spyOn(fixture.actions.reviewRequestActions, "keep")
  render(<Harness fixture={fixture} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[0]!)
  await user.click(await screen.findByRole("button", { name: "Merge" }))
  await user.click(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Merge" }))
  const conflicts = await screen.findByRole("region", { name: "Conflicts" })
  expect(conflicts).toHaveTextContent("No answer settles this one")
  await user.click(within(conflicts).getByRole("radio", { name: "Use the fork's version" }))
  await user.click(within(conflicts).getByRole("radio", { name: /Leave the fork's commits as a branch/ }))
  await user.click(within(conflicts).getByRole("button", { name: "Merge with these choices" }))
  await waitFor(() => expect(keep).toHaveBeenLastCalledWith({ requestId: "req-conflicts", choices: { "file:/etc/app.conf": "fork", "repo:/root/app": "branch" } }))
  await waitFor(() => expect(fixture.approvals().find(({ id }) => id === "ap-conflicts")?.state).toBe("allowed"))
})

it("denies a merge with a note without deleting the fork", async () => {
  const user = userEvent.setup()
  const fixture = createApprovalFixture({ now: NOW })
  const decide = vi.spyOn(fixture.actions, "decideApproval")
  const throwAway = vi.spyOn(fixture.actions.reviewRequestActions, "throwAway")
  render(<Harness fixture={fixture} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[0]!)
  await user.click(await screen.findByRole("button", { name: "Deny" }))
  const popover = document.querySelector<HTMLElement>("[data-slot=popover-content]")!
  await user.type(within(popover).getByLabelText("Note for the agent (optional)"), "Not this week.")
  await user.click(within(popover).getByRole("button", { name: "Deny" }))
  await waitFor(() => expect(decide).toHaveBeenCalledWith({ id: "ap-conflicts", decision: "deny", note: "Not this week." }))
  expect(throwAway).not.toHaveBeenCalled()
})

it("shows the device's memory and disk next to a request for more resources", async () => {
  const user = userEvent.setup()
  render(<Harness fixture={createApprovalFixture({ now: NOW })} />)
  await user.click((await screen.findAllByRole("button", { name: "main" }))[3]!)
  const now = await screen.findByLabelText("This device now")
  expect(now).toHaveTextContent("3.1 GiB of 16.0 GiB memory available")
})

it("opens the approval a notification names", async () => {
  const fixture = createApprovalFixture({ now: NOW })
  render(<Harness fixture={fixture} openApproval={{ id: "ap-ssh", token: 1 }} />)
  expect(await screen.findByLabelText("What is asked")).toHaveTextContent("Allow SSH into main from outside this device on port 2222")
})

it("says so when the approval a notification names is gone", async () => {
  render(<Harness fixture={createApprovalFixture({ now: NOW })} openApproval={{ id: "ap-missing", token: 1 }} />)
  expect(await screen.findByText(/no longer waiting/)).toBeInTheDocument()
})

it("shows an unreachable device as an unavailable row with its reason", async () => {
  render(<Harness fixture={createApprovalFixture({ now: NOW })} />)
  const row = (await screen.findByText("Office")).closest("[data-slot='list-row-content']") as HTMLElement
  expect(within(row).getByText("Unavailable")).toBeInTheDocument()
  expect(row).toHaveTextContent("The device did not answer.")
})

it("shows an empty state when nothing is waiting", async () => {
  const fixture = createApprovalFixture({ mode: "none", now: NOW })
  fixture.actions.listReviewRequests = async () => ({ requests: [], unreachable: [] })
  render(<Harness fixture={fixture} />)
  expect(await screen.findByText("Nothing is waiting for you")).toBeInTheDocument()
})

it("opens a review request that no agent asked about, naming its device", async () => {
  const user = userEvent.setup()
  const open = vi.fn()
  render(<Harness fixture={createApprovalFixture({ now: NOW })} onOpenRequest={open} />)
  await user.click(await screen.findByRole("button", { name: "cache-tune" }))
  expect(open).toHaveBeenCalledWith("req-cache-tune", "mac-mini")
  expect(await screen.findByText(/on Mac mini/)).toBeInTheDocument()
})

it("goes back from a review to a refreshed list", async () => {
  const user = userEvent.setup()
  const fixture = createApprovalFixture({ now: NOW })
  const list = vi.spyOn(fixture.actions, "listReviewRequests")
  render(<Harness fixture={fixture} />)
  await user.click(await screen.findByRole("button", { name: "login-fix" }))
  expect(await screen.findByText("login-fix → main")).toBeInTheDocument()
  const before = list.mock.calls.length
  await user.click(screen.getByRole("button", { name: "Back" }))
  expect(await screen.findByLabelText("Waiting for you")).toBeInTheDocument()
  await waitFor(() => expect(list.mock.calls.length).toBeGreaterThan(before))
})

it("reads again at once when an approval changes", async () => {
  const fixture = createApprovalFixture({ now: NOW })
  const list = vi.spyOn(fixture.actions, "listApprovals")
  render(<Harness fixture={fixture} />)
  await screen.findByLabelText("Waiting for you")
  const before = list.mock.calls.length
  await act(async () => { await fixture.actions.decideApproval({ id: "ap-port", decision: "deny" }) })
  await waitFor(() => expect(list.mock.calls.length).toBeGreaterThan(before))
  await waitFor(() => expect(within(screen.getByLabelText("Waiting for you")).queryByText(/Publish port 8080/)).toBeNull())
})

it("polls as a fallback, backs off while reads fail, keeps the last list, and recovers", async () => {
  vi.useFakeTimers()
  const fixture = createApprovalFixture({ now: NOW })
  let failing = false
  const real = fixture.actions.listApprovals
  const list = vi.fn(async (options?: Parameters<typeof real>[0]) => { if (failing) throw new Error("Device unavailable"); return real(options) })
  fixture.actions.listApprovals = list
  render(<Harness fixture={fixture} />)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(list).toHaveBeenCalledTimes(1)
  failing = true
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_CHANGES_INTERVAL_MS) })
  expect(list).toHaveBeenCalledTimes(2)
  expect(screen.getByRole("alert")).toHaveTextContent("Device unavailable")
  expect(screen.getAllByRole("button", { name: "main" }).length).toBeGreaterThan(0)
  // After one failure the wait doubles.
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_CHANGES_INTERVAL_MS) })
  expect(list).toHaveBeenCalledTimes(2)
  failing = false
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_CHANGES_INTERVAL_MS) })
  expect(list).toHaveBeenCalledTimes(3)
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("polls slowly while the page is not open", async () => {
  vi.useFakeTimers()
  const fixture = createApprovalFixture({ now: NOW })
  const list = vi.spyOn(fixture.actions, "listApprovals")
  render(<Harness fixture={fixture} active={false} />)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(list).toHaveBeenCalledTimes(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_CHANGES_INTERVAL_MS) })
  expect(list).toHaveBeenCalledTimes(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_BACKGROUND_INTERVAL_MS) })
  expect(list).toHaveBeenCalledTimes(2)
})

it("caps the backoff and summarizes buckets", () => {
  expect([0, 1, 2, 10].map(failures => pendingChangesDelay(failures))).toEqual([5_000, 10_000, 20_000, 60_000])
  expect(bucketSummary({ code: 1, packages: 0, settings: 2 })).toBe("1 code · 2 settings & files")
  expect(bucketSummary({})).toBe("No changes")
})

