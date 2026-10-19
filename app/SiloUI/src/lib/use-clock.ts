import { useSyncExternalStore } from "react"

const TICK_MS = 1000

const listeners = new Set<() => void>()
let timer: number | undefined
let tickedAt = Date.now()

function subscribe(listener: () => void) {
  if (listeners.size === 0) {
    tickedAt = Date.now()
    timer = window.setInterval(() => {
      tickedAt = Date.now()
      for (const notify of [...listeners]) notify()
    }, TICK_MS)
  }
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0 && timer !== undefined) {
      window.clearInterval(timer)
      timer = undefined
    }
  }
}

// Without subscribers the last tick is stale, so a snapshot read then is the current second.
function getNow() {
  return listeners.size > 0 ? tickedAt : Math.floor(Date.now() / TICK_MS) * TICK_MS
}

const subscribeNever = () => () => {}

/** The current time, refreshed every second by one interval shared by every subscribed component. */
export function useClock(active = true) {
  return useSyncExternalStore(active ? subscribe : subscribeNever, getNow)
}
