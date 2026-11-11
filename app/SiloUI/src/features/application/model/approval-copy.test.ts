import { expect, it } from "vitest"

import { approvalListSchema } from "@/desktop/production-schemas"

import { approvalKindLabel, approvalOutcomeText } from "./approval-copy"

const network = (change: Record<string, unknown>) => approvalListSchema.parse({ approvals: [{
  id: "ap-network", kind: "network", computerId: "c", computerName: "main", method: "network.policy.set", params: {},
  summary: "Let main reach registry.npmjs.org. It can now reach 1 host.", state: "allowed", createdAt: "2026-11-11T09:00:00Z", ...change,
}] }).approvals[0]!

it("names a request to widen a network as network access", () => {
  expect(approvalKindLabel("network")).toBe("Network access")
})

it("says when an allowed network change takes effect, from the engine's own result", () => {
  const outcome = (timing: string) => approvalOutcomeText(network({ outcome: { result: { computerId: "c", timing } } }))
  expect(outcome("nextRestart")).toEqual({ tone: "done", text: "Done. It applies when the computer restarts." })
  expect(outcome("nextStart")).toEqual({ tone: "done", text: "Done. It applies when the computer next starts." })
  expect(outcome("applied")).toEqual({ tone: "done", text: "Done." })
})

it("shows a network change the engine could not make as its error", () => {
  expect(approvalOutcomeText(network({ outcome: { error: "The computer is gone." } }))).toEqual({ tone: "failed", text: "Could not run: The computer is gone." })
})
