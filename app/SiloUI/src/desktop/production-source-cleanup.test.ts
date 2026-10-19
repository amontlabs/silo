import { expect, it, vi } from "vitest"
import { createProductionSource, type ProductionBridge } from "./production-source"

it.each([1, 3, 6])("releases listeners exactly once when disposed during registration %s", async pendingAt => {
  let register!: (stop: () => void) => void
  const pending = new Promise<() => void>(resolve => { register = resolve })
  const stops: Array<() => void> = []
  const native = {
    invoke: vi.fn().mockResolvedValue(undefined),
    listen: vi.fn(() => {
      const stop = vi.fn()
      stops.push(stop)
      return stops.length === pendingAt ? pending : Promise.resolve(stop)
    }),
  }
  const store = createProductionSource(native as ProductionBridge)
  const initialized = store.initialize()
  await vi.waitFor(() => expect(native.listen).toHaveBeenCalledTimes(7))
  store.dispose()
  register(stops[pendingAt - 1])
  await initialized
  expect(native.listen).toHaveBeenCalledTimes(7)
  for (const stop of stops) expect(stop).toHaveBeenCalledOnce()
  expect(native.invoke).not.toHaveBeenCalled()
})
