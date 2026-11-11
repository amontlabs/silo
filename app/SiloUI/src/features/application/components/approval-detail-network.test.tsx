import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { approvalListSchema } from "@/desktop/production-schemas"

import { ApprovalDetail } from "./approval-detail"

const summary = "Let main reach registry.npmjs.org and api.example.net. It can now reach 2 hosts instead of none."
const approval = (state: string, extra: Record<string, unknown> = {}) => approvalListSchema.parse({ approvals: [{
  id: "ap-network", kind: "network", computerId: "c", computerName: "main", method: "network.policy.set", params: {},
  summary, note: "I need the registry to install dependencies.", state, createdAt: new Date().toISOString(), ...extra,
}] }).approvals[0]!

it("shows a waiting request to widen a network with the engine's sentence, the agent's note apart and Allow and Deny", async () => {
  const decideApproval = vi.fn(async () => approval("allowed"))
  render(<ApprovalDetail approval={approval("pending")} actions={{ decideApproval, readApprovalJob: vi.fn() }} onBack={vi.fn()} onChanged={vi.fn()} />)
  expect(screen.getByText("main asks")).toBeVisible()
  expect(screen.getByText(/^Network access ·/)).toBeVisible()
  expect(screen.getByLabelText("What is asked")).toHaveTextContent(summary)
  expect(screen.getByLabelText("The agent's note")).toHaveTextContent("The agent wrote this note. Silo has not checked it.")
  await userEvent.click(screen.getByRole("button", { name: "Allow" }))
  expect(decideApproval).toHaveBeenCalledExactlyOnceWith({ id: "ap-network", decision: "allow" })
})

it("says after Allow that the change waits for the computer's restart", () => {
  render(<ApprovalDetail approval={approval("allowed", { outcome: { result: { computerId: "c", timing: "nextRestart" } } })} actions={{ decideApproval: vi.fn(), readApprovalJob: vi.fn() }} onBack={vi.fn()} onChanged={vi.fn()} />)
  expect(screen.getByRole("status")).toHaveTextContent("Done. It applies when the computer restarts.")
})

it("strips bidi overrides, preserves meaningful newlines, and displays computer id beside its name", () => {
  render(
    <ApprovalDetail
      approval={approval("pending", {
        computerId: "cmp-id-456",
        computerName: "dev\u202Efake",
        summary: "Line 1\nLine 2\u202Ebad",
        note: "Note 1\u200B\nNote 2",
      })}
      actions={{ decideApproval: vi.fn(), readApprovalJob: vi.fn() }}
      onBack={vi.fn()}
      onChanged={vi.fn()}
    />
  )
  expect(screen.getByText("devfake asks")).toBeVisible()
  expect(screen.getByText("cmp-id-456")).toBeVisible()
  const summaryEl = screen.getByLabelText("What is asked").querySelector("p")
  expect(summaryEl).toHaveClass("whitespace-pre-line")
  expect(summaryEl?.textContent).toBe("Line 1\nLine 2bad")
})
