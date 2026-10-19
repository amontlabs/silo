import { bridgeErrorMessage } from "@/contracts/bridge-error"

/**
 * Readable text for a thrown value: a native bridge error's message, an Error's message, or the
 * value as a string. With `fallback`, only non-blank strings and Error messages are used and
 * anything else (blank text, other objects) yields the fallback.
 */
export function errorMessage(error: unknown, options: { fallback?: string } = {}): string {
  const bridge = bridgeErrorMessage(error)
  if (bridge) return bridge
  const { fallback } = options
  if (fallback === undefined) return error instanceof Error ? error.message : String(error)
  const text = error instanceof Error ? error.message.trim() : typeof error === "string" ? error.trim() : ""
  return text || fallback
}
