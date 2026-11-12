import type { KeepRequestResult, OpenRequestResult, RequestDetail, ThrowAwayResult } from "@/desktop/production-schemas"
import type { ReviewRequestActions } from "@/features/application/components/review-request"

const request = (id: string, forkId: string) => ({
  id, forkId, target: "main", createdAt: "2026-11-05T09:30:00Z", reviewDigest: "d1", summary: "", status: "open" as const, counts: {},
  keepId: undefined, resolvedAt: undefined, keepingSinceMs: undefined, closedReason: undefined, approvalId: undefined,
})

const text = [
  "Code",
  "  ~/app: 46cc034..e710c1a",
  "Packages",
  "  apt: ripgrep, jq (+3 dependencies)",
  "Settings & files",
  "  /etc/app.conf: added",
  "Not kept",
  "  listening tcp 3001 (node)",
].join("\n")

const base = (id: string, forkName: string): RequestDetail => ({
  request: request(id, `${forkName}-id`), forkName, forkState: "running", targetName: undefined, stale: false, reviewDigest: "d1", fresh: true,
  diff: null, text, counts: { code: 1, packages: 1, settings: 1 }, credentials: { used: [], blocked: [] },
})

/** Three requests for the browser preview: one fresh, one stale, one whose fork was refused credentials. */
export const reviewRequestFixtures: Record<string, RequestDetail> = {
  "req-fresh": { ...base("req-fresh", "login-fix"), credentials: { used: [{ secret: "GITHUB_TOKEN", host: "api.github.com", method: "GET", count: 4 }], blocked: [] } },
  "req-stale": { ...base("req-stale", "billing-fix"), stale: true, fresh: false },
  "req-blocked": {
    ...base("req-blocked", "deploy-try"),
    credentials: {
      used: [{ secret: "NPM_TOKEN", host: "registry.npmjs.org", method: "GET", count: 2 }],
      blocked: [{ host: "api.stripe.com", method: "POST", count: 3, reason: "the fork was not given STRIPE_KEY" }],
    },
  },
}

/** The ids of the pending changes fixture open the matching requests: same fork names, same ids. */
Object.assign(reviewRequestFixtures, {
  "req-login-fix": { ...reviewRequestFixtures["req-fresh"], request: request("req-login-fix", "fork-login-fix") },
  "req-docs-pass": { ...base("req-docs-pass", "docs-pass"), forkState: "stopped", counts: { code: 3 }, request: { ...request("req-docs-pass", "fork-docs-pass"), status: "keeping" as const } },
  "req-cache-tune": { ...base("req-cache-tune", "cache-tune"), forkState: "stopped", text: "", counts: {} },
})

/** Actions answered from the fixtures, for the browser preview. */
export function createReviewRequestFixtureActions(overrides: Partial<ReviewRequestActions> = {}): ReviewRequestActions {
  return {
    show: async ({ requestId }) => {
      const detail = reviewRequestFixtures[requestId]
      if (!detail) throw new Error(`No request ${requestId}.`)
      return detail
    },
    keep: async ({ requestId }): Promise<KeepRequestResult> => {
      const detail = reviewRequestFixtures[requestId]
      return {
        status: "kept",
        report: { fork: detail.forkName, target: "main", summary: [], steps: [], notes: [], checkpoint: "cp-before-keep", reviewDigest: detail.reviewDigest, fastForward: undefined },
        refusal: undefined,
        request: { ...detail.request, status: "kept", keepId: "cp-before-keep" },
      }
    },
    throwAway: async ({ requestId }): Promise<ThrowAwayResult> => ({ request: reviewRequestFixtures[requestId].request, dropped: reviewRequestFixtures[requestId].forkName }),
    open: async ({ requestId }): Promise<OpenRequestResult> => ({
      request: reviewRequestFixtures[requestId].request, computerId: `${requestId}-computer`, name: reviewRequestFixtures[requestId].forkName,
      state: "running", started: false, sshAlias: `silo-${reviewRequestFixtures[requestId].forkName}`, urls: [`http://${reviewRequestFixtures[requestId].forkName}-3000.localhost`],
    }),
    ...overrides,
  }
}
