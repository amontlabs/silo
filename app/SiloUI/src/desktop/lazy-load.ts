/** Runs `load`, and once more after `delay` milliseconds if it fails, since a module request can fail transiently. */
export function loadWithRetry<T>(load: () => Promise<T>, delay = 400): () => Promise<T> {
  return () => load().catch(async (error: unknown) => {
    console.error("Silo could not load part of the app; trying once more:", error)
    await new Promise(resolve => setTimeout(resolve, delay))
    return load()
  })
}
