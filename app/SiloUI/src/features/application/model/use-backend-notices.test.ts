import { toast } from "sonner"
import { afterEach, expect, it, vi } from "vitest"

import { showBackendNotice, BACKEND_FAILURE_TOAST_MS } from "./use-backend-notices"

afterEach(() => { vi.restoreAllMocks(); toast.dismiss() })

const notice = (category: "failures" | "changes" | "completions" | "approvals") =>
  ({ category, key: `${category}:one`, title: `Couldn’t delete e2e (${category})`, body: "", computer: null })

it.each(["failures", "changes"] as const)("closes a %s notice by itself", (category) => {
  const error = vi.spyOn(toast, "error")
  const warning = vi.spyOn(toast, "warning")
  showBackendNotice(notice(category))
  const [, options] = (category === "failures" ? error : warning).mock.calls[0]!
  expect(options).toMatchObject({ duration: BACKEND_FAILURE_TOAST_MS, closeButton: true })
})

it("keeps a request from an agent until it is answered", () => {
  const warning = vi.spyOn(toast, "warning")
  showBackendNotice(notice("approvals"), () => {})
  expect(warning.mock.calls[0]![1]).toMatchObject({ duration: Infinity })
})
