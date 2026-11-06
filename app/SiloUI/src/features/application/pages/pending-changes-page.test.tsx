import { act, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { requestListSchema } from "@/desktop/production-schemas"
import { fixtureListReviewRequests, fixtureReviewRequests } from "@/fixtures/pending-changes"
import { bucketSummary, PENDING_CHANGES_INTERVAL_MS, pendingChangesDelay } from "../model/use-pending-changes"
import { createReviewRequestFixtureActions } from "@/fixtures/review-requests"
import { PendingChangesPage } from "./pending-changes-page"

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks() })

it("lists the requests of every device with target, device, counts and status", async () => {
  const load = vi.fn(fixtureListReviewRequests)
  render(<PendingChangesPage listReviewRequests={load} onOpenRequest={vi.fn()} />)
  const rows = await screen.findAllByRole("button", { name: /login-fix|docs-pass|cache-tune/ })
  expect(rows).toHaveLength(3)
  expect(load).toHaveBeenCalledWith({ status: "open", allDevices: true })
  const login = screen.getByRole("button", { name: "login-fix" }).closest("[data-slot='list-row-content']") as HTMLElement
  expect(login).toHaveTextContent("into main")
  expect(login).toHaveTextContent("This device")
  expect(login).toHaveTextContent("2 code · 1 packages · 1 settings")
  const remote = screen.getByRole("button", { name: "cache-tune" }).closest("[data-slot='list-row-content']") as HTMLElement
  expect(remote).toHaveTextContent("Mac mini")
  expect(remote).toHaveTextContent("No changes")
  expect(screen.getByText("Keeping…")).toBeInTheDocument()
})

it("shows an unreachable device as an unavailable row with its reason", async () => {
  render(<PendingChangesPage listReviewRequests={fixtureListReviewRequests} onOpenRequest={vi.fn()} />)
  const row = (await screen.findByText("Office")).closest("[data-slot='list-row-content']") as HTMLElement
  expect(within(row).getByText("Unavailable")).toBeInTheDocument()
  expect(row).toHaveTextContent(fixtureReviewRequests.unreachable[0]!.reason)
})

it("shows an empty state when nothing is pending", async () => {
  render(<PendingChangesPage listReviewRequests={async () => requestListSchema.parse({ requests: [], unreachable: [] })} onOpenRequest={vi.fn()} />)
  expect(await screen.findByText("No pending changes")).toBeInTheDocument()
})

it("reports the opened request with its device", async () => {
  const open = vi.fn()
  const user = userEvent.setup()
  render(<PendingChangesPage listReviewRequests={fixtureListReviewRequests} reviewActions={createReviewRequestFixtureActions()} onOpenRequest={open} />)
  await user.click(await screen.findByRole("button", { name: "cache-tune" }))
  expect(open).toHaveBeenCalledWith("req-cache-tune", "mac-mini")
})

it("opens the review from a row, names the device, and goes back to a refreshed list", async () => {
  const user = userEvent.setup()
  const load = vi.fn(fixtureListReviewRequests)
  render(<PendingChangesPage listReviewRequests={load} reviewActions={createReviewRequestFixtureActions()} />)
  await user.click(await screen.findByText(/2 code/))
  expect(await screen.findByText("login-fix → main")).toBeInTheDocument()
  expect(screen.queryByLabelText("Pending changes")).toBeNull()
  await user.click(screen.getByRole("button", { name: "Back" }))
  expect(await screen.findByLabelText("Pending changes")).toBeInTheDocument()
  expect(load.mock.calls.length).toBeGreaterThanOrEqual(2)
})

it("shows the device name in the review header", async () => {
  const user = userEvent.setup()
  render(<PendingChangesPage listReviewRequests={fixtureListReviewRequests} reviewActions={createReviewRequestFixtureActions()} />)
  await user.click(await screen.findByRole("button", { name: "cache-tune" }))
  expect(await screen.findByText(/on Mac mini/)).toBeInTheDocument()
})

it("polls, backs off while reads fail, keeps the last list, and recovers", async () => {
  vi.useFakeTimers()
  let failing = false
  const load = vi.fn(async () => { if (failing) throw new Error("Device unavailable"); return fixtureReviewRequests })
  render(<PendingChangesPage listReviewRequests={load} onOpenRequest={vi.fn()} />)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(load).toHaveBeenCalledTimes(1)
  failing = true
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_CHANGES_INTERVAL_MS) })
  expect(load).toHaveBeenCalledTimes(2)
  expect(screen.getByRole("alert")).toHaveTextContent("Device unavailable")
  expect(screen.getByRole("button", { name: "login-fix" })).toBeInTheDocument()
  // After one failure the wait doubles.
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_CHANGES_INTERVAL_MS) })
  expect(load).toHaveBeenCalledTimes(2)
  failing = false
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_CHANGES_INTERVAL_MS) })
  expect(load).toHaveBeenCalledTimes(3)
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  await act(async () => { await vi.advanceTimersByTimeAsync(PENDING_CHANGES_INTERVAL_MS) })
  expect(load).toHaveBeenCalledTimes(4)
})

it("caps the backoff and summarizes buckets", () => {
  expect([0, 1, 2, 10].map(pendingChangesDelay)).toEqual([5_000, 10_000, 20_000, 60_000])
  expect(bucketSummary({ code: 1, packages: 0, settings: 2 })).toBe("1 code · 2 settings & files")
  expect(bucketSummary({})).toBe("No changes")
})
