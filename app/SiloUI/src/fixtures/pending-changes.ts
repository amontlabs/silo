import { requestListSchema, type RequestList } from "@/desktop/production-schemas"

/** Review requests across this device and a connected one, plus a device that cannot be reached. Deterministic. */
export const fixtureReviewRequests: RequestList = requestListSchema.parse({
  requests: [
    {
      request: {
        id: "req-login-fix", forkId: "fork-login-fix", target: "main", createdAt: "2026-09-03T09:12:00Z",
        summary: "Fix the login redirect", status: "open", counts: { added: 2, modified: 5, removed: 1 },
      },
      forkName: "login-fix", forkState: "running", targetName: "main",
    },
    {
      request: {
        id: "req-docs-pass", forkId: "fork-docs-pass", target: "main", createdAt: "2026-09-02T16:40:00Z",
        summary: "Rewrite the setup guide", status: "keeping", counts: { modified: 3 },
      },
      forkName: "docs-pass", forkState: "stopped", targetName: "main",
    },
    {
      request: {
        id: "req-cache-tune", forkId: "fork-cache-tune", target: "main", createdAt: "2026-09-01T11:05:00Z",
        summary: "", status: "open", counts: {},
      },
      forkName: "cache-tune", forkState: "stopped", targetName: "main", deviceId: "mac-mini", deviceName: "Mac mini",
    },
  ],
  unreachable: [{ deviceId: "office", deviceName: "Office", reason: "The device did not answer. Check that it is on and connected." }],
})

/** The fixture source for `listReviewRequests`: the same list for every call. */
export async function fixtureListReviewRequests(): Promise<RequestList> {
  return fixtureReviewRequests
}
