import { useEffect } from "react"
import type { ApplicationActions } from "../model/application-source"

export function useSshAccessRefresh(refresh: ApplicationActions["refreshSshAccess"], active = true) {
  useEffect(() => {
    if (!active || !refresh) return
    const update = (background = false) => { if (document.visibilityState !== "hidden") void refresh({ background }) }
    // Focus and visibility changes arrive together when the window returns: one refresh covers both.
    let lastReturn = 0
    const onReturn = () => {
      if (Date.now() - lastReturn < 1000) return
      lastReturn = Date.now()
      update()
    }
    update()
    const timer = window.setInterval(() => update(true), 12_000)
    window.addEventListener("focus", onReturn)
    document.addEventListener("visibilitychange", onReturn)
    return () => { window.clearInterval(timer); window.removeEventListener("focus", onReturn); document.removeEventListener("visibilitychange", onReturn) }
  }, [active, refresh])
}

