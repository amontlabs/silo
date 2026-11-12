import { afterEach, describe, expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { BackupState } from "@/features/application/model/backup-source"
import { remoteComputerTarget } from "@/features/application/model/connections"
import { assertNativeBridgeMocksHandled, nativeBridgeMock, type NativeCommandHandlers } from "@/test/native-bridge-mock"
import contracts from "@/test/contracts/review-requests.json"
import approvalContracts from "@/test/contracts/approvals.json"
import { createProductionSource, decideApproval, keepReviewRequest, listApprovals, listAudit, readApprovalJob, readDeviceResources, watchApprovals, listComputerSteps, listReviewRequests, openReviewRequest, readComputerTree, showReviewRequest, throwAwayReviewRequest, type ProductionBridge } from "./production-source"

afterEach(assertNativeBridgeMocksHandled)

const source = applicationSourceForScenario("running")
const archive = { name: "dev.silo-backup", archivePath: "/backups/dev.silo-backup", completedLabel: "Today", size: "1 GiB", destination: "/backups", computers: ["dev"] }
const backup: BackupState = { snapshotId: "1", availability: "available", archives: [archive], operation: null }

function bridge(handlers: NativeCommandHandlers = {}) {
  const invoke = nativeBridgeMock({
    read_application_state: () => structuredClone(source),
    read_backup_state: () => structuredClone(backup),
    read_setup_activity: () => [],
    read_network_state: () => ({ computers: [] }),
    remote_network_state: () => ({ computers: [] }),
    device_list: () => [],
    connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "developer@laptop" }),
    read_operation_queue: () => ({ running: [], waiting: [] }),
    ...handlers,
  })
  return { invoke, listen: vi.fn(async () => () => {}) } as unknown as ProductionBridge & { invoke: typeof invoke }
}

