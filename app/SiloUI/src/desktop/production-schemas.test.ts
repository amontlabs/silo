import { describe, expect, it } from "vitest"

import { checkpointShape } from "./production-schemas"

const base = { id: "c1", name: "n", createdAt: "2026-01-01T00:00:00.000Z", scope: "disk", reason: "manual" }

describe("checkpoint kinds", () => {
  it.each(["manual", "automatic", "before-keep", "fork-point", "warm", "step"])("accepts %s", (kind) => {
    expect(checkpointShape.parse({ ...base, kind }).kind).toBe(kind)
  })

  it("rejects an unknown kind", () => {
    expect(() => checkpointShape.parse({ ...base, kind: "other" })).toThrow()
  })
})
