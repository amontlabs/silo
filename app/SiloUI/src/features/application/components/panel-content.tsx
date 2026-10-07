import { Component, Suspense, useEffect, useState, type ErrorInfo, type ReactNode } from "react"

import { Button } from "@/components/ui/button"
import { renewFailedPages } from "./lazy-page"

class LoadBoundary extends Component<{ children: ReactNode }, { failed: boolean; attempt: number }> {
  state = { failed: false, attempt: 0 }
  static getDerivedStateFromError() { return { failed: true } }
  componentDidCatch(error: Error, info: ErrorInfo) { console.error("Silo page failed to load:", error, info.componentStack) }
  render() {
    if (!this.state.failed) return <div key={this.state.attempt} className="contents">{this.props.children}</div>
    return <div role="alert" className="mx-auto flex w-full max-w-4xl items-center gap-2 px-4 py-5 text-xs text-destructive sm:px-6 sm:py-6">
      <span>This page could not be loaded.</span>
      <Button type="button" size="xs" variant="outline" onClick={() => { renewFailedPages(); this.setState(({ attempt }) => ({ failed: false, attempt: attempt + 1 })) }}>Retry</Button>
    </div>
  }
}

/** Shows nothing while a lazily loaded page arrives, and offers Retry if it cannot be loaded. */
export function LazyBoundary({ children }: { children: ReactNode }) {
  return <LoadBoundary><Suspense fallback={null}>{children}</Suspense></LoadBoundary>
}

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
  return mounted ? <LazyBoundary>{children}</LazyBoundary> : null
}