describe("production archive inspection", () => {
  it("rejects an already aborted inspection without starting native work", async () => {
    const native = bridge({ inspect_backup_archive: () => ({ archive, valid: true }) })
    const store = createProductionSource(native)
    const abort = new AbortController()
    abort.abort()
    try {
      await expect(store.backupActions.inspectArchive(archive, abort.signal)).rejects.toMatchObject({ name: "AbortError" })
      expect(native.invoke).not.toHaveBeenCalled()
    } finally { store.dispose() }
  })

  it("honors cancellation from the archive selection callback before inspection starts", async () => {
    const native = bridge({
      choose_backup_archive: () => archive.archivePath,
      inspect_backup_archive: () => ({ archive, valid: true }),
    })
    const store = createProductionSource(native)
    const abort = new AbortController()
    const reason = new Error("Import review closed")
    const selected = vi.fn(() => abort.abort(reason))
    try {
      await expect(store.backupActions.chooseArchive(selected, abort.signal)).rejects.toBe(reason)
      expect(selected).toHaveBeenCalledExactlyOnceWith(archive.archivePath)
      expect(native.invoke.mock.calls).toEqual([["choose_backup_archive"]])
    } finally { store.dispose() }
  })

  it("does not inspect or announce a cancelled archive picker", async () => {
    const native = bridge({ choose_backup_archive: () => null })
    const store = createProductionSource(native)
    const selected = vi.fn()
    try {
      await expect(store.backupActions.chooseArchive(selected)).resolves.toBeNull()
      expect(selected).not.toHaveBeenCalled()
      expect(native.invoke.mock.calls).toEqual([["choose_backup_archive"]])
    } finally { store.dispose() }
  })

  it("ignores a picker reply after its caller aborts", async () => {
    let choose!: (path: string) => void
    const native = bridge({ choose_backup_archive: () => new Promise<string>(resolve => { choose = resolve }) })
    const store = createProductionSource(native)
    const selected = vi.fn()
    const abort = new AbortController()
    try {
      const inspection = store.backupActions.chooseArchive(selected, abort.signal)
      abort.abort()
      choose(archive.archivePath)
      await expect(inspection).resolves.toBeNull()
      expect(selected).not.toHaveBeenCalled()
      expect(native.invoke.mock.calls).toEqual([["choose_backup_archive"]])
    } finally { store.dispose() }
  })

  it.each(["malformed", "rejected"] as const)("allows a fresh inspection after a %s reply and removes the old abort listener", async failureKind => {
    const failure = { code: "archive_unreadable", message: "Reconnect the backup drive" }
    let attempts = 0
    const native = bridge({ inspect_backup_archive: () => {
      if (++attempts === 1) {
        if (failureKind === "rejected") throw failure
        return { archive, valid: "yes" }
      }
      return { archive, valid: true }
    } })
    const store = createProductionSource(native)
    const abort = new AbortController()
    const before = store.getSnapshot()
    try {
      const first = store.backupActions.inspectArchive(archive, abort.signal)
      if (failureKind === "rejected") await expect(first).rejects.toBe(failure)
      else await expect(first).rejects.toThrow()
      abort.abort()
      await expect(store.backupActions.inspectArchive(archive)).resolves.toEqual({ archive, valid: true })
      const requests = native.invoke.mock.calls.map(([command, args]) => {
        expect(command).toBe("inspect_backup_archive")
        expect(args).toEqual({ archivePath: archive.archivePath, requestId: expect.any(String) })
        return args!.requestId
      })
      expect(requests).toHaveLength(2)
      expect(new Set(requests).size).toBe(2)
      expect(store.getSnapshot()).toBe(before)
    } finally { store.dispose() }
  })

  it("cancels only the requested inspection and detaches the completed inspection's signal", async () => {
    const finishes: Array<(result: unknown) => void> = []
    const native = bridge({
      inspect_backup_archive: () => new Promise(resolve => { finishes.push(resolve) }),
      cancel_backup_inspection: () => undefined,
    })
    const store = createProductionSource(native)
    const firstAbort = new AbortController()
    const secondAbort = new AbortController()
    const secondArchive = { ...archive, archivePath: "/backups/api.silo-backup" }
    try {
      const first = store.backupActions.inspectArchive(archive, firstAbort.signal)
      const second = store.backupActions.inspectArchive(secondArchive, secondAbort.signal)
      const requests = native.invoke.mock.calls.map(([, args]) => args!.requestId)
      expect(new Set(requests).size).toBe(2)
      expect(native.invoke).toHaveBeenNthCalledWith(2, "inspect_backup_archive", { archivePath: secondArchive.archivePath, requestId: requests[1] })
      firstAbort.abort()
      expect(native.invoke).toHaveBeenLastCalledWith("cancel_backup_inspection", { requestId: requests[0] })
      finishes[0]({ archive, valid: false, reason: "Cancelled" })
      finishes[1]({ archive: secondArchive, valid: true })
      await expect(first).resolves.toMatchObject({ valid: false })
      await expect(second).resolves.toEqual({ archive: secondArchive, valid: true })
      secondAbort.abort()
      expect(native.invoke.mock.calls.filter(([command]) => command === "cancel_backup_inspection")).toHaveLength(1)
    } finally { store.dispose() }
  })
})

