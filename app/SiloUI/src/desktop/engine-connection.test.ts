import { describe, expect, it, vi } from "vitest"

const events = vi.hoisted(() => ({ handlers: [] as Array<(event: { payload: unknown }) => void>, unlisten: vi.fn() }))
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, handler: (event: { payload: unknown }) => void) => { events.handlers.push(handler); return events.unlisten }),
}))

import { onEngineConnected } from "./engine-connection"

describe("engine connection events", () => {
  it("reports a connection but not a loss", async () => {
    const handler = vi.fn()
    const stop = onEngineConnected(handler)
    await vi.waitFor(() => expect(events.handlers).toHaveLength(1))
    events.handlers[0]({ payload: { connected: false, message: "ended" } })
    expect(handler).not.toHaveBeenCalled()
    events.handlers[0]({ payload: { connected: true, message: null } })
    expect(handler).toHaveBeenCalledOnce()
    stop()
    expect(events.unlisten).toHaveBeenCalledOnce()
  })
})
