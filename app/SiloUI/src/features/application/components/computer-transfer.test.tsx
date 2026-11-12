import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"

import { useComputerTransfer, type ComputerTransfer } from "./computer-transfer"
import { Toaster } from "@/components/ui/sonner"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { ExportIncompleteError, type BackupController, type BackupOperation } from "../model/backup-source"

const source = applicationSourceForScenario("running")
const localComputer = source.computers.find((w) => !w.device)!
const archive = { name: "dev.silo-backup", archivePath: "/backups/dev.silo-backup", completedLabel: "Today", size: "2 GiB", destination: "/backups", computers: ["dev"] }

function controller(overrides: Partial<BackupController["state"]> = {}, actions: Partial<BackupController["actions"]> = {}): BackupController {
  return {
    state: { snapshotId: "1", availability: "available", archives: [archive], operation: null, ...overrides },
    actions: { checkLogins: vi.fn().mockResolvedValue({ logins: [] }), chooseDestination: vi.fn(), chooseArchive: vi.fn(), inspectArchive: vi.fn(), startBackup: vi.fn(), exportAndVerify: vi.fn().mockReturnValue(new Promise(() => {})), startRestore: vi.fn(), cancelOperation: vi.fn(), dismissOperation: vi.fn(), revealArchive: vi.fn().mockResolvedValue(undefined), ...actions },
  }
}

function Capture({ backup, onTransfer }: { backup: BackupController; onTransfer: (transfer: ComputerTransfer) => void }) {
  onTransfer(useComputerTransfer(backup, { source }))
  return null
}

function Harness({ backup, openComputer = vi.fn() }: { backup: BackupController; openComputer?: (id: string) => void }) {
  const transfer = useComputerTransfer(backup, { source, openComputer })
  return <SettingsProvider initialSettings={{ theme: "light" }}>
    <Toaster />
    <button type="button" onClick={() => void transfer.exportComputer("dev")}>Start export</button>
    <button type="button" onClick={() => void transfer.beginImport()}>Start import</button>
    {transfer.importPopover(<button type="button">Add</button>)}
    {transfer.loginConfirmation}
  </SettingsProvider>
}

afterEach(() => { toast.dismiss() })