describe("production command behavior", () => {
  it.each([
    ["authorizeDevice", "authorize_device"],
    ["setupDeviceKey", "setup_device_key"],
  ] as const)("passes the address to %s and preserves a failed repair for the caller to retry", async (action, command) => {
    const failure = { code: "ssh_authentication_failed", message: "Unlock your SSH key" }
    let attempts = 0
    const native = bridge({ [command]: () => { if (++attempts === 1) throw failure } })
    const store = createProductionSource(native)
    try {
      await expect(store.applicationActions[action]!("owner@office")).rejects.toBe(failure)
      await store.applicationActions[action]!("owner@office")
      expect(native.invoke.mock.calls).toEqual([[command, { address: "owner@office" }], [command, { address: "owner@office" }]])
      expect(store.getSnapshot().error).toBeNull()
    } finally { store.dispose() }
  })

  it.each([false, true])("connects a device with explicit address replacement=%s and publishes its authoritative list", async replaceAddress => {
    const device = { id: "office", name: "Office", address: "owner@office" }
    let connected = false
    const native = bridge({
      connect_device: () => { connected = true; return device },
      device_list: () => connected ? [device] : [],
      device_snapshot: () => structuredClone(source),
    })
    const store = createProductionSource(native)
    try {
      await store.initialize()
      await store.applicationActions.connectDevice!(device.address, replaceAddress ? { replaceAddress } : undefined)
      expect(native.invoke).toHaveBeenCalledWith("connect_device", { address: device.address, replace: replaceAddress })
      await vi.waitFor(() => expect(store.getSnapshot().source?.devices).toContainEqual(expect.objectContaining({ id: "office", connected: true })))
      expect(store.getSnapshot().source?.computers.some(computer => computer.device?.id === "office")).toBe(true)
    } finally { store.dispose() }
  })

  it.each(["save_secret", "remove_secret", "retry_secret"])("rejects a malformed %s reply without replacing cached secrets", async command => {
    const request = { operation: "edit" as const, id: "package-token", name: "PACKAGE_TOKEN", computers: ["dev"], allowedDomains: ["registry.npmjs.org"] }
    const native = bridge({ [command]: () => ({ secrets: "not a secret list" }) })
    const store = createProductionSource(native)
    try {
      await store.initialize()
      const saved = store.getSnapshot().source!.secrets
      const action = command === "save_secret" ? store.applicationActions.saveSecret(request)
        : command === "remove_secret" ? store.applicationActions.removeSecret(request.id) : store.applicationActions.retrySecret!(request.id)
      await expect(action).rejects.toThrow()
      expect(native.invoke).toHaveBeenCalledWith(command, command === "save_secret" ? { request } : { id: request.id })
      expect(store.getSnapshot().source!.secrets).toEqual(saved)
    } finally { store.dispose() }
  })

  it("does not resurrect removed secrets when an earlier save reply arrives late", async () => {
    let finishSave!: (secrets: typeof source.secrets) => void
    let finishRead!: (value: typeof source) => void
    const pendingRead = new Promise<typeof source>(resolve => { finishRead = resolve })
    let holdReads = false
    const native = bridge({
      read_application_state: () => holdReads ? pendingRead : structuredClone(source),
      save_secret: () => new Promise<typeof source.secrets>(resolve => { finishSave = resolve }),
      remove_secret: () => [],
    })
    const store = createProductionSource(native)
    try {
      await store.initialize()
      holdReads = true
      const save = store.applicationActions.saveSecret({ operation: "edit", id: source.secrets[0].id, name: source.secrets[0].name, computers: ["dev"], allowedDomains: ["api.example.test"] })
      await store.applicationActions.removeSecret(source.secrets[0].id)
      expect(store.getSnapshot().source!.secrets).toEqual([])
      const published: Array<typeof source.secrets> = []
      const stop = store.subscribe(() => { published.push(store.getSnapshot().source!.secrets) })
      finishSave(source.secrets)
      await save
      expect(store.getSnapshot().source!.secrets).toEqual([])
      expect(published.every(secrets => secrets.length === 0)).toBe(true)
      stop()
    } finally { finishRead({ ...source, secrets: [] }); store.dispose() }
  })

  it("accepts an earlier successful secret save when a later removal was rejected", async () => {
    let finishSave!: (secrets: typeof source.secrets) => void
    let finishRead!: (value: typeof source) => void
    const pendingRead = new Promise<typeof source>(resolve => { finishRead = resolve })
    let holdReads = false
    const failure = { code: "credential_store_locked", message: "Unlock credential storage" }
    const updated = source.secrets.map(secret => ({ ...secret, allowedDomains: ["api.example.test"] }))
    const native = bridge({
      read_application_state: () => holdReads ? pendingRead : structuredClone(source),
      save_secret: () => new Promise<typeof source.secrets>(resolve => { finishSave = resolve }),
      remove_secret: () => { throw failure },
    })
    const store = createProductionSource(native)
    try {
      await store.initialize()
      holdReads = true
      const save = store.applicationActions.saveSecret({ operation: "edit", id: source.secrets[0].id, name: source.secrets[0].name, computers: ["dev"], allowedDomains: ["api.example.test"] })
      await expect(store.applicationActions.removeSecret(source.secrets[0].id)).rejects.toBe(failure)
      finishSave(updated)
      await save
      expect(store.getSnapshot().source!.secrets).toEqual(updated)
    } finally { finishRead({ ...source, secrets: updated }); store.dispose() }
  })

  it("does not start refreshes when a secret mutation completes after disposal", async () => {
    let finishSave!: (secrets: typeof source.secrets) => void
    const native = bridge({ save_secret: () => new Promise<typeof source.secrets>(resolve => { finishSave = resolve }) })
    const store = createProductionSource(native)
    await store.initialize()
    native.invoke.mockClear()
    const save = store.applicationActions.saveSecret({ operation: "edit", id: source.secrets[0].id, name: source.secrets[0].name, computers: ["dev"], allowedDomains: ["api.example.test"] })
    store.dispose()
    finishSave(source.secrets)
    await save
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual(["save_secret"])
  })

  it("preserves a device and its rows when native connection removal is rejected", async () => {
    const failure = { code: "settings_write_failed", message: "Connection not saved" }
    const native = bridge({
      device_list: () => [{ id: "office", name: "Office", address: "owner@office" }],
      device_snapshot: () => structuredClone(source),
      remove_device: () => { throw failure },
    })
    const store = createProductionSource(native)
    try {
      await store.initialize()
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers.some(computer => computer.device?.id === "office")).toBe(true))
      await expect(store.applicationActions.removeDevice!("office")).rejects.toBe(failure)
      expect(native.invoke).toHaveBeenCalledWith("remove_device", { deviceId: "office" })
      expect(store.getSnapshot().source?.devices?.map(device => device.id)).toEqual(["office"])
      expect(store.getSnapshot().source?.computers.some(computer => computer.device?.id === "office")).toBe(true)
    } finally { store.dispose() }
  })

  it("dismisses only the matching remote completed push and retains other results and running pushes", async () => {
    const remote = structuredClone(source)
    const completed = { computer: "dev", repositoryPath: "/workspace/repo", commitCount: 1, status: "succeeded" as const }
    remote.repositoryPushOperations = [completed, { ...completed, repositoryPath: "/workspace/other" }, { ...completed, status: "pushing" }]
    let reads = 0
    let holdRefresh!: () => void
    const refreshed = new Promise<void>(resolve => { holdRefresh = resolve })
    const native = bridge({
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: async () => { if (reads++ > 0) await refreshed; return structuredClone(remote) },
      dismiss_repository_push: () => undefined,
    })
    const store = createProductionSource(native)
    const target = remoteComputerTarget("office", remote.computers[0].configuration.id)
    try {
      await store.initialize()
      await vi.waitFor(() => expect(store.getSnapshot().source?.repositoryPushOperations.filter(operation => operation.computer === target)).toHaveLength(3))
      store.statusActions.dismissRepositoryPush(target, "/workspace/repo")
      await vi.waitFor(() => expect(store.getSnapshot().source?.repositoryPushOperations.filter(operation => operation.computer === target)).toEqual([
        expect.objectContaining({ repositoryPath: "/workspace/other", status: "succeeded" }),
        expect.objectContaining({ repositoryPath: "/workspace/repo", status: "pushing" }),
      ]))
      expect(native.invoke).toHaveBeenCalledWith("dismiss_repository_push", { computer: target, repositoryPath: "/workspace/repo" })
      // The owner's cache must stay dismissed even when local state refreshes.
      await store.refresh()
      expect(store.getSnapshot().source?.repositoryPushOperations.filter(operation => operation.computer === target)).toHaveLength(2)
    } finally { holdRefresh(); store.dispose() }
  })

  it("stops GitHub verification at 300 seconds and leaves setup incomplete", async () => {
    vi.useFakeTimers()
    const configurations = source.computers.filter(({ device }) => !device).map(({ configuration }) => configuration)
    const github = { ...source.github, policyRevision: 3, computerOperations: configurations.map(({ name }) => ({ computer: name, status: "applying", message: "Applying access" })) }
    const native = bridge({
      configure_computer_identities: () => undefined,
      verify_computer_identities: () => false,
      save_github_configuration: () => structuredClone(github),
      read_github_state: () => structuredClone(github),
    })
    const store = createProductionSource(native)
    try {
      await store.initialize()
      const complete = vi.fn(async () => {})
      const finished = store.finishSetup({
        computerConfiguration: { schemaVersion: 1, computers: configurations }, applications: source.preferences,
        github: { connectionState: "connected", computers: configurations.map(configuration => ({ computer: configuration.name, repositories: [], identity: { name: "Test", email: "test@example.invalid", apply: true } })) },
      }, complete)
      const failure = vi.fn()
      void finished.catch(failure)
      await vi.advanceTimersByTimeAsync(0)
      expect(native.invoke).toHaveBeenCalledWith("save_github_configuration", expect.anything())
      await vi.advanceTimersByTimeAsync(299_999)
      expect(failure).not.toHaveBeenCalled()
      expect(complete).not.toHaveBeenCalled()
      await vi.advanceTimersByTimeAsync(1)
      expect(failure).toHaveBeenCalledWith(new Error("GitHub access has not been verified in every computer. Retry to check again."))
      expect(complete).not.toHaveBeenCalled()
      const polls = native.invoke.mock.calls.filter(([command]) => command === "read_github_state").length
      await vi.advanceTimersByTimeAsync(10_000)
      expect(native.invoke.mock.calls.filter(([command]) => command === "read_github_state")).toHaveLength(polls)
    } finally { store.dispose(); vi.useRealTimers() }
  })

  it("refreshes transfer state after cancelling and reveals the selected archive path", async () => {
    let cancelled = false
    const native = bridge({
      cancel_backup_operation: () => { cancelled = true },
      read_backup_state: () => ({ ...backup, snapshotId: cancelled ? "cancelled" : "1" }),
      reveal_backup_archive: () => undefined,
    })
    const store = createProductionSource(native)
    try {
      await store.initialize()
      store.backupActions.cancelOperation()
      await vi.waitFor(() => expect(store.getSnapshot().backup.snapshotId).toBe("cancelled"))
      expect(native.invoke).toHaveBeenCalledWith("cancel_backup_operation")
      await store.backupActions.revealArchive(archive)
      expect(native.invoke).toHaveBeenCalledWith("reveal_backup_archive", { archivePath: archive.archivePath })
    } finally { store.dispose() }
  })

  it("reports failed cancellation through the integration error command and preserves archive reveal failures", async () => {
    const native = bridge({
      cancel_backup_operation: () => { throw new Error("Bridge closed") },
      reveal_backup_archive: () => { throw new Error("Archive missing") },
      show_integration_error: () => undefined,
    })
    const store = createProductionSource(native)
    try {
      await store.initialize()
      store.backupActions.cancelOperation()
      await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("show_integration_error", {
        message: "Export or import cancellation failed: Bridge closed The operation may still be running.",
      }))
      await expect(store.backupActions.revealArchive(archive)).rejects.toThrow("Archive missing")
    } finally { store.dispose() }
  })
})

