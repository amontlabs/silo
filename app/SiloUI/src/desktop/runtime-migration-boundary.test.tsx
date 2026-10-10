import { act, fireEvent, render, screen } from "@testing-library/react"
import { expect, it, vi } from "vitest"
import { RuntimeMigrationBoundary, type RuntimeMigrationBackend, type RuntimeMigrationState } from "./runtime-migration-boundary"

const failed: RuntimeMigrationState = {
  status: "failed", stage: "Converting dev disks", logs: ["Copying dev", "Conversion failed"],
  migratedCount: 1, failedCount: 1, totalCount: 2, canContinue: true,
  logPath: "/tmp/silo-migration.log", error: "dev could not be converted",
}

it("keeps the application gated through failure and requires an explicit acknowledged continue", async () => {
  let refresh: (() => void) | undefined
  const read = vi.fn().mockResolvedValue(failed)
  const retry = vi.fn().mockResolvedValue({ ...failed, status: "running", error: undefined })
  const continueAfterFailure = vi.fn().mockResolvedValue({ ...failed, status: "complete" })
  const backend: RuntimeMigrationBackend = { read, retry, continueAfterFailure, subscribe: async handler => { refresh = handler; return () => {} } }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
  await screen.findByText("Some computers could not be migrated")
  expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Continue with available computers" })).toBeDisabled()
  fireEvent.click(screen.getByRole("button", { name: "Show logs" }))
  expect(screen.getByText(/Conversion failed/)).toBeVisible()
  expect(screen.getByRole("link", { name: "Prepare GitHub issue" })).toHaveAttribute("href", expect.stringContaining("issues/new"))
  fireEvent.click(screen.getByRole("checkbox"))
  fireEvent.click(screen.getByRole("button", { name: "Continue with available computers" }))
  await screen.findByText("Normal application")
  expect(continueAfterFailure).toHaveBeenCalledOnce()
  await act(async () => { refresh?.() })
})

it("does not show the application when migration status cannot be read", async () => {
  const backend: RuntimeMigrationBackend = {
    read: () => Promise.reject(new Error("migration gate unavailable")),
    retry: vi.fn(), continueAfterFailure: vi.fn(), subscribe: async () => () => {},
  }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  expect(await screen.findByRole("alert")).toHaveTextContent("migration gate unavailable")
  expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
})

it("offers Retry that reads migration status again after a failed read", async () => {
  const read = vi.fn()
    .mockRejectedValueOnce(new Error("migration gate unavailable"))
    .mockResolvedValue({ ...failed, status: "not-required", error: undefined })
  const backend: RuntimeMigrationBackend = { read, retry: vi.fn(), continueAfterFailure: vi.fn(), subscribe: async () => () => {} }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  expect(await screen.findByRole("alert")).toHaveTextContent("migration gate unavailable")
  fireEvent.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByText("Normal application")).toBeVisible()
  expect(read).toHaveBeenCalledTimes(2)
})

it("subscribes again on Retry when the first subscription failed", async () => {
  const stop = vi.fn()
  const subscribe = vi.fn()
    .mockRejectedValueOnce(new Error("event bridge unavailable"))
    .mockResolvedValue(stop)
  const read = vi.fn().mockResolvedValue({ ...failed, status: "not-required", error: undefined })
  const backend: RuntimeMigrationBackend = { read, retry: vi.fn(), continueAfterFailure: vi.fn(), subscribe }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  expect(await screen.findByRole("alert")).toHaveTextContent("event bridge unavailable")
  expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
  fireEvent.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByText("Normal application")).toBeVisible()
  expect(subscribe).toHaveBeenCalledTimes(2)
})

it("does not announce a migration before the first status arrives", async () => {
  let resolve!: (state: RuntimeMigrationState) => void
  const read = vi.fn(() => new Promise<RuntimeMigrationState>(done => { resolve = done }))
  const backend: RuntimeMigrationBackend = { read, retry: vi.fn(), continueAfterFailure: vi.fn(), subscribe: async () => () => {} }
  await act(async () => { render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>) })
  expect(read).toHaveBeenCalled()
  expect(screen.queryByText("Updating your computers")).not.toBeInTheDocument()
  expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
  await act(async () => resolve({ ...failed, status: "not-required", error: undefined }))
  expect(screen.getByText("Normal application")).toBeVisible()
})

it("shows migration progress once the first status reports it", async () => {
  const backend: RuntimeMigrationBackend = {
    read: vi.fn().mockResolvedValue({ ...failed, status: "running", error: undefined }),
    retry: vi.fn(), continueAfterFailure: vi.fn(), subscribe: async () => () => {},
  }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  expect(await screen.findByText("Updating your computers")).toBeVisible()
})

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(done => { resolve = done })
  return { promise, resolve }
}