describe("logins found before an export", () => {
  const claude = "a Claude Code login (/root/.claude/.credentials.json)"

  it("names the logins and waits for an answer before a folder is chosen", async () => {
    const backup = controller({}, { checkLogins: vi.fn().mockResolvedValue({ logins: [claude] }), chooseDestination: vi.fn().mockResolvedValue("/Volumes/Backups") })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start export" }))
    expect(await screen.findByText(claude)).toBeInTheDocument()
    expect(screen.getByText(/Silo cannot leave these files out of this export/)).toBeInTheDocument()
    expect(screen.queryByText(/Silo keeps these files in the export/)).not.toBeInTheDocument()
    expect(backup.actions.checkLogins).toHaveBeenCalledWith("dev", undefined)
    expect(backup.actions.chooseDestination).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Share anyway" }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.exportAndVerify).toHaveBeenCalledExactlyOnceWith("/Volumes/Backups", ["dev"], undefined)
  })

  describe("when the export removes them", () => {
    const found = { logins: [claude], leavesOut: true }
    const withLogins = () => controller({}, { checkLogins: vi.fn().mockResolvedValue(found), chooseDestination: vi.fn().mockResolvedValue("/Volumes/Backups") })

    it("says they are removed, with the caveat, and exports without them on the default choice", async () => {
      const backup = withLogins()
      render(<Harness backup={backup} />)
      fireEvent.click(screen.getByRole("button", { name: "Start export" }))
      expect(await screen.findByText(claude)).toBeInTheDocument()
      expect(screen.getByText(/Silo removes these files from the export, but traces may remain in the file/)).toBeInTheDocument()
      expect(screen.queryByText(/Silo keeps these files/)).not.toBeInTheDocument()
      expect(backup.actions.chooseDestination).not.toHaveBeenCalled()
      fireEvent.click(screen.getByRole("button", { name: "Remove logins and export" }))
      await act(async () => { await Promise.resolve() })
      expect(backup.actions.exportAndVerify).toHaveBeenCalledExactlyOnceWith("/Volumes/Backups", ["dev"], undefined)
    })

    it("includes them only when the person chooses to", async () => {
      const backup = withLogins()
      render(<Harness backup={backup} />)
      fireEvent.click(screen.getByRole("button", { name: "Start export" }))
      fireEvent.click(await screen.findByRole("button", { name: "Include logins" }))
      await act(async () => { await Promise.resolve() })
      expect(backup.actions.exportAndVerify).toHaveBeenCalledExactlyOnceWith("/Volumes/Backups", ["dev"], undefined, true)
    })

    it("exports nothing on Cancel", async () => {
      const backup = withLogins()
      render(<Harness backup={backup} />)
      fireEvent.click(screen.getByRole("button", { name: "Start export" }))
      fireEvent.click(await screen.findByRole("button", { name: "Cancel" }))
      await act(async () => { await Promise.resolve() })
      expect(backup.actions.chooseDestination).not.toHaveBeenCalled()
      expect(backup.actions.exportAndVerify).not.toHaveBeenCalled()
    })

    it("asks nothing when none are found, even if the computer could not be listed", async () => {
      const unchecked = "the computer is stopped, so its files were not listed; start it to check them"
      const backup = controller({}, { checkLogins: vi.fn().mockResolvedValue({ logins: [], unchecked, leavesOut: true }), chooseDestination: vi.fn().mockResolvedValue("/Volumes/Backups") })
      let transfer: ComputerTransfer | undefined
      render(<Capture backup={backup} onTransfer={(value) => { transfer = value }} />)
      void transfer!.exportComputer("dev")
      await act(async () => { await Promise.resolve(); await Promise.resolve() })
      expect(backup.actions.exportAndVerify).toHaveBeenCalledExactlyOnceWith("/Volumes/Backups", ["dev"], undefined)
    })
  })

  it("exports nothing when the answer is Cancel", async () => {
    const backup = controller({}, { checkLogins: vi.fn().mockResolvedValue({ logins: [claude] }), chooseDestination: vi.fn().mockResolvedValue("/Volumes/Backups") })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start export" }))
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.chooseDestination).not.toHaveBeenCalled()
    expect(backup.actions.exportAndVerify).not.toHaveBeenCalled()
    expect(screen.queryByText(claude)).not.toBeInTheDocument()
  })

  it("asks nothing when no login is found, and checks the checkpoint it exports", async () => {
    const backup = controller({}, { chooseDestination: vi.fn().mockResolvedValue("/Volumes/Backups") })
    let transfer: ComputerTransfer | undefined
    render(<Capture backup={backup} onTransfer={(value) => { transfer = value }} />)
    void transfer!.exportComputer("dev", { id: "checkpoint-1", name: "Before upgrade" })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(backup.actions.checkLogins).toHaveBeenCalledWith("dev", "checkpoint-1")
    expect(backup.actions.exportAndVerify).toHaveBeenCalledWith("/Volumes/Backups", ["dev"], "checkpoint-1")
  })

  it("asks again when the check itself fails, saying why", async () => {
    const backup = controller({}, { checkLogins: vi.fn().mockRejectedValue(new Error("The computer is busy.")), chooseDestination: vi.fn().mockResolvedValue("/Volumes/Backups") })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start export" }))
    expect(await screen.findByText("Silo could not check dev for logins (The computer is busy).")).toBeInTheDocument()
    expect(backup.actions.exportAndVerify).not.toHaveBeenCalled()
  })

  it("asks about a stopped computer that holds no logins that were found, and exports only after a yes", async () => {
    const unchecked = "the computer is stopped, so its files were not listed; start it to check them"
    const backup = controller({}, { checkLogins: vi.fn().mockResolvedValue({ logins: [], unchecked }), chooseDestination: vi.fn().mockResolvedValue("/Volumes/Backups") })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start export" }))
    expect(await screen.findByText(`Silo could not check dev for logins (${unchecked}).`)).toBeInTheDocument()
    expect(backup.actions.chooseDestination).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Share anyway" }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.exportAndVerify).toHaveBeenCalledExactlyOnceWith("/Volumes/Backups", ["dev"], undefined)
  })

  it("says what could not be checked beside the logins that were found", async () => {
    const unchecked = "the computer is stopped, so its files were not listed; start it to check them"
    const backup = controller({}, { checkLogins: vi.fn().mockResolvedValue({ logins: ["a GitHub CLI login (/root/.config/gh/hosts.yml)"], unchecked }) })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start export" }))
    expect(await screen.findByText(`Silo could not check dev for other logins (${unchecked}).`)).toBeInTheDocument()
  })
})

