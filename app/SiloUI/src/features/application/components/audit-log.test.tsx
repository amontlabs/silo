import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createApprovalFixture } from "@/fixtures/approvals"
import { auditAction, isAuditBookkeeping } from "../model/audit-copy"
import { AUDIT_PAGE_SIZE } from "../model/use-audit-log"
import { AuditLog } from "./audit-log"

const computers = applicationSourceForScenario("running").computers
const setup = (props: Partial<Parameters<typeof AuditLog>[0]> = {}) => {
  const fixture = createApprovalFixture({ computers: computers.map(({ configuration }) => ({ id: configuration.id, name: configuration.name })) })
  const read = vi.spyOn(fixture.actions, "listAudit")
  return { fixture, read, user: userEvent.setup(), ...render(<AuditLog read={fixture.actions.listAudit} computers={computers} active {...props} />) }
}

// The table is on screen while the first page is still being read; wait until it stops being busy.
async function loadedTable() {
  const table = await screen.findByRole("table", { name: "Who did what" })
  await waitFor(() => expect(table).not.toHaveAttribute("aria-busy"))
  return table
}

it("shows time, who, what in plain words, the computer, the outcome and a repeat count in a plain table", async () => {
  setup()
  const table = await loadedTable()
  expect(within(table).getAllByRole("columnheader").map(header => header.textContent)).toEqual(["Time", "Who", "What", "Computer", "Outcome", "Request"])
  const rows = within(table).getAllByRole("row").slice(1)
  expect(rows).toHaveLength(AUDIT_PAGE_SIZE)
  // The newest row: a computer's call that the engine held for an answer.
  const first = within(rows[0]!).getAllByRole("cell")
  expect(first[1]).toHaveTextContent(computers[0]!.configuration.name)
  expect(first[2]).toHaveTextContent("Started a computer")
  expect(first[4]).toHaveTextContent("Done")
  // A refused call repeated four times is one row with its count.
  const refused = rows.find(row => within(row).queryByText("×4"))!
  expect(within(refused).getAllByRole("cell")[4]).toHaveTextContent("Refused")
  expect(within(refused).getAllByRole("cell")[4]).toHaveTextContent("permission_denied")
  // A call from the owner reads "You" and another device reads as such.
  expect(within(table).getAllByText("You").length).toBeGreaterThan(0)
  expect(within(table).getAllByText("Another device").length).toBeGreaterThan(0)
})

it("never prints a method name: unknown calls read as a request about their area", () => {
  expect(auditAction("computers.start")).toBe("Started a computer")
  expect(auditAction("guestClient.status")).toBe("Checked Silo's tools in a computer")
  expect(auditAction("steps.record")).toBe("Recorded a step")
  expect(auditAction("changes.diff")).toBe("Looked at a computer's changes")
  expect(auditAction("changes.requests.show")).toBe("Opened a review")
  expect(auditAction("computers.somethingNew")).toBe("Made a request about computers")
  expect(auditAction("future.method")).toBe("Made a request")
  expect(auditAction("")).toBe("Made a request")
  for (const method of ["operations.job.acknowledge", "a.b.c.d", "guestClient.enable"]) expect(auditAction(method)).not.toMatch(/\w\.\w/)
})

it("leaves out bookkeeping calls that went fine, but keeps them when they failed", async () => {
  expect(isAuditBookkeeping({ method: "operations.job.acknowledge", outcome: "ok" })).toBe(true)
  expect(isAuditBookkeeping({ method: "operations.job.acknowledge", outcome: "error" })).toBe(false)
  expect(isAuditBookkeeping({ method: "computers.start", outcome: "ok" })).toBe(false)
  const entry = (id: number, method: string, outcome: "ok" | "error") => ({ id, at: "2026-11-11T12:00:00Z", callerKind: "owner" as const, callerId: undefined, client: undefined, method, computerId: undefined, outcome, errorCode: undefined, jobId: undefined, approvalId: undefined, count: 1, lastAt: undefined })
  render(<AuditLog read={async () => ({ entries: [entry(3, "operations.job.acknowledge", "ok"), entry(2, "operations.job.acknowledge", "error"), entry(1, "computers.start", "ok")], nextBeforeId: undefined })} computers={computers} active />)
  const table = await loadedTable()
  const rows = within(table).getAllByRole("row").slice(1)
  expect(rows.map(row => within(row).getAllByRole("cell")[2]?.textContent)).toEqual(["Marked a background job as seen", "Started a computer"])
  expect(table).not.toHaveTextContent(/operations\.|guestClient\./)
})

