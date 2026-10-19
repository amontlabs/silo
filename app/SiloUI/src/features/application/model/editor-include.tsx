/* oxlint-disable react/only-export-components */
import { errorMessage } from "@/lib/error-message"
import { createContext, useContext, useEffect, useState, type ReactNode } from "react"

/**
 * The SSH `Include` line Silo could not add to the user's own SSH configuration after an upgrade,
 * because that file links to one Silo can't change. Without it, an editor that reconnects by itself
 * still uses the previous storage.
 */
export interface EditorIncludeBackend {
  /** The line the user has to add and has not added yet, or null. */
  read: () => Promise<string | null>
  /** Calls `refresh` when Silo learns the line changed. */
  subscribe: (refresh: () => void) => Promise<() => void>
}

const Backend = createContext<EditorIncludeBackend | undefined>(undefined)

export function EditorIncludeProvider({ backend, children }: { backend: EditorIncludeBackend; children: ReactNode }) {
  return <Backend value={backend}>{children}</Backend>
}

/**
 * The line the user has to add, or null: none is needed, it is not known yet, or there is no
 * provider (windows and previews without a previous storage). Read when mounted, when Silo reports
 * a change, and when the window regains focus, since the user adds the line elsewhere.
 */
export function useEditorIncludeLine(): string | null {
  const backend = useContext(Backend)
  const [line, setLine] = useState<string | null>(null)
  useEffect(() => {
    if (!backend) return
    let live = true
    let sequence = 0
    let unsubscribe: (() => void) | undefined
    const refresh = () => {
      if (!live) return
      const mine = ++sequence
      backend.read().then(
        next => { if (live && mine === sequence) setLine(next) },
        (cause: unknown) => console.error("Silo editor connections:", errorMessage(cause)),
      )
    }
    backend.subscribe(refresh).then(
      stop => { if (live) unsubscribe = stop; else stop() },
      (cause: unknown) => console.error("Silo editor connections:", errorMessage(cause)),
    ).then(() => { if (live) refresh() })
    window.addEventListener("focus", refresh)
    return () => {
      live = false
      unsubscribe?.()
      window.removeEventListener("focus", refresh)
    }
  }, [backend])
  return line
}
