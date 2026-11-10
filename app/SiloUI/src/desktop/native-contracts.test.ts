import { readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { describe, expect, it } from "vitest"

import { operationKinds, operationQueueSchema } from "@/features/application/model/operation-queue"
import {
  parseApplicationSource,
  parseNetworkState,
  parseRemoteApplicationSource,
  parseSshAccessState,
} from "./production-source"

function fixture(name: string): unknown[] {
  return JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), `../test/contracts/${name}.json`), "utf8")) as unknown[]
}

describe("Rust-emitted native wire contracts", () => {
  for (const [name, parse] of [
    ["application-state", parseApplicationSource],
    ["device-snapshot", parseRemoteApplicationSource],
  ] as const) {
    it(`preserves native computer, checkpoint, and activity data from ${name}`, () => {
      const raw = fixture(name)
      const states = raw.map(parse)
      // Check every native field, allowing presentation defaults and the intended
      // checkpoint boundary conversion. nativeId belongs to runtime storage.
      raw.forEach((value, index) => {
        const expected = structuredClone(value) as { computers: Array<{ checkpoints?: Array<Record<string, unknown>> }> }
        for (const computer of expected.computers) {
          if (!computer.checkpoints) continue
          computer.checkpoints = computer.checkpoints.map(checkpoint => {
            const publicFields = Object.fromEntries(Object.entries(checkpoint).filter(([key]) => key !== "nativeId"))
            return { ...publicFields, createdAt: new Date(Number(checkpoint.createdAt)).toISOString() }
          })
        }
        expect(states[index]).toMatchObject(expected)
      })
      expect(states.map(state => state.computers[0].state)).toEqual(["running", "starting", "stopped", "failed"])
      for (const state of states) {
        expect(state.computers).toHaveLength(1)
        const computer = state.computers[0]
        expect(computer.configuration.id).toBe("00000000-0000-4000-8000-000000000001")
        expect(computer.checkpoints).toEqual([expect.objectContaining({
          id: "point-1", name: "Before change", createdAt: "2026-01-01T00:00:00.000Z", scope: "full", reason: "manual",
        })])
        expect(computer.checkpointOperation).toEqual({
          kind: "capture", status: "failed", stage: "Saving checkpoint", error: "Checkpoint storage is unavailable.",
        })
        expect(computer.pendingCheckpointRestore).toEqual({ checkpointId: "native-1", sourceComputer: "dev", state: "full" })
        expect(computer.unfinishedRestore).toEqual({ checkpointId: "point-1", checkpointName: "Before change", phase: "secured" })
        expect(computer.lifecycleFailure).toBe("Start failed: The computer could not start.")
        expect(computer.lifecycleFailureDiagnostic).toBe("Runtime startup failed.")
        expect(state.activities).toHaveLength(1)
        expect(state.activities[0]).toMatchObject({
          id: "contract-lifecycle-1", category: "computer", status: "completed", tone: "danger",
          diagnostic: "Runtime startup failed.", computer: "dev",
        })
        expect(state.runtimeRepair).toBeNull()
        expect(state.computerConfigurationOperation).toBeNull()
      }
    })
  }

  it("reads nullable GitHub authentication without discarding the application snapshot", () => {
    const github = fixture("github-authentication")[0] as { computers: Array<Record<string, unknown>> }
    const raw = fixture("application-state")[0] as Record<string, unknown>
    const state = parseApplicationSource({ ...raw, github })
    expect(state.github).toEqual({ ...github, account: undefined, computers: github.computers.map(policy => ({
      ...policy, authenticationMethod: policy.authenticationMethod ?? undefined,
    })) })
    expect(state.github.computers?.map(policy => policy.authenticationMethod)).toEqual([undefined, "oauth", "token", "token"])
    expect(state.github.computers?.map(policy => policy.tokenSecretId)).toEqual([undefined, undefined, undefined, "00000000-0000-4000-8000-0000000000aa"])
    expect(state.github.computers?.map(policy => policy.computer)).toEqual(["dev-0", "dev-1", "dev-2", "dev-3"])
    expect(state.computers).toHaveLength(1)
  })

  it("preserves each native SSH listener state and nullable fields", () => {
    const raw = fixture("ssh-access-state") as Array<{ computers: Array<Record<string, unknown>> }>
    const states = raw.map(parseSshAccessState)
    // computerId is native listener ownership; every UI field must survive parsing.
    expect(states).toEqual(raw.map(state => ({ computers: state.computers.map(computer =>
      Object.fromEntries(Object.entries(computer).filter(([key]) => key !== "computerId")),
    ) })))
    expect(states.map(state => state.computers[0].state)).toEqual(["disabled", "waiting", "listening", "error"])
    expect(states.map(state => state.computers[0].enabled)).toEqual([false, true, true, true])
    expect(states.map(state => state.computers[0].fingerprint)).toEqual([null, null, "SHA256:contract-public-fingerprint", null])
    expect(states.map(state => state.computers[0].message)).toEqual([null, null, null, "SSH listener could not start."])
    for (const state of states) {
      expect(state.computers[0]).toMatchObject({ computer: "dev", port: 2222, bindAddress: "127.0.0.1", deviceName: "Contract device", addresses: ["192.0.2.10"], user: "silo" })
    }
  })

  it("preserves every native network port state, configuration, and failure", () => {
    const raw = fixture("network-state")
    const states = raw.map(parseNetworkState)
    expect(states).toEqual(raw)
    expect(states.flatMap(state => state.computers.flatMap(computer => computer.ports.map(port => port.state))))
      .toEqual(["reachable", "waiting", "unpublished", "unknown"])
    expect(states[0].computers[0].host).toBe("dev-00000000.localhost")
    expect(states[1].computers[0].error).toBe("Network state could not be read.")
  })

  it("preserves queue scopes, cancellation, hidden blockers, and every native operation kind", () => {
    const raw = fixture("operation-queue")
    const queues = raw.map(value => operationQueueSchema.parse(value))
    expect(queues).toEqual(raw)
    expect(queues[0]).toEqual({ running: [], waiting: [] })
    expect(queues[1].running.map(operation => operation.kind)).toEqual(operationKinds)
    expect(queues[1].running.find(operation => operation.kind === "shutdown")).toMatchObject({ computerId: null, computerName: null })
    expect(queues[1].waiting[0]).toMatchObject({ cancellable: true, expectedMs: null, blockedByHidden: true })
  })
})