it("links a row to the request it belonged to", async () => {
  const onOpenApproval = vi.fn()
  const { user } = setup({ onOpenApproval })
  const table = await loadedTable()
  await user.click(within(table).getAllByRole("button", { name: "Open request" })[0]!)
  expect(onOpenApproval).toHaveBeenCalledWith("ap-port")
})

it("opens a request Silo still lists", async () => {
  const onOpenApproval = vi.fn()
  const { user } = setup({ onOpenApproval, openableApprovals: new Set(["ap-port"]) })
  const table = await loadedTable()
  const button = within(table).getAllByRole("button", { name: "Open request" })[0]!
  expect(button).toBeEnabled()
  await user.click(button)
  expect(onOpenApproval).toHaveBeenCalledWith("ap-port")
})

it("disables Open request with the reason for a request Silo no longer lists", async () => {
  const onOpenApproval = vi.fn()
  const { user } = setup({ onOpenApproval, openableApprovals: new Set() })
  const table = await loadedTable()
  const button = within(table).getAllByRole("button", { name: "Open request" })[0]!
  expect(button).toBeDisabled()
  expect(button.parentElement).toHaveAttribute("title", expect.stringMatching(/no longer in the list/))
  await user.click(button)
  expect(onOpenApproval).not.toHaveBeenCalled()
})

it("filters by computer and by who", async () => {
  const { user, read } = setup()
  await loadedTable()
  await user.selectOptions(screen.getByRole("combobox", { name: "Computer" }), computers[0]!.configuration.id)
  await waitFor(() => expect(read).toHaveBeenLastCalledWith({ computerId: computers[0]!.configuration.id, callerKind: undefined, limit: AUDIT_PAGE_SIZE }))
  await user.selectOptions(screen.getByRole("combobox", { name: "Who" }), "owner")
  await waitFor(() => expect(read).toHaveBeenLastCalledWith({ computerId: computers[0]!.configuration.id, callerKind: "owner", limit: AUDIT_PAGE_SIZE }))
  await waitFor(() => {
    const rows = within(screen.getByRole("table", { name: "Who did what" })).getAllByRole("row").slice(1)
    expect(rows.length).toBeGreaterThan(0)
    for (const row of rows) expect(within(row).getAllByRole("cell")[1]).toHaveTextContent("You")
  })
})

it("loads older rows from the cursor until the last page", async () => {
  const { user, read } = setup()
  const table = await loadedTable()
  const count = () => within(table).getAllByRole("row").length - 1
  expect(count()).toBe(AUDIT_PAGE_SIZE)
  await user.click(screen.getByRole("button", { name: "Load more" }))
  await waitFor(() => expect(count()).toBe(AUDIT_PAGE_SIZE * 2))
  expect(read).toHaveBeenLastCalledWith({ computerId: undefined, callerKind: undefined, beforeId: 120 - AUDIT_PAGE_SIZE + 1, limit: AUDIT_PAGE_SIZE })
  await user.click(screen.getByRole("button", { name: "Load more" }))
  await waitFor(() => expect(count()).toBe(120))
  expect(screen.queryByRole("button", { name: "Load more" })).toBeNull()
})

it("says so when nothing is recorded and when the log cannot be read", async () => {
  const { unmount } = render(<AuditLog read={async () => ({ entries: [], nextBeforeId: undefined })} computers={computers} active />)
  expect(await screen.findByText("Nothing recorded yet")).toBeInTheDocument()
  unmount()
  render(<AuditLog read={async () => { throw new Error("The engine is not answering.") }} computers={computers} active />)
  expect(await screen.findByRole("alert")).toHaveTextContent("The engine is not answering.")
})