describe("export notifications", () => {
  it("does not start an export if another transfer begins while the folder picker is open", async () => {
    let choose!: (path: string) => void
    const backup = controller({}, { chooseDestination: vi.fn(() => new Promise<string>(resolve => { choose = resolve })) })
    let transfer!: ComputerTransfer
    const { rerender } = render(<Capture backup={backup} onTransfer={value => { transfer = value }} />)
    const exporting = transfer.exportComputer("dev")
    await act(async () => { await Promise.resolve() })
    const running: BackupOperation = { kind: "running", operation: "restore", archive, runningNames: [], progress: 30, phases: [] }
    rerender(<Capture backup={{ ...backup, state: { ...backup.state, operation: running } }} onTransfer={value => { transfer = value }} />)
    choose("/backups")
    await expect(exporting).resolves.toBeNull()
    expect(backup.actions.exportAndVerify).not.toHaveBeenCalled()
    expect(backup.actions.cancelOperation).not.toHaveBeenCalled()
  })

  it("starts the export after a folder is chosen and shows a running toast, not an inline panel", async () => {
    const backup = controller({}, { chooseDestination: vi.fn().mockResolvedValue("/Volumes/Backups") })
    const { rerender } = render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start export" }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.exportAndVerify).toHaveBeenCalledExactlyOnceWith("/Volumes/Backups", ["dev"], undefined)
    // No inline export panel is rendered anymore.
    expect(screen.queryByRole("region", { name: /Export dev/i })).not.toBeInTheDocument()

    const running: BackupOperation = { kind: "running", operation: "backup", archive, runningNames: [], progress: 40, phases: [{ title: "Save disk copies", detail: "Saving each managed disk.", tone: "running" }] }
    rerender(<Harness backup={controller({ operation: running })} />)
    expect(await screen.findByText("Exporting dev")).toBeInTheDocument()
    expect(screen.getByRole("progressbar", { name: "Saving each managed disk." })).toBeInTheDocument()
  })

  it("resolves with the verified export, or null when no verified export was produced", async () => {
    const verified = { operationId: "op-1", archive }
    const exportAndVerify = vi.fn().mockResolvedValueOnce(verified).mockRejectedValueOnce(new ExportIncompleteError("failed", "Disk full", "op-2"))
    const backup = controller({}, { chooseDestination: vi.fn().mockResolvedValueOnce("/Volumes/Backups").mockResolvedValueOnce("/Volumes/Backups").mockResolvedValueOnce(null), exportAndVerify })
    let transfer: ComputerTransfer | undefined
    render(<Capture backup={backup} onTransfer={(value) => { transfer = value }} />)
    await expect(transfer!.exportComputer("dev", { id: "checkpoint-1", name: "Before upgrade" })).resolves.toEqual(verified)
    expect(exportAndVerify).toHaveBeenCalledWith("/Volumes/Backups", ["dev"], "checkpoint-1")
    await expect(transfer!.exportComputer("dev")).resolves.toBeNull()
    await expect(transfer!.exportComputer("dev")).resolves.toBeNull()
    expect(exportAndVerify).toHaveBeenCalledTimes(2)
  })

  it("titles a checkpoint export from the backend state, even after a reload", async () => {
    const checkpointArchive = { ...archive, checkpointName: "Before upgrade" }
    const running: BackupOperation = { kind: "running", operation: "backup", archive: checkpointArchive, runningNames: [], progress: 0, phases: [{ title: "Waiting for other computer work", detail: "", tone: "running" }] }
    const { rerender } = render(<Harness backup={controller({ operation: running })} />)
    expect(await screen.findByText("Exporting checkpoint “Before upgrade”")).toBeInTheDocument()
    const done: BackupOperation = { kind: "result", operation: "backup", archive: checkpointArchive, runningNames: [], outcome: "success", title: "Export complete", message: "Computer exported." }
    rerender(<Harness backup={controller({ operation: done })} />)
    expect(await screen.findByText("Checkpoint exported")).toBeInTheDocument()
  })

  it("starting another export while one runs changes neither its title nor its Retry", async () => {
    const running: BackupOperation = { kind: "running", operation: "backup", archive, runningNames: [], progress: 0, phases: [{ title: "Capture and verify", detail: "", tone: "running" }] }
    const chooseDestination = vi.fn().mockResolvedValue("/Volumes/Backups")
    const exportAndVerify = vi.fn()
    let transfer: ComputerTransfer | undefined
    const view = (operation: BackupOperation) => <SettingsProvider initialSettings={{ theme: "light" }}>
      <Toaster />
      <Capture backup={controller({ operation }, { chooseDestination, exportAndVerify })} onTransfer={(value) => { transfer = value }} />
    </SettingsProvider>
    const { rerender } = render(view(running))
    expect(await screen.findByText("Exporting dev")).toBeInTheDocument()
    await expect(transfer!.exportComputer("dev", { id: "checkpoint-1", name: "Other" })).resolves.toBeNull()
    expect(exportAndVerify).not.toHaveBeenCalled()
    expect(screen.getByText("Exporting dev")).toBeInTheDocument()
    // The export this hook never started fails: there is nothing of its own to retry.
    rerender(view({ kind: "result", operation: "backup", archive, runningNames: [], outcome: "failed", title: "Export failed", message: "Disk full" }))
    expect(await screen.findByText("Export failed")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument()
  })

  it("does nothing when the folder picker is cancelled", async () => {
    const backup = controller({}, { chooseDestination: vi.fn().mockResolvedValue(null) })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start export" }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.exportAndVerify).not.toHaveBeenCalled()
  })

  it("keeps a success toast with Show in Finder that reveals the export file", async () => {
    const success: BackupOperation = { kind: "result", operation: "backup", archive, runningNames: [], outcome: "success", title: "Export ready", message: "done" }
    const backup = controller({ operation: success })
    const { rerender } = render(<Harness backup={controller()} />)
    rerender(<Harness backup={backup} />)
    expect(await screen.findByText("Exported")).toBeInTheDocument()
    expect(screen.getByText("dev.silo-backup · 2 GiB")).toBeInTheDocument()
    fireEvent.click(await screen.findByRole("button", { name: /Show in (Finder|folder)/ }))
    expect(backup.actions.revealArchive).toHaveBeenCalledWith(archive)
    expect(backup.actions.dismissOperation).toHaveBeenCalledOnce()
  })

  it("tells how many login files an export removed", async () => {
    const left = { ...archive, leftOut: ["a Claude Code login (/root/.claude/.credentials.json)", "Claude Code sign-in data (/root/.claude.json)"] }
    const success: BackupOperation = { kind: "result", operation: "backup", archive: left, runningNames: [], outcome: "success", title: "Export ready", message: "done" }
    const { rerender } = render(<Harness backup={controller()} />)
    rerender(<Harness backup={controller({ operation: success })} />)
    expect(await screen.findByText("dev.silo-backup · 2 GiB · 2 login files removed")).toBeInTheDocument()
  })

  it("dismissing a result toast clears the backend operation", async () => {
    const success: BackupOperation = { kind: "result", operation: "backup", archive, runningNames: [], outcome: "success", title: "Export ready", message: "done" }
    const backup = controller({ operation: success })
    const { rerender } = render(<Harness backup={controller()} />)
    rerender(<Harness backup={backup} />)
    fireEvent.click(await screen.findByRole("button", { name: /close|dismiss/i }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.dismissOperation).toHaveBeenCalled()
  })

  it("shows the archive reveal failure from Show in Finder", async () => {
    const success: BackupOperation = { kind: "result", operation: "backup", archive, runningNames: [], outcome: "success", title: "Export ready", message: "done" }
    const backup = controller({ operation: success }, { revealArchive: vi.fn().mockRejectedValue(new Error("Archive missing")) })
    const { rerender } = render(<Harness backup={controller()} />)
    rerender(<Harness backup={backup} />)
    fireEvent.click(await screen.findByRole("button", { name: /Show in (Finder|folder)/ }))
    expect(await screen.findByText("Could not reveal the export")).toBeVisible()
    expect(screen.getByText("Archive missing")).toBeVisible()
    expect(backup.actions.revealArchive).toHaveBeenCalledExactlyOnceWith(archive)
  })

  it("offers Retry on a failed export", async () => {
    const backup = controller({}, { chooseDestination: vi.fn().mockResolvedValue("/vol") })
    const { rerender } = render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start export" }))
    await act(async () => { await Promise.resolve() })
    const failed: BackupOperation = { kind: "result", operation: "backup", archive, runningNames: [], outcome: "failed", title: "Export could not be verified", message: "The destination disconnected." }
    rerender(<Harness backup={controller({ operation: failed }, { chooseDestination: backup.actions.chooseDestination, exportAndVerify: backup.actions.exportAndVerify })} />)
    expect(await screen.findByText("Export could not be verified")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Retry" }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.chooseDestination).toHaveBeenCalledTimes(2)
  })
})

describe("results present at load", () => {
  it("does not toast a finished result from a previous session", async () => {
    const success: BackupOperation = { kind: "result", operation: "restore", archive, runningNames: [], targetName: localComputer.configuration.name, outcome: "success", title: "ready", message: "ok" }
    const backup = controller({ operation: success })
    render(<Harness backup={backup} />)
    await act(async () => { await Promise.resolve() })
    expect(screen.queryByText(`Imported ${localComputer.configuration.name}`)).not.toBeInTheDocument()
    expect(backup.actions.dismissOperation).not.toHaveBeenCalled()
  })

  it("does not toast a finished failure or cancellation from a previous session either", async () => {
    for (const outcome of ["failed", "cancelled"] as const) {
      const old: BackupOperation = { kind: "result", operation: "restore", archive, runningNames: [], targetName: "copy", outcome, title: "Import interrupted", message: "Silo closed before this import finished.", detail: "No computer was added. Import the file again." }
      const backup = controller({ operation: old })
      const { unmount } = render(<Harness backup={backup} />)
      await act(async () => { await Promise.resolve() })
      expect(screen.queryByText("Import interrupted")).not.toBeInTheDocument()
      expect(backup.actions.dismissOperation).not.toHaveBeenCalled()
      unmount()
    }
  })

  it("shows a failure the backend marks unseen even though it is present at load", async () => {
    const interrupted: BackupOperation = { kind: "result", operation: "restore", archive, runningNames: [], targetName: "copy", outcome: "failed", title: "Import interrupted before the upgrade", message: "Silo closed before this import finished.", detail: "No computer was added. Import the file again." }
    const backup = controller({ operation: interrupted, resultUnseen: true })
    render(<Harness backup={backup} />)
    expect(await screen.findByText("Import interrupted before the upgrade")).toBeVisible()
    expect(screen.getByText("Silo closed before this import finished.")).toBeVisible()
    expect(screen.getByText("No computer was added. Import the file again.")).toBeVisible()
    // There is no import of this session to retry.
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument()
    // Showing it acknowledges nothing: the window may be hidden, so only dismissing it does.
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.dismissOperation).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: /close|dismiss/i }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.dismissOperation).toHaveBeenCalledOnce()
  })

  it("shows the notice that an unreadable record was set aside", async () => {
    const setAside: BackupOperation = { kind: "result", operation: "backup", archive: { ...archive, name: "Export or import record", archivePath: "/data/backup-operation.unreadable-2026-10-01.json", computers: [] }, runningNames: [], outcome: "failed", title: "Export or import record set aside", message: "An export or import record couldn’t be read and was set aside.", detail: "If an export or import was running, run it again." }
    render(<Harness backup={controller({ operation: setAside, resultUnseen: true })} />)
    expect(await screen.findByText("Export or import record set aside")).toBeVisible()
    expect(screen.getByText("An export or import record couldn’t be read and was set aside.")).toBeVisible()
    expect(screen.getByText("If an export or import was running, run it again.")).toBeVisible()
  })

  it("shows an unseen result of every outcome, with the actions of an ordinary one", async () => {
    const done: BackupOperation = { kind: "result", operation: "backup", archive, runningNames: [], outcome: "success", title: "Export complete", message: "Silo verified this export after relaunching." }
    const { rerender } = render(<Harness backup={controller({ operation: done, resultUnseen: true })} />)
    expect(await screen.findByText("Exported")).toBeVisible()
    expect(screen.getByRole("button", { name: /Show in (Finder|folder)/ })).toBeVisible()
    const cancelled: BackupOperation = { kind: "result", operation: "backup", archive, runningNames: [], outcome: "cancelled", title: "Export cancelled", message: "The export was cancelled.", detail: "No export file was saved." }
    rerender(<Harness backup={controller({ operation: cancelled, resultUnseen: true })} />)
    expect(await screen.findByText("Export cancelled")).toBeVisible()
    expect(screen.getByText("The export was cancelled.")).toBeVisible()
    expect(screen.getByText("No export file was saved.")).toBeVisible()
  })

  it("keeps an unseen cancellation until it is dismissed, unlike the ordinary one that disappears by itself", async () => {
    vi.useFakeTimers()
    try {
      const cancelled: BackupOperation = { kind: "result", operation: "backup", archive, runningNames: [], outcome: "cancelled", title: "Export cancelled", message: "The export was cancelled." }
      const unseen = controller({ operation: cancelled, resultUnseen: true })
      render(<Harness backup={unseen} />)
      await act(async () => { await vi.advanceTimersByTimeAsync(10_000) })
      expect(screen.getByText("Export cancelled")).toBeVisible()
      expect(unseen.actions.dismissOperation).not.toHaveBeenCalled()
    } finally { vi.useRealTimers() }
  })

  it("does not show a result as unseen once the backend no longer reports it so", async () => {
    const interrupted: BackupOperation = { kind: "result", operation: "restore", archive, runningNames: [], targetName: "copy", outcome: "failed", title: "Import interrupted before the upgrade", message: "Silo closed before this import finished." }
    // Acknowledged on the screen shown after the upgrade, so it opens as any old result does.
    render(<Harness backup={controller({ operation: interrupted, resultUnseen: false })} />)
    await act(async () => { await Promise.resolve() })
    expect(screen.queryByText("Import interrupted before the upgrade")).not.toBeInTheDocument()
  })

  it("does not take an unseen flag for a running operation", async () => {
    const running: BackupOperation = { kind: "running", operation: "restore", archive, runningNames: [], targetName: "copy", progress: 10, phases: [{ title: "Create new computer", detail: "Writing managed disk data.", tone: "running" }] }
    render(<Harness backup={controller({ operation: running, resultUnseen: true })} />)
    expect(await screen.findByText("Importing copy")).toBeVisible()
  })

  it("dismisses a stale import result whose computer no longer exists", async () => {
    const success: BackupOperation = { kind: "result", operation: "restore", archive, runningNames: [], targetName: "gone-computer", outcome: "success", title: "ready", message: "ok" }
    const backup = controller({ operation: success })
    render(<Harness backup={backup} />)
    await act(async () => { await Promise.resolve() })
    expect(screen.queryByText("Imported gone-computer")).not.toBeInTheDocument()
    expect(backup.actions.dismissOperation).toHaveBeenCalled()
  })
})

