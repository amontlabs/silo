import { act, renderHook } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions } from "../model/application-source"
import { useNetworkPorts } from "../components/network-ports-state"
import { useSshAccessRefresh } from "./use-ssh-access-refresh"

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks() })

it.each(["network", "ssh"] as const)("marks %s polling as background and bypasses it on focus", async service => {
  vi.useFakeTimers()
  const refresh = vi.fn().mockResolvedValue(undefined)
  const actions = { refreshNetwork: refresh } as unknown as ApplicationActions
  const computers = applicationSourceForScenario("running").computers
  const useRefresh = service === "ssh"
    ? () => useSshAccessRefresh(refresh)
    : () => useNetworkPorts({ computers, actions, active: true })
  const { unmount } = renderHook(useRefresh)
  expect(refresh).toHaveBeenCalledExactlyOnceWith({ background: false })
  await act(async () => vi.advanceTimersByTimeAsync(12_000))
  expect(refresh).toHaveBeenLastCalledWith({ background: true })
  act(() => window.dispatchEvent(new Event("focus")))
  expect(refresh).toHaveBeenCalledTimes(3)
  expect(refresh).toHaveBeenLastCalledWith({ background: false })
  // The visibility change that accompanies a focus does not refresh a second time.
  act(() => document.dispatchEvent(new Event("visibilitychange")))
  expect(refresh).toHaveBeenCalledTimes(3)
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden")
  await act(async () => vi.advanceTimersByTimeAsync(12_000))
  expect(refresh).toHaveBeenCalledTimes(3)
  unmount()
  await act(async () => vi.advanceTimersByTimeAsync(60_000))
  expect(refresh).toHaveBeenCalledTimes(3)
})
