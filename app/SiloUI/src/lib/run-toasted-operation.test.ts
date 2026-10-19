import { beforeEach, expect, it, vi } from "vitest"

import { runToastedOperation } from "@/lib/run-toasted-operation"

const toasts = vi.hoisted(() => ({ showOperationProgress: vi.fn(), showOperationSuccess: vi.fn(), showOperationFailure: vi.fn() }))
vi.mock("@/lib/operation-toast", () => toasts)

beforeEach(() => vi.clearAllMocks())

it("shows progress then success and runs the follow-up", async () => {
  const onSuccess = vi.fn()
  const ok = await runToastedOperation({
    id: "op", progress: { title: "Working", progress: null }, work: async () => 7,
    success: { title: "Done", computer: "dev" }, failure: { title: "Failed" }, onSuccess,
  })
  expect(ok).toBe(true)
  expect(toasts.showOperationProgress).toHaveBeenCalledWith("op", { title: "Working", progress: null })
  expect(toasts.showOperationSuccess).toHaveBeenCalledWith("op", "Done", { computer: "dev" })
  expect(onSuccess).toHaveBeenCalledWith(7)
  expect(toasts.showOperationFailure).not.toHaveBeenCalled()
})

it("reports the error with the failure options and Retry", async () => {
  const retry = vi.fn()
  const ok = await runToastedOperation({ id: "op", work: async () => { throw new Error("boom") }, failure: { title: "Failed", retry, native: false } })
  expect(ok).toBe(false)
  expect(toasts.showOperationProgress).not.toHaveBeenCalled()
  expect(toasts.showOperationFailure).toHaveBeenCalledWith("op", "Failed", { retry, native: false, description: "boom" })
})

it("uses the fallback for unreadable errors and reports a throwing follow-up as a failure", async () => {
  await runToastedOperation({ id: "a", work: async () => { throw {} }, failure: { title: "Failed", fallback: "Could not." } })
  expect(toasts.showOperationFailure).toHaveBeenCalledWith("a", "Failed", { description: "Could not." })
  const ok = await runToastedOperation({ id: "b", work: async () => 1, onSuccess: () => { throw new Error("late") }, failure: { title: "Failed" } })
  expect(ok).toBe(false)
  expect(toasts.showOperationFailure).toHaveBeenLastCalledWith("b", "Failed", { description: "late" })
})