describe("import notifications and popover", () => {
  it("replaces an in-flight inspection and ignores its late failure", async () => {
    const signals: AbortSignal[] = []
    let rejectFirst!: (error: Error) => void
    const next = { ...archive, computers: ["api"] }
    const chooseArchive = vi.fn().mockImplementationOnce((selected: (path: string) => void, signal: AbortSignal) => {
      signals.push(signal)
      selected("/first")
      return new Promise((_, reject) => { rejectFirst = reject })
    }).mockImplementationOnce(async (selected: (path: string) => void, signal: AbortSignal) => {
      signals.push(signal)
      selected("/second")
      return { archive: next, valid: true }
    })
    const backup = controller({}, { chooseArchive })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start import" }))
    expect(await screen.findByText("Checking export")).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Start import" }))
    expect(await screen.findByRole("textbox", { name: "New computer name" })).toHaveValue("api-imported")
    expect(signals[0].aborted).toBe(true)
    expect(signals[1].aborted).toBe(false)
    await act(async () => rejectFirst(new Error("Old inspection failed")))
    expect(screen.getByRole("textbox", { name: "New computer name" })).toHaveValue("api-imported")
    expect(screen.queryByText("Old inspection failed")).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Import" }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.startRestore).toHaveBeenCalledExactlyOnceWith(next, "api-imported", "api")
  })

  it("aborts archive inspection on unmount without starting an import", async () => {
    let signal!: AbortSignal
    let finish!: (value: { archive: typeof archive; valid: boolean }) => void
    const backup = controller({}, { chooseArchive: vi.fn((selected?: (path: string) => void, abort?: AbortSignal) => {
      signal = abort!
      selected?.("/export")
      return new Promise<{ archive: typeof archive; valid: boolean }>(resolve => { finish = resolve })
    }) })
    const view = render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start import" }))
    await screen.findByText("Checking export")
    view.unmount()
    expect(signal.aborted).toBe(true)
    await act(async () => finish({ archive, valid: true }))
    expect(backup.actions.startRestore).not.toHaveBeenCalled()
  })

  it("validates the new name, blocks conflicts, offers a source select, and imports", async () => {
    const multi = { ...archive, computers: ["dev", "api"] }
    const backup = controller({}, { chooseArchive: vi.fn().mockImplementation(async (onSelected?: (path: string) => void) => { onSelected?.("/p"); return { archive: multi, valid: true } }) })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start import" }))
    await act(async () => { await Promise.resolve() })
    const name = await screen.findByRole("textbox", { name: "New computer name" })
    expect(name).toHaveValue("dev-imported")
    expect(name).toHaveAttribute("autocapitalize", "off")
    expect(name).toHaveAttribute("autocorrect", "off")
    expect(name).toHaveAttribute("spellcheck", "false")
    expect(screen.getByRole("combobox", { name: "Computer to import" })).toBeInTheDocument()
    fireEvent.change(name, { target: { value: "Dev" } })
    expect(name).toHaveAttribute("aria-invalid", "true")
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled()
    fireEvent.change(name, { target: { value: "dev" } })
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled()
    fireEvent.change(name, { target: { value: "dev-copy" } })
    fireEvent.click(screen.getByRole("button", { name: "Import" }))
    await act(async () => { await Promise.resolve() })
    expect(backup.actions.startRestore).toHaveBeenCalledExactlyOnceWith(multi, "dev-copy", "dev")
  })

  it("closing the check stops it and never reopens the review with its late result", async () => {
    let signal: AbortSignal | undefined
    let finish: ((value: { archive: typeof archive; valid: boolean }) => void) | undefined
    const chooseArchive = vi.fn().mockImplementation((onSelected?: (path: string) => void, abort?: AbortSignal) => {
      onSelected?.("/p")
      signal = abort
      return new Promise(resolve => { finish = resolve })
    })
    render(<Harness backup={controller({}, { chooseArchive })} />)
    fireEvent.click(screen.getByRole("button", { name: "Start import" }))
    expect(await screen.findByText("Checking export")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    await vi.waitFor(() => expect(screen.queryByText("Checking export")).not.toBeInTheDocument())
    expect(signal?.aborted).toBe(true)
    await act(async () => { finish?.({ archive, valid: true }) })
    expect(screen.queryByRole("textbox", { name: "New computer name" })).not.toBeInTheDocument()
    expect(screen.queryByText(`Import ${archive.name}`)).not.toBeInTheDocument()
  })

  it("reports an invalid export file in the popover and lets the user choose another", async () => {
    const backup = controller({}, { chooseArchive: vi.fn().mockImplementation(async (onSelected?: (path: string) => void) => { onSelected?.("/p"); return { archive, valid: false, reason: "The checksum does not match." } }) })
    render(<Harness backup={backup} />)
    fireEvent.click(screen.getByRole("button", { name: "Start import" }))
    await act(async () => { await Promise.resolve() })
    expect(await screen.findByText(/This export cannot be imported/)).toBeInTheDocument()
    expect(screen.getByText(/The checksum does not match/)).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Choose another file" })).toBeEnabled()
    expect(backup.actions.startRestore).not.toHaveBeenCalled()
  })

  it("shows an Open action on import success that navigates to the new computer", async () => {
    const openComputer = vi.fn()
    const success: BackupOperation = { kind: "result", operation: "restore", archive, runningNames: [], targetName: localComputer.configuration.name, outcome: "success", title: `${localComputer.configuration.name} is ready`, message: "ok" }
    const backup = controller({ operation: success })
    const { rerender } = render(<Harness backup={controller()} openComputer={openComputer} />)
    rerender(<Harness backup={backup} openComputer={openComputer} />)
    expect(await screen.findByText(`Imported ${localComputer.configuration.name}`)).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Open" }))
    expect(openComputer).toHaveBeenCalledWith(localComputer.configuration.id)
    expect(backup.actions.dismissOperation).toHaveBeenCalledOnce()
  })

  it("finds the imported computer when Open is clicked, even if it appeared after the toast", async () => {
    const openComputer = vi.fn()
    const imported = { ...localComputer, configuration: { ...localComputer.configuration, id: "imported-id", name: "dev-copy" } }
    const withoutCopy = { ...source, computers: source.computers.filter(({ configuration }) => configuration.name !== "dev-copy") }
    const withCopy = { ...source, computers: [...withoutCopy.computers, imported] }
    const View = ({ backup, current }: { backup: BackupController; current: typeof source }) => {
      const transfer = useComputerTransfer(backup, { source: current, openComputer })
      return <SettingsProvider initialSettings={{ theme: "light" }}><Toaster />{transfer.importPopover(<button type="button">Add</button>)}</SettingsProvider>
    }
    const success: BackupOperation = { kind: "result", operation: "restore", archive, runningNames: [], targetName: "dev-copy", outcome: "success", title: "Import complete", message: "Computer imported." }
    const { rerender } = render(<View backup={controller()} current={withoutCopy} />)
    rerender(<View backup={controller({ operation: success })} current={withoutCopy} />)
    expect(await screen.findByText("Imported dev-copy")).toBeInTheDocument()
    // The application snapshot catches up after the toast was shown.
    rerender(<View backup={controller({ operation: success })} current={withCopy} />)
    fireEvent.click(screen.getByRole("button", { name: "Open" }))
    expect(openComputer).toHaveBeenCalledWith("imported-id")
  })

  it("confirms before cancelling a running import", async () => {
    const running: BackupOperation = { kind: "running", operation: "restore", archive, runningNames: [], targetName: "dev-copy", progress: 30, phases: [{ title: "Create new computer", detail: "Writing managed disk data.", tone: "running" }] }
    const backup = controller({ operation: running })
    render(<Harness backup={backup} />)
    expect(await screen.findByText("Importing dev-copy")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    // Cancelling before the import saves its computer adds nothing; nothing is removed.
    expect(await screen.findByText("Stop importing? No computer is added.")).toBeInTheDocument()
    expect(screen.queryByText(/Remove/)).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Stop" }))
    expect(backup.actions.cancelOperation).toHaveBeenCalledOnce()
  })

  it("stops offering Cancel once the import is saving its computer", async () => {
    const running: BackupOperation = { kind: "running", operation: "restore", archive, runningNames: [], targetName: "dev-copy", progress: 90, canCancel: false, phases: [{ title: "Saving stopped computer", detail: "", tone: "running" }] }
    render(<Harness backup={controller({ operation: running })} />)
    expect(await screen.findByText("Importing dev-copy")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument()
  })
})
