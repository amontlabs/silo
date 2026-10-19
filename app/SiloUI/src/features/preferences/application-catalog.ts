import { createContext, createElement, useContext, useEffect, useEffectEvent, useRef, useState, type ReactNode } from "react"
export type ApplicationKind = "terminal" | "editor" | "browser"
export interface Application { name: string; path: string; icon?: string }
export function matchesApplication(application: Application, savedName: string) {
  // Older settings used labels such as iTerm and Visual Studio Code; bundle
  // metadata can call the same installed apps iTerm2 and Code.
  const bundleName = application.path.split("/").at(-1)?.replace(/\.app$/i, "")
  return application.name === savedName || bundleName === savedName
}
export interface ApplicationCatalog {
  terminal: Application[]
  editor: Application[]
  browser: Application[]
  defaults: Partial<Record<ApplicationKind, string>>
}
export interface ApplicationService {
  read: () => Promise<ApplicationCatalog>
  choose: (kind: ApplicationKind) => Promise<Application | null>
}

const emptyCatalog: ApplicationCatalog = { terminal: [], editor: [], browser: [], defaults: {} }
const fallback = {
  catalog: emptyCatalog,
  available: false,
  /** Whether the installed applications have been read, or were supplied up front. */
  loaded: true,
  refresh: async () => {},
  choose: async (_kind: ApplicationKind): Promise<Application | null> => null,
}
const ApplicationContext = createContext(fallback)

export function ApplicationCatalogProvider({ initialCatalog = emptyCatalog, service, children }: {
  initialCatalog?: ApplicationCatalog
  service?: ApplicationService
  children: ReactNode
}) {
  const [catalog, setCatalog] = useState(initialCatalog)
  const [loaded, setLoaded] = useState(initialCatalog !== emptyCatalog || !service)
  const revision = useRef(0)

  async function refresh() {
    if (!service) return
    const request = ++revision.current
    try {
      const next = await service.read()
      if (request === revision.current) { setCatalog(next); setLoaded(true) }
    } catch (error) { console.error("Silo applications:", error); if (request === revision.current) setLoaded(true) }
  }

  const refreshOnFocus = useEffectEvent(refresh)
  useEffect(() => {
    const requests = revision
    const focus = () => { void refreshOnFocus() }
    window.addEventListener("focus", focus)
    return () => { window.removeEventListener("focus", focus); requests.current++ }
  }, [])

  async function choose(kind: ApplicationKind) {
    if (!service) return null
    const application = await service.choose(kind)
    if (application) {
      revision.current++
      setCatalog((current) => ({ ...current, [kind]: [
        ...current[kind].filter(({ path }) => path !== application.path), application,
      ] }))
    }
    return application
  }

  // These are event handlers; their request counter is never accessed during render.
  // oxlint-disable-next-line react/refs
  return createElement(ApplicationContext.Provider, { value: { catalog, available: Boolean(service), loaded, refresh, choose } }, children)
}

export function useApplications() { return useContext(ApplicationContext) }
