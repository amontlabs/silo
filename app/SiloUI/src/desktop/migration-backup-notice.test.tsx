import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import type { TransferResultNotice, TransferResultNoticeBackend } from "@/features/application/model/transfer-result-notice"
import { createFixtureMigrationBackend, createFixturePreUpgradeBackup, fixtureDeleteFailure, type PreUpgradeBackupFixtureOptions } from "@/fixtures/pre-upgrade-backup"
import { createFixtureUnseenResult, type UnseenResultFixtureMode } from "@/fixtures/transfer-result-notice"
import { RuntimeMigrationBoundary } from "./runtime-migration-boundary"

function setup(options: PreUpgradeBackupFixtureOptions = {}) {
  const preUpgradeBackup = createFixturePreUpgradeBackup(options)
  const user = userEvent.setup()
  render(<RuntimeMigrationBoundary backend={createFixtureMigrationBackend(preUpgradeBackup)}><p>Normal application</p></RuntimeMigrationBoundary>)
  return { backup: preUpgradeBackup, user }
}

describe("migration complete: pre-upgrade backup", () => {
  it("tells the user the backup was kept, how big it is and the date it is deleted, before opening Silo", async () => {
    const { backup, user } = setup()
    expect(await screen.findByRole("heading", { name: "Your computers were updated" })).toBeVisible()
    expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
    expect(screen.getByRole("heading", { name: "Pre-upgrade backup" })).toBeVisible()
    expect(await screen.findByText("12.4 GiB")).toBeVisible()
    // A date, not a countdown.
    expect(screen.getByText("Silo deletes it automatically on October 15, 2026.")).toBeVisible()
    expect(screen.queryByText(/\bdays?\b/i)).not.toBeInTheDocument()
    // Linux disk images: copyable, not browsable. The notice points to Settings for Show.
    expect(screen.getByText(/copy it but not browse its files/)).toBeVisible()
    expect(screen.getByText(/Settings, General, Storage/)).toBeVisible()
    expect(screen.getByRole("button", { name: "Delete now" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(backup.calls).toContain("acknowledge")
    expect(backup.calls).not.toContain("remove")
  })

  it("shows the notice only until it was acknowledged, without measuring on later launches", async () => {
    const { backup } = setup({ noticePending: false })
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(screen.queryByRole("heading", { name: "Your computers were updated" })).not.toBeInTheDocument()
    // Opening Silo at every launch must not walk the backup.
    expect(backup.calls).toEqual(["read"])
  })

  it("opens Silo when there is no backup to report", async () => {
    const { backup } = setup({ gone: true })
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(backup.calls).toEqual(["read"])
  })

  it("opens Silo rather than blocking it when the backup cannot be read", async () => {
    const backup = createFixturePreUpgradeBackup()
    vi.spyOn(backup, "read").mockRejectedValue("Silo application storage is unavailable.")
    render(<RuntimeMigrationBoundary backend={createFixtureMigrationBackend(backup)}><p>Normal application</p></RuntimeMigrationBoundary>)
    expect(await screen.findByText("Normal application")).toBeVisible()
  })

  it("stays neutral while the backup is being read", async () => {
    let finish!: (value: null) => void
    const backup = createFixturePreUpgradeBackup()
    const read = vi.fn(() => new Promise<null>(resolve => { finish = resolve }))
    backup.read = read
    render(<RuntimeMigrationBoundary backend={createFixtureMigrationBackend(backup)}><p>Normal application</p></RuntimeMigrationBoundary>)
    await waitFor(() => expect(read).toHaveBeenCalled())
    expect(screen.getByText("Opening Silo…")).toBeInTheDocument()
    expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
    expect(screen.queryByRole("heading", { name: "Your computers were updated" })).not.toBeInTheDocument()
    await act(async () => finish(null))
    expect(await screen.findByText("Normal application")).toBeVisible()
  })

  it("says Silo will not delete the backup by itself when it cannot read its date", async () => {
    setup({ deleteAt: null })
    expect(await screen.findByText("Silo will not delete it automatically.")).toBeVisible()
  })

  it("asks before deleting and deletes only after confirmation", async () => {
    const { backup, user } = setup()
    await user.click(await screen.findByRole("button", { name: "Delete now" }))
    expect(await screen.findByText("Delete the pre-upgrade backup permanently?")).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(backup.calls).not.toContain("remove")
    await user.click(screen.getByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    expect(await screen.findByText("The pre-upgrade backup was deleted.")).toBeVisible()
    expect(backup.calls.filter(call => call === "remove")).toHaveLength(1)
    expect(screen.queryByRole("button", { name: "Delete now" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
  })

  it("keeps the backup and the screen on failure so the user can retry", async () => {
    const { backup, user } = setup({ failures: 1 })
    await user.click(await screen.findByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    expect(await screen.findByRole("alert")).toHaveTextContent(fixtureDeleteFailure)
    expect(screen.getByRole("heading", { name: "Pre-upgrade backup" })).toBeVisible()
    await waitFor(() => expect(screen.getByRole("button", { name: "Delete now" })).toBeEnabled())
    await user.click(screen.getByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    expect(await screen.findByText("The pre-upgrade backup was deleted.")).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    expect(backup.calls.filter(call => call === "remove")).toHaveLength(2)
  })

  it("keeps the screen when a change is reported while it is open", async () => {
    const { backup, user } = setup()
    await screen.findByRole("heading", { name: "Your computers were updated" })
    // Another deletion (the automatic one) reports a change: the notice says so, it does not vanish.
    await act(async () => { await backup.remove() })
    expect(await screen.findByText("The pre-upgrade backup was deleted.")).toBeVisible()
    expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
  })

  it("opens Silo even when recording that the notice was shown fails", async () => {
    const { backup, user } = setup()
    const errors = vi.spyOn(console, "error").mockImplementation(() => {})
    backup.acknowledge = async () => { throw new Error("disk full") }
    await user.click(await screen.findByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    await waitFor(() => expect(errors).toHaveBeenCalledWith("Silo pre-upgrade backup:", "disk full"))
  })

  it("is not offered while the migration is unfinished or failed", async () => {
    const backup = createFixturePreUpgradeBackup()
    const migration = { ...createFixtureMigrationBackend(backup), read: async () => ({ status: "running" as const, stage: "Converting computer 1 of 2", logs: [], migratedCount: 0, failedCount: 0, totalCount: 2, canContinue: false }) }
    render(<RuntimeMigrationBoundary backend={migration}><p>Normal application</p></RuntimeMigrationBoundary>)
    expect(await screen.findByText("Updating your computers")).toBeVisible()
    expect(backup.calls).toEqual([])
  })
})

describe("migration complete: result of an export or import the upgrade interrupted", () => {
  function setupResult(mode: UnseenResultFixtureMode, options: PreUpgradeBackupFixtureOptions = {}, transfer = createFixtureUnseenResult(mode)) {
    const backup = createFixturePreUpgradeBackup(options)
    const user = userEvent.setup()
    // What the application sees when it opens: the child records whether the result was still unseen then.
    const opened: boolean[] = []
    function Application() {
      opened.push(transfer.current().unseen)
      return <p>Normal application</p>
    }
    render(<RuntimeMigrationBoundary backend={createFixtureMigrationBackend(backup, transfer)}><Application /></RuntimeMigrationBoundary>)
    return { backup, transfer, user, opened }
  }

  it("tells the user what became of an interrupted import on the screen about the backup", async () => {
    setupResult("interrupted-import")
    expect(await screen.findByRole("heading", { name: "Your computers were updated" })).toBeVisible()
    const result = await screen.findByRole("region", { name: "Import interrupted before the upgrade" })
    expect(result).toHaveTextContent("Silo closed before this import finished. No computer was added. Import the file again.")
    // Beside the backup, not instead of it.
    expect(screen.getByRole("heading", { name: "Pre-upgrade backup" })).toBeVisible()
    expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
  })

  it("shows the notice that an unreadable record was set aside", async () => {
    setupResult("set-aside")
    const result = await screen.findByRole("region", { name: "Export or import record set aside" })
    expect(result).toHaveTextContent("An export or import record couldn’t be read and was set aside. If an export or import was running, run it again.")
  })

  it("acknowledges the result when Open Silo is chosen, before the application opens, so the application does not show it again", async () => {
    const { transfer, user, opened } = setupResult("interrupted-import")
    await screen.findByRole("region", { name: "Import interrupted before the upgrade" })
    // Nothing is acknowledged by showing it: the user may close the window without reading it.
    expect(transfer.calls).not.toContain("acknowledge")
    expect(transfer.current().unseen).toBe(true)
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(transfer.calls.filter(call => call === "acknowledge")).toHaveLength(1)
    expect(transfer.current().unseen).toBe(false)
    expect(opened).toEqual([false])
    expect(screen.queryByRole("region", { name: "Import interrupted before the upgrade" })).not.toBeInTheDocument()
  })

  it("keeps the result on the screen while it is acknowledged, and Open Silo waits for it", async () => {
    const transfer = createFixtureUnseenResult("interrupted-export")
    let finish!: () => void
    const acknowledge = transfer.acknowledge
    transfer.acknowledge = async id => { await new Promise<void>(resolve => { finish = resolve }); await acknowledge(id) }
    const { user } = setupResult("interrupted-export", {}, transfer)
    await screen.findByRole("region", { name: "Export interrupted before the upgrade" })
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(screen.getByRole("button", { name: "Open Silo" })).toBeDisabled()
    expect(screen.queryByText("Normal application")).not.toBeInTheDocument()
    await act(async () => finish())
    expect(await screen.findByText("Normal application")).toBeVisible()
  })

  it("does not make the result vanish when Silo reports a change after it was acknowledged", async () => {
    const transfer = createFixtureUnseenResult("interrupted-import")
    let finish!: () => void
    const acknowledge = transfer.acknowledge
    // The acknowledgement changes the result, and Silo reports that before the call returns.
    transfer.acknowledge = async id => { await acknowledge(id); await new Promise<void>(resolve => { finish = resolve }) }
    const { user } = setupResult("interrupted-import", {}, transfer)
    await screen.findByRole("region", { name: "Import interrupted before the upgrade" })
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    await waitFor(() => expect(transfer.current().unseen).toBe(false))
    await act(async () => { await Promise.resolve() })
    expect(screen.getByRole("region", { name: "Import interrupted before the upgrade" })).toBeVisible()
    await act(async () => finish())
    expect(await screen.findByText("Normal application")).toBeVisible()
  })

  it("opens Silo and leaves the result unseen, so the application shows it, when acknowledging fails", async () => {
    const transfer = createFixtureUnseenResult("interrupted-import")
    const errors = vi.spyOn(console, "error").mockImplementation(() => {})
    transfer.acknowledge = async () => { throw new Error("disk full") }
    const { user, opened } = setupResult("interrupted-import", {}, transfer)
    await user.click(await screen.findByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(opened).toEqual([true])
    await waitFor(() => expect(errors).toHaveBeenCalledWith("Silo export and import result:", "disk full"))
  })

  it("shows a result that is recorded after the screen opened, once recovery has finished", async () => {
    let result: TransferResultNotice | null = null
    let refresh = () => {}
    const transfer: TransferResultNoticeBackend = {
      read: async () => result,
      acknowledge: async () => {},
      subscribe: async changed => { refresh = changed; return () => {} },
    }
    const backup = createFixturePreUpgradeBackup()
    render(<RuntimeMigrationBoundary backend={createFixtureMigrationBackend(backup, transfer)}><p>Normal application</p></RuntimeMigrationBoundary>)
    await screen.findByRole("heading", { name: "Your computers were updated" })
    expect(screen.queryByRole("region", { name: "Import interrupted" })).not.toBeInTheDocument()
    result = { id: "op-1", operation: "restore", outcome: "failed", title: "Import interrupted", message: "Silo closed before this import finished.", detail: "No computer was added. Import the file again." }
    await act(async () => refresh())
    expect(await screen.findByRole("region", { name: "Import interrupted" })).toBeVisible()
  })

  it("shows an outcome that is not a failure without a warning", async () => {
    const transfer: TransferResultNoticeBackend = {
      read: async () => ({ id: "op-1", operation: "backup", outcome: "success", title: "Export complete", message: "Silo verified this export after relaunching." }),
      acknowledge: async () => {},
      subscribe: async () => () => {},
    }
    render(<RuntimeMigrationBoundary backend={createFixtureMigrationBackend(createFixturePreUpgradeBackup(), transfer)}><p>Normal application</p></RuntimeMigrationBoundary>)
    const result = await screen.findByRole("region", { name: "Export complete" })
    expect(result).toHaveTextContent("Silo verified this export after relaunching.")
    expect(result).not.toHaveClass("border-warning/30")
  })

  it("does not read the result when the screen is not shown, so the application shows it", async () => {
    const { backup, transfer, opened } = setupResult("interrupted-import", { noticePending: false })
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(backup.calls).toEqual(["read"])
    expect(transfer.calls).toEqual([])
    // Still unseen when the application opens, which is what makes it show the result.
    expect(opened).toEqual([true])
  })

  it("does not read it either when the backup is already gone", async () => {
    const { transfer, opened } = setupResult("interrupted-import", { gone: true })
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(transfer.calls).toEqual([])
    expect(opened).toEqual([true])
  })

  it("opens Silo without a result when it cannot be read", async () => {
    const transfer = createFixtureUnseenResult("interrupted-import")
    const errors = vi.spyOn(console, "error").mockImplementation(() => {})
    transfer.read = async () => { throw new Error("Silo could not read export and import state.") }
    const { user } = setupResult("interrupted-import", {}, transfer)
    await user.click(await screen.findByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(screen.queryByRole("region", { name: /interrupted/ })).not.toBeInTheDocument()
    expect(transfer.calls).not.toContain("acknowledge")
    expect(errors).toHaveBeenCalledWith("Silo export and import result:", "Silo could not read export and import state.")
  })

  it("shows the screen without a result when none is unseen", async () => {
    const transfer = createFixtureUnseenResult("interrupted-import")
    transfer.read = async () => null
    const { user } = setupResult("interrupted-import", {}, transfer)
    await screen.findByRole("heading", { name: "Your computers were updated" })
    expect(screen.queryByRole("region", { name: /interrupted/ })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(await screen.findByText("Normal application")).toBeVisible()
    expect(transfer.calls).not.toContain("acknowledge")
  })
})
