import { fireEvent, render, screen } from "@testing-library/react"
import { expect, it, vi } from "vitest"
import { loadWithRetry } from "./lazy-load"
import { StartupFailure } from "./startup-failure"

it("retries a failed load once and returns its result", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {})
  const load = vi.fn().mockRejectedValueOnce(new Error("chunk")).mockResolvedValue("module")
  await expect(loadWithRetry(load, 0)()).resolves.toBe("module")
  expect(load).toHaveBeenCalledTimes(2)
})

it("gives up after the retry fails, and a new call starts a fresh load", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {})
  const load = vi.fn().mockRejectedValue(new Error("chunk"))
  const loader = loadWithRetry(load, 0)
  await expect(loader()).rejects.toThrow("chunk")
  expect(load).toHaveBeenCalledTimes(2)
  load.mockResolvedValue("module")
  await expect(loader()).resolves.toBe("module")
})

it("offers Retry when the window code could not load", () => {
  const retry = vi.fn()
  render(<StartupFailure message="Silo startup failed." retry={retry} />)
  expect(screen.getByRole("alert")).toHaveTextContent("Silo startup failed.")
  fireEvent.click(screen.getByRole("button", { name: "Retry" }))
  expect(retry).toHaveBeenCalledOnce()
})