describe("review request commands", () => {
  const fixtures = contracts as Record<string, unknown>

  it("lists, shows, keeps, throws away and opens requests through their commands", async () => {
    const native = bridge({
      list_review_requests: () => fixtures.list,
      show_review_request: () => fixtures.show,
      keep_review_request: () => fixtures.keep,
      throw_away_review_request: () => fixtures.throwAway,
      open_review_request: () => fixtures.open,
    })
    const list = await listReviewRequests({ status: "all", allDevices: true }, native)
    expect(list.requests.map(entry => [entry.request.id, entry.request.status, entry.deviceId])).toEqual([["req-1", "open", "dev-2"], ["req-2", "thrown-away", undefined]])
    expect(list.unreachable).toEqual([{ deviceId: "dev-3", deviceName: "mac-mini", reason: "offline" }])
    const detail = await showReviewRequest({ requestId: "req-1", deviceId: "dev-2" }, native)
    expect(detail).toMatchObject({ stale: false, reviewDigest: "d1", counts: { code: 1 } })
    expect(detail.credentials.used[0]).toMatchObject({ secret: "GITHUB_TOKEN", count: 4 })
    expect((await keepReviewRequest({ requestId: "req-1", dataOk: true }, native)).request.keepId).toBe("cp-before-keep")
    expect((await throwAwayReviewRequest({ requestId: "req-1" }, native)).dropped).toBe("login-fix")
    expect((await openReviewRequest({ requestId: "req-1" }, native)).urls).toEqual(["http://login-fix-3000.localhost"])
    expect(native.invoke.mock.calls).toEqual([
      ["list_review_requests", { status: "all", allDevices: true }],
      ["show_review_request", { requestId: "req-1", deviceId: "dev-2" }],
      ["keep_review_request", { requestId: "req-1", dataOk: true }],
      ["throw_away_review_request", { requestId: "req-1" }],
      ["open_review_request", { requestId: "req-1" }],
    ])
  })

  it("reads steps and the tree", async () => {
    const native = bridge({
      list_computer_steps: () => fixtures.steps,
      read_computer_tree: () => fixtures.tree,
    })
    expect((await listComputerSteps("fork-1", undefined, native)).map(step => [step.number, step.exitCode, step.parentStep])).toEqual([[1, 0, undefined], [2, undefined, 1]])
    const nodes = await readComputerTree(undefined, native)
    expect(nodes[1]).toMatchObject({ parent: "main-1", from: { kind: "step", number: 2 }, pendingChanges: { requestId: "req-1" } })
  })

  it("keeps a refused keep and tolerates rows and origins it cannot read", async () => {
    const native = bridge({
      keep_review_request: () => fixtures.refusedKeep,
      read_computer_tree: () => ({ nodes: [{ id: "a", name: "a", parent: null, from: { kind: "from-the-future" }, state: "running", pendingChanges: { requestId: null, buckets: null } }, { broken: true }] }),
      list_review_requests: () => ({ requests: [{ nonsense: 1 }, ...(fixtures.list as { requests: unknown[] }).requests.slice(0, 1)] }),
    })
    const refused = await keepReviewRequest({ requestId: "req-1" }, native)
    expect(refused).toMatchObject({ status: "refused", refusal: { reason: "divergent" }, request: { status: "open" } })
    expect(await readComputerTree(undefined, native)).toMatchObject([{ id: "a", from: { kind: "root" } }])
    const list = await listReviewRequests({}, native)
    expect(list.requests).toHaveLength(1)
    expect(list.unreachable).toEqual([])
  })
})

