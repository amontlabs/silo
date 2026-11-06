import { useEffect, useEffectEvent, useState } from "react"
import { SectionHeading } from "@/components/page"
import type { ComputerTreeNode, TreeFrom } from "@/desktop/production-schemas"

function fromLabel(from: TreeFrom) {
  switch (from.kind) {
    case "step": return `from step ${from.number}`
    case "checkpoint": return "from a checkpoint"
    case "main": return "from main"
    case "recipe": return "from a recipe"
    case "folder": return "from a folder"
    default: return ""
  }
}

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

/** A compact view of every computer on the device under the computer it was forked from. */
export function ForkTree({ read, deviceId, currentId }: {
  read: (deviceId?: string) => Promise<ComputerTreeNode[]>
  deviceId?: string
  currentId: string
}) {
  const [nodes, setNodes] = useState<ComputerTreeNode[]>([])
  const load = useEffectEvent(() => read(deviceId))
  useEffect(() => {
    let current = true
    void load().then(value => { if (current) setNodes(value) }, () => { if (current) setNodes([]) })
    return () => { current = false }
  }, [deviceId, currentId])
  const rows = arrangeTree(nodes)
  if (rows.length < 2) return null
  return <div className="grid gap-1.5 pt-2">
    <SectionHeading>Fork tree</SectionHeading>
    <ul aria-label="Fork tree" className="grid gap-0.5 rounded-md border border-border p-2">
      {rows.map(({ node, depth }) => <li key={node.id} data-tree-node={node.name} style={{ paddingLeft: `${depth * 16}px` }} className="flex flex-wrap items-baseline gap-x-1.5">
        <span className={node.id === currentId ? "font-medium" : undefined}>{node.name}</span>
        <span className="text-muted-foreground">{[node.state, fromLabel(node.from)].filter(Boolean).join(" · ")}</span>
        {node.pendingChanges.requestId && <span className="text-warning">Review requested</span>}
      </li>)}
    </ul>
  </div>
}
