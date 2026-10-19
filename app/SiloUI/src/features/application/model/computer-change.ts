import { errorMessage } from "@/lib/error-message"
import type { SetupComputerConfiguration } from "@/contracts/silo"

/**
 * One targeted change to the local computer inventory, matching the backend
 * `change_computer_configuration` command. `expected` (and `expectedOrder`) carry the
 * configuration the edit started from so the backend can apply the change to fresh
 * state — or reject it if the computer changed while the request waited its turn — instead
 * of overwriting concurrent work with a stale whole-list snapshot.
 */
export type ComputerConfigurationChange =
  | { kind: "upsert"; configuration: SetupComputerConfiguration; expected: SetupComputerConfiguration | null }
  | { kind: "delete"; computerId: string; expected: SetupComputerConfiguration }
  | { kind: "reorder"; order: string[]; expectedOrder: string[] }
  | { kind: "batch"; changes: ComputerConfigurationChange[] }

/** Field-order-independent structural comparison of two computer configurations. */
function stableStringify(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value) ?? "null"
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`
  const entries = Object.entries(value as Record<string, unknown>)
    .filter(([, entry]) => entry !== undefined)
    .sort(([a], [b]) => a.localeCompare(b))
  return `{${entries.map(([key, entry]) => `${JSON.stringify(key)}:${stableStringify(entry)}`).join(",")}}`
}

function sameComputer(a: SetupComputerConfiguration, b: SetupComputerConfiguration | undefined): boolean {
  return b !== undefined && stableStringify(a) === stableStringify(b)
}

/**
 * Field-order-independent equality of two computer configurations. Used by the editor
 * to notice, while a form is open, that the committed configuration diverged from the
 * baseline the user started editing from.
 */
export function sameComputerConfiguration(
  a: SetupComputerConfiguration | undefined,
  b: SetupComputerConfiguration | undefined,
): boolean {
  if (a === undefined || b === undefined) return a === b
  return stableStringify(a) === stableStringify(b)
}

/** The keys whose values differ between two computer configurations. */
export function divergentComputerFields(
  a: SetupComputerConfiguration,
  b: SetupComputerConfiguration,
): string[] {
  const aValues = new Map<string, unknown>(Object.entries(a))
  const bValues = new Map<string, unknown>(Object.entries(b))
  const keys = new Set([...aValues.keys(), ...bValues.keys()])
  return [...keys].filter((key) => {
    const aValue = aValues.get(key)
    const bValue = bValues.get(key)
    return (aValue === undefined) !== (bValue === undefined) || stableStringify(aValue) !== stableStringify(bValue)
  })
}

/**
 * Recognize the backend's optimistic-concurrency rejection, raised when a targeted
 * change's `expected` no longer matches the computer's saved configuration because it changed
 * while the user's edit waited. The backend phrases both the per-computer and reorder variants
 * with this stem, so match on it rather than on the whole sentence.
 */
export function isStaleConfigurationError(error: unknown): boolean {
  return errorMessage(error).includes("changed while your edit was waiting")
}

/**
 * Reduce an edited local computer list to the targeted changes needed to turn the committed
 * configuration `previous` into `next`, each carrying the state it started from so the
 * backend applies it to fresh state (or rejects it) instead of overwriting concurrent
 * work with a stale whole-list snapshot.
 *
 * Returns an ordered list: deletions, then creations (`expected: null` = must not yet
 * exist), then in-place edits, or a single reorder when only the order changed. An empty
 * list means the submission is a no-op and the backend need not be called. The caller
 * sends one change on its own or several as a `batch`. The UI applies one
 * create/edit/delete/reorder at a time; onboarding submits several creations at once.
 */
export function deriveComputerChanges(
  previous: SetupComputerConfiguration[],
  next: SetupComputerConfiguration[],
): ComputerConfigurationChange[] {
  const prevById = new Map(previous.map((configuration) => [configuration.id, configuration]))
  const nextById = new Map(next.map((configuration) => [configuration.id, configuration]))
  const added = next.filter((configuration) => !prevById.has(configuration.id))
  const removed = previous.filter((configuration) => !nextById.has(configuration.id))
  const changed = next.filter((configuration) => prevById.has(configuration.id) && !sameComputer(configuration, prevById.get(configuration.id)))

  const changes: ComputerConfigurationChange[] = []
  for (const configuration of removed) changes.push({ kind: "delete", computerId: configuration.id, expected: configuration })
  for (const configuration of added) changes.push({ kind: "upsert", configuration, expected: null })
  for (const configuration of changed) changes.push({ kind: "upsert", configuration, expected: prevById.get(configuration.id) ?? null })

  if (added.length === 0 && removed.length === 0 && changed.length === 0) {
    const order = next.map((configuration) => configuration.id)
    const expectedOrder = previous.map((configuration) => configuration.id)
    if (order.length === expectedOrder.length && order.some((id, index) => id !== expectedOrder[index])) {
      changes.push({ kind: "reorder", order, expectedOrder })
    }
  }
  return changes
}
