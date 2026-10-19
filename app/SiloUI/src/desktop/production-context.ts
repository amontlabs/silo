import type { ProductionBridge, ProductionSnapshot } from "./production-types"

/** What the parts of a production source share: the native bridge, the base state and how to replace it. */
export interface ProductionContext {
  native: ProductionBridge
  snapshot: () => ProductionSnapshot
  publish: (next: ProductionSnapshot) => void
  disposed: () => boolean
}
