import { beforeEach, expect, it, vi } from "vitest"
import { desktopUpdateBackend } from "./updates"
import type { UpdateSnapshot } from "@/features/updates/update-store"

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }))
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }))
const state: UpdateSnapshot = { phase: "idle", lastChecked: null, retryAction: null, currentVersion: "0.1.0", availableVersion: null, releaseNotes: null, downloadedBytes: 0, totalBytes: null, automaticChecks: true, packageKind: "macos", releaseUrl: "https://github.com/amontlabs/silo/releases", error: null, errorDetails: null, installBlockReason: null, runningComputers: [], canInstall: true }
beforeEach(() => { vi.resetAllMocks(); native.invoke.mockResolvedValue(state) })
it("uses native updater commands and requires an explicit computer-stop decision", async () => {
  await desktopUpdateBackend.read()
  expect(native.invoke).toHaveBeenLastCalledWith("get_update_state", undefined)
  await desktopUpdateBackend.check()
  expect(native.invoke).toHaveBeenLastCalledWith("check_for_update", undefined)
  await desktopUpdateBackend.download()
  expect(native.invoke).toHaveBeenLastCalledWith("download_update", undefined)
  await desktopUpdateBackend.install(false)
  expect(native.invoke).toHaveBeenLastCalledWith("install_update", { stopComputers: false })
  await desktopUpdateBackend.install(true)
  expect(native.invoke).toHaveBeenLastCalledWith("install_update", { stopComputers: true })
  await desktopUpdateBackend.setAutomaticChecks(false)
  expect(native.invoke).toHaveBeenLastCalledWith("set_update_automatic_checks", { enabled: false })
  await desktopUpdateBackend.openRelease()
  expect(native.invoke).toHaveBeenLastCalledWith("open_update_release")
})
it("rejects malformed native state instead of reporting update success", async () => {
  native.invoke.mockResolvedValue({ phase: "ready" })
  await expect(desktopUpdateBackend.read()).rejects.toThrow()
})
it("listens to real native update progress and returns the native unsubscriber", async () => {
  const stop = vi.fn()
  native.listen.mockResolvedValue(stop)
  const receive = vi.fn()
  expect(await desktopUpdateBackend.subscribe(receive)).toBe(stop)
  expect(native.listen).toHaveBeenCalledWith("silo://update-state", expect.any(Function))
  native.listen.mock.calls[0][1]({ payload: { ...state, phase: "downloading", downloadedBytes: 75, totalBytes: 100 } })
  expect(receive).toHaveBeenCalledWith(expect.objectContaining({ phase: "downloading", downloadedBytes: 75 }))
})

it("ignores malformed native update events without losing the next valid event", async () => {
  native.listen.mockResolvedValue(vi.fn())
  const receive = vi.fn()
  const log = vi.spyOn(console, "error").mockImplementation(() => {})
  await desktopUpdateBackend.subscribe(receive)
  const emit = native.listen.mock.calls[0][1]
  emit({ payload: { ...state, downloadedBytes: -1 } })
  expect(receive).not.toHaveBeenCalled()
  expect(log).toHaveBeenCalledWith("Silo updates: invalid native state")
  emit({ payload: { ...state, phase: "ready", availableVersion: "0.2.0" } })
  expect(receive).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ phase: "ready", availableVersion: "0.2.0" }))
})
