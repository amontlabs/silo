import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { RuntimeMigrationBoundary } from "./runtime-migration-boundary"

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(async () => () => {}) }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: () => false }))
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }))

// Matches MigrationState's serde camelCase JSON, including its version and
// omitted Option fields, rather than passing a typed frontend state directly.
const failedJson = JSON.parse(`{"version":1,"status":"failed","stage":"Converting dev disks","logs":["Conversion failed"],"migratedCount":1,"failedCount":1,"totalCount":2,"canContinue":true,"logPath":"/tmp/silo-migration.log","error":"dev could not be converted"}`)
const completeJson = JSON.parse(`{"version":1,"status":"complete","stage":"Ready","logs":[],"migratedCount":2,"failedCount":0,"totalCount":2,"canContinue":false}`)
// Matches backup_controller::BackupState's serde camelCase JSON: nothing to report, then a result an upgrade produced.
const idleBackupState = JSON.parse(`{"snapshotId":"1","availability":"available","archives":[],"operation":null}`)
const unseenBackupState = JSON.parse(`{"snapshotId":"2","operationId":"5b0c8e3e-3b8e-4c4c-9a0b-1f0f5f2d2b77","availability":"available","archives":[],"resultUnseen":true,"operation":{"kind":"result","operation":"restore","archive":{"name":"dev.silo-backup","archivePath":"/exports/dev.silo-backup","completedLabel":"In progress","size":"Unknown","destination":"/exports","computers":["dev"]},"runningNames":[],"targetName":"copy","outcome":"failed","title":"Import interrupted before the upgrade","message":"Silo closed before this import finished.","detail":"No computer was added. Import the file again."}}`)

beforeEach(() => { native.invoke.mockReset(); native.listen.mockClear() })

function renderNativeGate() {
  return render(<RuntimeMigrationBoundary><p>Normal application</p></RuntimeMigrationBoundary>)
}

