import { Suspense, useEffect, useState, type ReactNode } from "react"

/** True from the first time `active` holds, so a panel mounts on its first visit and then stays mounted. */
function useEverActive(active: boolean) {
  const [seen, setSeen] = useState(active)
  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    if (active) setSeen(true)
  }, [active])
  return active || seen
}

/** Renders its children only after the panel was first shown; the fallback is empty so nothing flashes. */
export function PanelContent({ active, children }: { active: boolean; children: ReactNode }) {
  const mounted = useEverActive(active)
  return mounted ? <Suspense fallback={null}>{children}</Suspense> : null
}
