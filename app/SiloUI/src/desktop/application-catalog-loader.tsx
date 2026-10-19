import { useEffect, useEffectEvent } from "react"

import { useApplications } from "@/features/preferences/application-catalog"

/** Reads the installed applications once mounted. The defaults they resolve reach the settings when the read finishes. */
export function ApplicationCatalogLoader() {
  const { refresh } = useApplications()
  const load = useEffectEvent(refresh)
  useEffect(() => { void load() }, [])
  return null
}
