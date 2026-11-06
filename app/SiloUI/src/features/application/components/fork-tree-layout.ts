import type { ComputerTreeNode } from "@/desktop/production-schemas"

/** Nodes in parent → children order with their depth; a node whose parent is missing is a root. */
export function arrangeTree(nodes: readonly ComputerTreeNode[]) {
  const ids = new Set(nodes.map(node => node.id))
  const children = new Map<string | null, ComputerTreeNode[]>()
  for (const node of nodes) {
    const key = node.parent && ids.has(node.parent) ? node.parent : null
    children.set(key, [...(children.get(key) ?? []), node])
  }
  const rows: { node: ComputerTreeNode; depth: number }[] = []
  const seen = new Set<string>()
  const visit = (key: string | null, depth: number) => {
    for (const node of children.get(key) ?? []) {
      if (seen.has(node.id)) continue
      seen.add(node.id)
      rows.push({ node, depth })
      visit(node.id, depth + 1)
    }
  }
  visit(null, 0)
  return rows
}
