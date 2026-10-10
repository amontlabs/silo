import { act, render } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { useClock } from "./use-clock"

function Clock({ id, active = true }: { id: string; active?: boolean }) {
  return <output data-testid={id}>{useClock(active)}</output>
}

describe("useClock", () => {
  beforeEach(() => { vi.useFakeTimers({ now: 1_000_000 }) })
  afterEach(() => { vi.useRealTimers() })

  it("runs one interval for every subscriber and stops with the last one", () => {
    const setInterval = vi.spyOn(window, "setInterval")
    const clearInterval = vi.spyOn(window, "clearInterval")
    const first = render(<Clock id="a" />)
    const second = render(<Clock id="b" />)
    expect(setInterval).toHaveBeenCalledTimes(1)
    act(() => { vi.advanceTimersByTime(3000) })
    expect(first.getByTestId("a").textContent).toBe("1003000")
    expect(second.getByTestId("b").textContent).toBe("1003000")
    first.unmount()
    expect(clearInterval).not.toHaveBeenCalled()
    second.unmount()
    expect(clearInterval).toHaveBeenCalledTimes(1)
    expect(vi.getTimerCount()).toBe(0)
  })

  it("does not tick while inactive", () => {
    render(<Clock id="a" active={false} />)
    expect(vi.getTimerCount()).toBe(0)
  })
})
