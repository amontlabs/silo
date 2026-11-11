import { listen } from "@tauri-apps/api/event"

const ENGINE_CONNECTION_EVENT = "silo://engine-connection"

/** Calls `handler` each time the app's connection to the engine is made. Returns the stop function. */
export function onEngineConnected(handler: () => void): () => void {
  let stopped = false
  let stop: (() => void) | undefined
  Promise.resolve()
    .then(() => listen<{ connected?: boolean }>(ENGINE_CONNECTION_EVENT, (event) => { if (event.payload?.connected === true) handler() }))
    .then((unlisten) => { if (stopped) unlisten(); else stop = unlisten })
    .catch((error: unknown) => console.error("Silo engine connection:", error))
  return () => { stopped = true; stop?.(); stop = undefined }
}
