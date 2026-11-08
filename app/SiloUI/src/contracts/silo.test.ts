import { computerEditorDraftSchema } from "@/features/onboarding/model/onboarding-draft"
import { describe, expect, it } from "vitest"

import {
  githubComputerPolicySchema,
  setupComputerConfigurationRequestSchema,
  setupComputerConfigurationSchema,
  siloProgressEventSchema,
} from "@/contracts/silo"
import { onboardingSourceSchema } from "@/features/onboarding/model/onboarding-source"
import { fixtureComputerDefaults } from "@/fixtures/computer-configurations"
import { onboardingScenarios, scenarioNames } from "@/fixtures/scenarios"

describe("Silo contract fixtures", () => {
  it("parses every scenario through strict runtime contracts", () => {
    for (const name of scenarioNames) {
      expect(onboardingSourceSchema.parse(onboardingScenarios[name])).toEqual(onboardingScenarios[name])
    }
  })

  it("accepts a container runtime and rejects any other", () => {
    const computer = fixtureComputerDefaults[0]
    expect(setupComputerConfigurationSchema.parse({ ...computer, runtime: "container" }).runtime).toBe("container")
    expect(setupComputerConfigurationSchema.parse(computer).runtime).toBeUndefined()
    expect(() => setupComputerConfigurationSchema.parse({ ...computer, runtime: "docker" })).toThrow()
  })

  it("accepts an explicit optional host Git identity without reading local configuration", () => {
    expect(onboardingSourceSchema.parse({
      ...onboardingScenarios.running,
      currentDeviceGitIdentity: null,
    }).currentDeviceGitIdentity).toBeNull()
    expect(onboardingScenarios.running.currentDeviceGitIdentity).toEqual({
      name: "Taylor Example",
      email: "taylor@example.com",
    })
  })

  it("keeps progress events on the exact schema-version 1 contract", () => {
    const event = onboardingScenarios.running.progressEvents.at(-1)
    expect(event).toMatchObject({
      schemaVersion: 1,
      type: "progress",
      requestId: "setup-bootstrap-20260903",
      phase: "verification",
      step: "computer-verification",
      computer: "personal",
      fraction: 0,
      message: "Verifying 'personal'.",
      safeForDisplay: true,
    })
    expect(event?.revision).toMatch(/^[0-9a-f]{64}$/)
    expect(onboardingScenarios.running.bootstrapState).toMatchObject({
      phase: "computers",
      startedAt: expect.any(Number),
      updatedAt: expect.any(Number),
      phaseDurations: expect.any(Object),
    })
  })

  it("rejects unknown keys and a numeric progress revision", () => {
    const valid = onboardingScenarios.running.progressEvents.at(-1)
    expect(() => siloProgressEventSchema.parse({ ...valid, inventedCaption: "no" })).toThrow()
    expect(() => siloProgressEventSchema.parse({ ...valid, revision: 1 })).toThrow()
  })

  it("enforces the native computer and repository invariants", () => {
    expect(() => setupComputerConfigurationSchema.parse({
      ...onboardingScenarios.complete.bootstrapState.computerConfigurations?.[0],
      cpus: 12,
      maxCPUs: 4,
    })).toThrow("cpus must not exceed maxCPUs")

    expect(() => githubComputerPolicySchema.parse({
      ...onboardingScenarios.running.githubPolicies[0],
      repositories: [{
        ...onboardingScenarios.running.githubPolicies[0].repositories[0],
        computer: "personal",
      }],
    })).toThrow("Repository computers must match the computer access policy.")
  })

  it("keeps the computer request ordered and strict", () => {
    const request = setupComputerConfigurationRequestSchema.parse({
      schemaVersion: 1,
      computers: [fixtureComputerDefaults[1], fixtureComputerDefaults[0]],
    })
    expect(request.computers.map(({ name }) => name)).toEqual(["playgrounds", "dev"])
    expect(() => setupComputerConfigurationRequestSchema.parse({
      ...request,
      computers: [{ ...request.computers[1], ignoredCredential: "secret" }],
    })).toThrow()
    expect(() => setupComputerConfigurationRequestSchema.parse({
      ...request,
      computers: [request.computers[0], { ...request.computers[1], name: "PLAYGROUNDS" }],
    })).toThrow()
    expect(() => setupComputerConfigurationRequestSchema.parse({
      ...request,
      computers: [request.computers[0], { ...request.computers[1], id: request.computers[0].id }],
    })).toThrow("Computer IDs must be unique.")
  })
})

describe("custom computer memory", () => {
  it.each([1, 2, 4, 8, 12, 24, 64])("accepts %i GiB through the saved configuration contract", (memory) => {
    const configuration = { ...fixtureComputerDefaults[0], memoryGiB: memory, maxMemoryGiB: memory }
    expect(setupComputerConfigurationRequestSchema.safeParse({ schemaVersion: 1, computers: [configuration] }).success).toBe(true)
  })
  it.each([0, -1, 1.5, 4294967296])("rejects invalid memory %s", (memory) => {
    const configuration = { ...fixtureComputerDefaults[0], memoryGiB: memory, maxMemoryGiB: memory }
    expect(setupComputerConfigurationRequestSchema.safeParse({ schemaVersion: 1, computers: [configuration] }).success).toBe(false)
  })
})

it("preserves temporarily invalid custom memory only in an unfinished editor", () => {
  const draft = { ...fixtureComputerDefaults[0], memoryGiB: 0, maxMemoryGiB: 12 }
  expect(computerEditorDraftSchema.safeParse({ draft, insertAt: 0 }).success).toBe(true)
  expect(setupComputerConfigurationRequestSchema.safeParse({ schemaVersion: 1, computers: [draft] }).success).toBe(false)
})

it("accepts the native CPU maximum and keeps larger counts only in unfinished input", () => {
  const configuration = { ...fixtureComputerDefaults[0], cpus: 255, maxCPUs: 255 }
  expect(setupComputerConfigurationRequestSchema.safeParse({ schemaVersion: 1, computers: [configuration] }).success).toBe(true)
  const unfinished = { ...configuration, cpus: 256, maxCPUs: 256 }
  expect(computerEditorDraftSchema.safeParse({ draft: unfinished, insertAt: 0 }).success).toBe(true)
  expect(setupComputerConfigurationRequestSchema.safeParse({ schemaVersion: 1, computers: [unfinished] }).success).toBe(false)
})

it("accepts custom CPUs and disks and rejects storage overflow", () => {
  const configuration = { ...fixtureComputerDefaults[0], cpus: 3, maxCPUs: 5, memoryGiB: 12, maxMemoryGiB: 12, workspaceStorageGiB: 35, runtimeStorageGiB: 25 }
  const parse = (changes: object) => setupComputerConfigurationRequestSchema.safeParse({ schemaVersion: 1, computers: [{ ...configuration, ...changes }] }).success
  expect(parse({})).toBe(true)
  for (const changes of [{ cpus: 0 }, { cpus: 1.5 }, { cpus: 6 }, { maxCPUs: 4294967296 }, { workspaceStorageGiB: 0 }, { runtimeStorageGiB: 1.5 }, { workspaceStorageGiB: 4194300, runtimeStorageGiB: 4 }]) {
    expect(parse(changes)).toBe(false)
  }
})