describe("native migration boundary", () => {
  it("parses backend JSON and retries migration through the native command", async () => {
    let retried = false
    native.invoke.mockImplementation(async command => {
      if (command === "read_runtime_migration_state") return retried ? completeJson : failedJson
      if (command === "retry_runtime_migration") { retried = true; return completeJson }
      // A finished migration also asks for the backup it left behind; this one left none.
      if (command === "read_pre_upgrade_backup") return null
      throw new Error(`Unexpected migration command: ${command}`)
    })
    renderNativeGate()
    expect(await screen.findByText("Some computers could not be migrated")).toBeVisible()
    expect(screen.getByRole("alert")).toHaveTextContent("dev could not be converted")
    fireEvent.click(screen.getByRole("button", { name: "Show logs" }))
    expect(screen.getByText("Full log: /tmp/silo-migration.log")).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Retry migration" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(native.invoke.mock.calls).toEqual([["read_runtime_migration_state"], ["read_runtime_migration_state"], ["retry_runtime_migration"], ["read_runtime_migration_state"], ["read_pre_upgrade_backup"]])
    expect(native.listen).toHaveBeenCalledWith("silo://application-state-changed", expect.any(Function))
  })

  it("continues through the native command only after the user acknowledges failure", async () => {
    native.invoke.mockImplementation(async command => {
      if (command === "read_runtime_migration_state") return failedJson
      if (command === "continue_after_migration_failure") return completeJson
      if (command === "read_pre_upgrade_backup") return null
      throw new Error(`Unexpected migration command: ${command}`)
    })
    renderNativeGate()
    const continueButton = await screen.findByRole("button", { name: "Continue with available computers" })
    expect(continueButton).toBeDisabled()
    expect(native.invoke).not.toHaveBeenCalledWith("continue_after_migration_failure")
    fireEvent.click(screen.getByRole("checkbox"))
    fireEvent.click(continueButton)
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(native.invoke.mock.calls).toEqual([["read_runtime_migration_state"], ["read_runtime_migration_state"], ["continue_after_migration_failure"], ["read_pre_upgrade_backup"]])
  })

  it("tells the user about the pre-upgrade backup through the native commands, once", async () => {
    // Matches pre_upgrade_backup::Status's serde camelCase JSON.
    const backup = JSON.parse(`{"deleteAt":"2026-10-15T12:00:00Z","noticePending":true}`)
    native.invoke.mockImplementation(async command => {
      if (command === "read_runtime_migration_state") return completeJson
      if (command === "read_pre_upgrade_backup") return backup
      if (command === "measure_pre_upgrade_backup") return 13314398617
      if (command === "acknowledge_pre_upgrade_backup_notice") return null
      if (command === "read_backup_state") return idleBackupState
      throw new Error(`Unexpected migration command: ${command}`)
    })
    renderNativeGate()
    expect(await screen.findByRole("heading", { name: "Your computers were updated" })).toBeVisible()
    expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
    expect(await screen.findByText("12.4 GiB")).toBeVisible()
    expect(screen.getByText("Silo deletes it automatically on October 15, 2026.")).toBeVisible()
    expect(native.listen).toHaveBeenCalledWith("silo://pre-upgrade-backup-changed", expect.any(Function))
    fireEvent.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("acknowledge_pre_upgrade_backup_notice"))
    // Nothing was produced by the upgrade, so nothing is acknowledged.
    expect(native.invoke).not.toHaveBeenCalledWith("acknowledge_backup_result", expect.anything())
  })

  it("tells the user the outcome of an export or import the upgrade interrupted, and acknowledges it before opening Silo", async () => {
    const backup = JSON.parse(`{"deleteAt":"2026-10-15T12:00:00Z","noticePending":true}`)
    let acknowledged: unknown
    native.invoke.mockImplementation(async (command, args) => {
      if (command === "read_runtime_migration_state") return completeJson
      if (command === "read_pre_upgrade_backup") return backup
      if (command === "measure_pre_upgrade_backup") return 13314398617
      if (command === "acknowledge_pre_upgrade_backup_notice") return null
      if (command === "read_backup_state") return acknowledged ? idleBackupState : unseenBackupState
      if (command === "acknowledge_backup_result") { acknowledged = args; return true }
      throw new Error(`Unexpected migration command: ${command}`)
    })
    renderNativeGate()
    expect(await screen.findByRole("heading", { name: "Import interrupted before the upgrade" })).toBeVisible()
    expect(screen.getByText("Silo closed before this import finished. No computer was added. Import the file again.")).toBeVisible()
    expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
    expect(native.listen).toHaveBeenCalledWith("silo://application-state-changed", expect.any(Function))
    fireEvent.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    // The id the result was read with, so nothing else can be marked seen.
    expect(acknowledged).toEqual({ expectedOperationId: "5b0c8e3e-3b8e-4c4c-9a0b-1f0f5f2d2b77" })
  })

  it("does not read the export and import state when the screen about the backup is not shown", async () => {
    native.invoke.mockImplementation(async command => {
      if (command === "read_runtime_migration_state") return completeJson
      // The notice was already acknowledged, so the application shows any unseen result itself.
      if (command === "read_pre_upgrade_backup") return { deleteAt: "2026-10-15T12:00:00Z", noticePending: false }
      throw new Error(`Unexpected migration command: ${command}`)
    })
    renderNativeGate()
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(native.invoke).not.toHaveBeenCalledWith("read_backup_state")
  })

  it("deletes the pre-upgrade backup through the native command", async () => {
    let present = true
    native.invoke.mockImplementation(async command => {
      if (command === "read_runtime_migration_state") return completeJson
      if (command === "read_pre_upgrade_backup") return present ? { deleteAt: null, noticePending: true } : null
      if (command === "measure_pre_upgrade_backup") return 1048576
      if (command === "delete_pre_upgrade_backup") { present = false; return null }
      if (command === "read_backup_state") return idleBackupState
      throw new Error(`Unexpected migration command: ${command}`)
    })
    renderNativeGate()
    expect(await screen.findByText("Silo will not delete it automatically.")).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Delete now" }))
    fireEvent.click(await screen.findByRole("button", { name: "Delete permanently" }))
    expect(await screen.findByText("The pre-upgrade backup was deleted.")).toBeVisible()
    expect(native.invoke).toHaveBeenCalledWith("delete_pre_upgrade_backup")
  })

  it.each([
    ["a damaged date", { deleteAt: "someday", noticePending: true }],
    ["a missing notice flag", { deleteAt: null }],
  ])("opens Silo rather than blocking it on %s in the backup status", async (_case, backup) => {
    native.invoke.mockImplementation(async command => {
      if (command === "read_runtime_migration_state") return completeJson
      if (command === "read_pre_upgrade_backup") return backup
      throw new Error(`Unexpected migration command: ${command}`)
    })
    renderNativeGate()
    expect(await screen.findByText("Normal application")).toBeVisible()
  })

  it.each([
    ["unknown status", { ...completeJson, status: "ready" }],
    ["negative count", { ...completeJson, migratedCount: -1 }],
    ["fractional count", { ...completeJson, failedCount: 0.5 }],
    ["missing stage", { ...completeJson, stage: undefined }],
    ["non-string log", { ...completeJson, logs: [23] }],
    ["non-boolean continue", { ...completeJson, canContinue: "true" }],
  ])("keeps the gate closed when backend JSON has %s", async (_case, response) => {
    native.invoke.mockResolvedValue(response)
    renderNativeGate()
    expect(await screen.findByRole("alert")).toBeVisible()
    expect(screen.getByText("Migration status is unavailable")).toBeVisible()
    expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith("read_runtime_migration_state")
  })
})