it.each(["complete", "failed"] as const)("preserves event-%s when an older Retry response arrives", async status => {
  let refresh!: () => void
  let current = failed
  const response = deferred<RuntimeMigrationState>()
  const read = vi.fn(async () => current)
  const backend: RuntimeMigrationBackend = {
    read, retry: () => response.promise, continueAfterFailure: vi.fn(),
    subscribe: async handler => { refresh = handler; return () => {} },
  }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  await screen.findByText("Some computers could not be migrated")
  fireEvent.click(screen.getByRole("button", { name: "Retry migration" }))
  current = { ...failed, status, stage: "Attempt finished", error: status === "failed" ? "Retry conversion failed" : undefined }
  await act(async () => refresh())
  await act(async () => response.resolve({ ...failed, status: "running", error: undefined }))
  expect(screen.queryByText("Updating your computers")).not.toBeInTheDocument()
  if (status === "complete") expect(screen.getByText("Normal application")).toBeVisible()
  else {
    expect(screen.getByRole("alert")).toHaveTextContent("Retry conversion failed")
    expect(screen.getByRole("button", { name: "Retry migration" })).toBeEnabled()
  }
})

it("keeps the latest event read when two status reads resolve in reverse order", async () => {
  let refresh!: () => void
  const older = deferred<RuntimeMigrationState>()
  const newer = deferred<RuntimeMigrationState>()
  const read = vi.fn().mockResolvedValueOnce(failed).mockResolvedValueOnce(failed).mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise)
  const backend: RuntimeMigrationBackend = {
    read, retry: vi.fn(), continueAfterFailure: vi.fn(),
    subscribe: async handler => { refresh = handler; return () => {} },
  }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  await screen.findByText("Some computers could not be migrated")
  await act(async () => { refresh(); refresh() })
  await act(async () => newer.resolve({ ...failed, status: "complete", error: undefined }))
  expect(screen.getByText("Normal application")).toBeVisible()
  await act(async () => older.resolve({ ...failed, status: "running", error: undefined }))
  expect(screen.getByText("Normal application")).toBeVisible()
})

it("reads authoritative status after Retry even without a completion event", async () => {
  const read = vi.fn().mockResolvedValueOnce(failed).mockResolvedValueOnce(failed).mockResolvedValue({ ...failed, status: "complete", error: undefined })
  const backend: RuntimeMigrationBackend = {
    read, retry: vi.fn().mockResolvedValue({ ...failed, status: "running", error: undefined }),
    continueAfterFailure: vi.fn(), subscribe: async () => () => {},
  }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  await screen.findByText("Some computers could not be migrated")
  fireEvent.click(screen.getByRole("button", { name: "Retry migration" }))
  expect(await screen.findByText("Normal application")).toBeVisible()
  // The first status, the read that closes the gap before the subscription, and the read after Retry.
  expect(read).toHaveBeenCalledTimes(3)
})

it("reads once without waiting for the subscription and stops listening when no migration is needed", async () => {
  const stop = vi.fn()
  let register!: (stop: () => void) => void
  const subscribe = vi.fn(() => new Promise<() => void>(resolve => { register = resolve }))
  const read = vi.fn().mockResolvedValue({ ...failed, status: "not-required", error: undefined })
  const backend: RuntimeMigrationBackend = { read, retry: vi.fn(), continueAfterFailure: vi.fn(), subscribe }
  await act(async () => { render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>) })
  expect(read).toHaveBeenCalledTimes(1)
  expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
  await act(async () => register(stop))
  expect(await screen.findByText("Normal application")).toBeVisible()
  expect(stop).toHaveBeenCalledOnce()
  expect(read).toHaveBeenCalledTimes(1)
})

it("stops listening once a running migration completes, and reads again after the subscription for a status that may predate it", async () => {
  const stop = vi.fn()
  let refresh!: () => void
  let current: RuntimeMigrationState = { ...failed, status: "running", error: undefined }
  const read = vi.fn(async () => current)
  const backend: RuntimeMigrationBackend = { read, retry: vi.fn(), continueAfterFailure: vi.fn(), subscribe: async handler => { refresh = handler; return stop } }
  render(<RuntimeMigrationBoundary backend={backend}><p>Normal application</p></RuntimeMigrationBoundary>)
  expect(await screen.findByText("Updating your computers")).toBeVisible()
  await vi.waitFor(() => expect(read).toHaveBeenCalledTimes(2))
  expect(stop).not.toHaveBeenCalled()
  current = { ...failed, status: "not-required", error: undefined }
  await act(async () => refresh())
  expect(await screen.findByText("Normal application")).toBeVisible()
  expect(stop).toHaveBeenCalledOnce()
})
