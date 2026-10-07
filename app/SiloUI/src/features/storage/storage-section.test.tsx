import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { Toaster } from "@/components/ui/sonner"
import { createFixturePreUpgradeBackup, fixtureDeleteFailure, type PreUpgradeBackupFixtureOptions } from "@/fixtures/pre-upgrade-backup"
import { PreUpgradeBackupProvider, type PreUpgradeBackupBackend } from "./pre-upgrade-backup"
import { StorageSection } from "./storage-section"

function setup(options: PreUpgradeBackupFixtureOptions = {}, adjust: (backend: PreUpgradeBackupBackend & { calls: string[] }) => void = () => {}) {
  const backend = createFixturePreUpgradeBackup(options)
  adjust(backend)
  const user = userEvent.setup()
  render(<><Toaster /><PreUpgradeBackupProvider backend={backend}><StorageSection /></PreUpgradeBackupProvider></>)
  return { backend, user }
}

describe("Settings, General: Storage", () => {
  it("shows the backup with its size, the date it is deleted and what it holds", async () => {
    setup()
    const section = await screen.findByRole("region", { name: "Storage" })
    expect(within(section).getByRole("heading", { name: "Storage" })).toBeVisible()
    expect(within(section).getByRole("heading", { name: "Pre-upgrade backup" })).toBeVisible()
    expect(await within(section).findByText("12.4 GiB · deleted on October 15, 2026")).toBeVisible()
    // Linux disk images: copyable, not browsable. No countdown either.
    expect(within(section).getByText("It holds Linux disk images, so you can copy it but not browse its files.")).toBeVisible()
    expect(section).not.toHaveTextContent(/\bdays?\b/i)
    expect(within(section).getByRole("button", { name: "Show" })).toBeEnabled()
    expect(within(section).getByRole("button", { name: "Delete now" })).toBeEnabled()
  })

  it("says the size is being calculated until the walk finishes, and when it cannot be", async () => {
    let finish!: (bytes: number) => void
    const first = setup({}, backend => { backend.measure = () => new Promise(resolve => { finish = resolve }) })
    expect(await screen.findByText("Calculating size… · deleted on October 15, 2026")).toBeVisible()
    await act(async () => finish(1536 * 1024 ** 2))
    expect(await screen.findByText("1.5 GiB · deleted on October 15, 2026")).toBeVisible()
    expect(first.backend.calls).toEqual(["read"])
  })

  it("shows the unmeasurable size honestly", async () => {
    setup({}, backend => { backend.measure = async () => { throw new Error("Silo could not measure the pre-upgrade backup.") } })
    expect(await screen.findByText("Size unavailable · deleted on October 15, 2026")).toBeVisible()
  })

  it("says the backup is not deleted automatically when Silo cannot read its date", async () => {
    setup({ deleteAt: null })
    expect(await screen.findByText("12.4 GiB · not deleted automatically")).toBeVisible()
  })

  it("shows nothing, and measures nothing, when there is no backup", async () => {
    const none = setup({ gone: true })
    await waitFor(() => expect(none.backend.calls).toEqual(["read"]))
    expect(screen.queryByText("Storage")).not.toBeInTheDocument()
    expect(none.backend.calls).not.toContain("measure")
  })

  it("renders nothing without a backend", () => {
    render(<StorageSection />)
    expect(screen.queryByText("Storage")).not.toBeInTheDocument()
  })

  it("reveals the folder, and reports when it cannot", async () => {
    const { backend, user } = setup()
    await user.click(await screen.findByRole("button", { name: "Show" }))
    expect(backend.calls).toContain("reveal")
    backend.reveal = async () => { throw new Error("Silo could not show the pre-upgrade backup: no file manager") }
    await user.click(screen.getByRole("button", { name: "Show" }))
    expect(await screen.findByText("Could not show the pre-upgrade backup")).toBeVisible()
    expect(screen.getByText("Silo could not show the pre-upgrade backup: no file manager")).toBeVisible()
  })

  it("asks before deleting, and cancelling deletes nothing", async () => {
    const { backend, user } = setup()
    await user.click(await screen.findByRole("button", { name: "Delete now" }))
    expect(await screen.findByText("Delete the pre-upgrade backup permanently?")).toBeVisible()
    await screen.findByText(/frees up to 12.4 GiB/)
    expect(screen.getByText(/can't be undone/)).toBeVisible()
    expect(screen.getByText(/current computers aren't affected/)).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(screen.queryByText("Delete the pre-upgrade backup permanently?")).not.toBeInTheDocument()
    expect(backend.calls).not.toContain("remove")
    expect(screen.getByRole("heading", { name: "Pre-upgrade backup" })).toBeVisible()
  })

  it("deletes after confirmation, reports it and removes the row and section", async () => {
    const { backend, user } = setup()
    await user.click(await screen.findByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    expect(await screen.findByText("Pre-upgrade backup deleted")).toBeVisible()
    expect(backend.calls.filter(call => call === "remove")).toHaveLength(1)
    await waitFor(() => expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument())
  })

  it("keeps the row on failure, names the cause and succeeds on retry", async () => {
    const { backend, user } = setup({ failures: 1 })
    await user.click(await screen.findByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    expect(await screen.findByText("Could not delete the pre-upgrade backup")).toBeVisible()
    expect(screen.getByText(fixtureDeleteFailure)).toBeVisible()
    expect(screen.getByRole("heading", { name: "Pre-upgrade backup" })).toBeVisible()
    // The row stays usable, and its size is read again in case part of it was deleted.
    await waitFor(() => expect(backend.calls.filter(call => call === "measure")).toHaveLength(2))
    expect(screen.getByRole("button", { name: "Delete now" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Retry" }))
    await waitFor(() => expect(screen.getByText("Pre-upgrade backup deleted")).toBeVisible())
    expect(backend.calls.filter(call => call === "remove")).toHaveLength(2)
    await waitFor(() => expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument())
  })

  it("keeps the row on a second attempt through the row's own button too", async () => {
    const { backend, user } = setup({ failures: 1 })
    await user.click(await screen.findByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    await screen.findByText(fixtureDeleteFailure)
    await user.click(screen.getByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    await waitFor(() => expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument())
    expect(backend.calls.filter(call => call === "remove")).toHaveLength(2)
  })

  it("disables its actions while deleting and deletes once", async () => {
    let finish!: () => void
    const { backend, user } = setup({}, backend => {
      const remove = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
      backend.remove = remove
    })
    await user.click(await screen.findByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    await waitFor(() => expect(screen.getByRole("button", { name: "Delete now" })).toBeDisabled())
    expect(screen.getByRole("button", { name: "Show" })).toBeDisabled()
    expect(screen.getByText("Deleting the pre-upgrade backup")).toBeVisible()
    await act(async () => finish())
    await waitFor(() => expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument())
    expect(backend.remove).toHaveBeenCalledTimes(1)
  })

  it("does not report deletion success when an old toast retries an unfinished deletion", async () => {
    const { backend, user } = setup({ failures: 1 })
    await user.click(await screen.findByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    await screen.findByText(fixtureDeleteFailure)
    let fail!: (cause: Error) => void
    backend.remove = vi.fn(() => new Promise<void>((_, reject) => { fail = reject }))
    await user.click(screen.getByRole("button", { name: "Delete now" }))
    await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(screen.queryByText("Pre-upgrade backup deleted")).not.toBeInTheDocument()
    expect(backend.remove).toHaveBeenCalledTimes(1)
    await act(async () => fail(new Error("Deletion is still unavailable.")))
    await waitFor(() => expect(screen.getAllByText("Deletion is still unavailable.")).toHaveLength(2))
    expect(screen.queryByText("Pre-upgrade backup deleted")).not.toBeInTheDocument()
    expect(screen.getByRole("region", { name: "Storage" })).toBeVisible()
  })

  it("explains a read failure and offers Retry", async () => {
    const user = userEvent.setup()
    const backend = createFixturePreUpgradeBackup()
    const read = vi.spyOn(backend, "read").mockRejectedValueOnce("Silo application storage is unavailable.")
    render(<PreUpgradeBackupProvider backend={backend}><StorageSection /></PreUpgradeBackupProvider>)
    expect(await screen.findByRole("alert")).toHaveTextContent("Silo could not check for it. Silo application storage is unavailable.")
    expect(screen.queryByRole("button", { name: "Delete now" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(await screen.findByText("12.4 GiB · deleted on October 15, 2026")).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    expect(read).toHaveBeenCalledTimes(2)
  })

  it("follows a deletion that happens elsewhere, such as the automatic one", async () => {
    const { backend } = setup()
    await screen.findByRole("region", { name: "Storage" })
    await act(async () => { await backend.remove() })
    await waitFor(() => expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument())
  })

  it("reconciles a deletion missed while its listener was registering", async () => {
    let register!: () => void
    const { backend } = setup({}, backend => {
      const subscribe = backend.subscribe
      backend.subscribe = refresh => new Promise(resolve => {
        register = () => { void subscribe(refresh).then(resolve) }
      })
    })
    await act(async () => {})
    await act(async () => { await backend.remove() })
    await act(async () => register())
    await waitFor(() => expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument())
  })

  it("reports a failed backup listener and reconnects it through Retry", async () => {
    const { backend, user } = setup({}, backend => {
      vi.spyOn(backend, "subscribe").mockRejectedValueOnce(new Error("event registration failed"))
    })
    await screen.findByRole("region", { name: "Storage" })
    expect(await screen.findByRole("alert")).toHaveTextContent("Silo could not listen for backup changes. Try again.")
    await user.click(screen.getByRole("button", { name: "Retry" }))
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument())
    await act(async () => { await backend.remove() })
    await waitFor(() => expect(screen.queryByRole("region", { name: "Storage" })).not.toBeInTheDocument())
  })
})