describe("approval commands", () => {
  const fixtures = approvalContracts as Record<string, unknown>

  it("lists, answers, reads the audit log, the job and the device's resources through their commands", async () => {
    const native = bridge({
      list_approvals: () => fixtures.list,
      decide_approval: () => fixtures.decided,
      list_audit: () => fixtures.audit,
      read_device_resources: () => fixtures.resources,
      read_approval_job: () => fixtures.job,
    })
    const list = await listApprovals({ state: "pending" }, native)
    expect(list.approvals.map(({ id, kind, state }) => [id, kind, state])).toEqual([["ap-merge", "merge", "pending"], ["ap-port", "port", "pending"], ["ap-done", "github", "allowed"], ["ap-failed", "resources", "allowed"]])
    expect(list.approvals[0]).toMatchObject({ note: "The login page redirected in a loop.", reviewRequestId: "req-1", summary: "Merge login-fix into main" })
    expect(list.approvals[3]?.outcome?.error).toMatch(/3.1 GiB/)
    const decided = await decideApproval({ id: "ap-port", decision: "deny", note: "no" }, native)
    expect(decided).toMatchObject({ state: "allowed", outcome: { jobId: "job:7" } })
    const audit = await listAudit({ computerId: "main-1", callerKind: "computer", beforeId: 50, limit: 25 }, native)
    expect(audit.nextBeforeId).toBe(40)
    expect(audit.entries[1]).toMatchObject({ count: 5, lastAt: "2026-11-11T08:10:00Z", outcome: "refused", errorCode: "permission_denied" })
    expect(audit.entries[0]).toMatchObject({ approvalId: "ap-merge", outcome: "asked" })
    expect((await readDeviceResources("dev-2", native)).computers).toEqual([{ id: "main-1", name: "main", running: true, cpus: 4, memoryGib: 4, diskGib: 42, workspaceStorageGib: 30, runtimeStorageGib: 12 }])
    expect(await readApprovalJob("job:7", native)).toMatchObject({ state: "finished", status: "refused" })
    expect(native.invoke.mock.calls).toEqual([
      ["list_approvals", { state: "pending" }],
      ["decide_approval", { id: "ap-port", decision: "deny", note: "no" }],
      ["list_audit", { computerId: "main-1", callerKind: "computer", beforeId: 50, limit: 25 }],
      ["read_device_resources", { deviceId: "dev-2" }],
      ["read_approval_job", { jobId: "job:7" }],
    ])
  })

  it("reads what a newer engine adds as the safest known value and drops rows it cannot read", async () => {
    const native = bridge({
      list_approvals: () => ({ approvals: [
        { id: "ap-x", kind: "from-the-future", computerId: "c", computerName: "dev", summary: "Do a new thing", state: "waiting-for-tea", createdAt: "2026-11-11T09:00:00Z" },
        { nonsense: 1 },
      ] }),
      list_audit: () => ({ entries: [{ id: 1, at: "2026-11-11T09:00:00Z", callerKind: "robot", method: "x.y", outcome: "maybe" }, { broken: true }] }),
    })
    const list = await listApprovals({}, native)
    // A state it does not know is never offered for an answer.
    expect(list.approvals).toHaveLength(1)
    expect(list.approvals[0]).toMatchObject({ kind: "from-the-future", state: "cancelled" })
    const audit = await listAudit({}, native)
    expect(audit.entries).toHaveLength(1)
    expect(audit.entries[0]).toMatchObject({ callerKind: "computer", outcome: "error", count: 1 })
  })

  it("keeps a merge's choices when keeping a review request", async () => {
    const native = bridge({ keep_review_request: () => (contracts as Record<string, unknown>).choicesNeeded })
    const result = await keepReviewRequest({ requestId: "req-1", choices: { "repo:/root/app": "branch" } }, native)
    expect(native.invoke).toHaveBeenCalledWith("keep_review_request", { requestId: "req-1", choices: { "repo:/root/app": "branch" } })
    expect(result.refusal?.conflicts.map(({ id, allowed }) => [id, allowed])).toEqual([["file:/etc/app.conf", ["fork", "main"]], ["repo:/root/app", ["branch", "main"]]])
    expect(result.request.approvalId).toBe("ap-merge")
  })

  it("listens for approval changes and stops when told", async () => {
    const stop = vi.fn()
    let emit!: () => void
    const native: ProductionBridge = { invoke: vi.fn(), listen: vi.fn(async (_event, handler) => { emit = () => handler(); return stop }) }
    const changed = vi.fn()
    const dispose = watchApprovals(changed, native)
    await Promise.resolve()
    expect(native.listen).toHaveBeenCalledWith("silo://approvals-changed", expect.any(Function))
    emit()
    expect(changed).toHaveBeenCalledOnce()
    dispose()
    expect(stop).toHaveBeenCalledOnce()
  })

  it("stops listening even when told before the listener is ready", async () => {
    const stop = vi.fn()
    let ready!: (stop: () => void) => void
    const native: ProductionBridge = { invoke: vi.fn(), listen: vi.fn(() => new Promise<() => void>(resolve => { ready = resolve })) }
    watchApprovals(() => {}, native)()
    ready(stop)
    await Promise.resolve()
    expect(stop).toHaveBeenCalledOnce()
  })
})
