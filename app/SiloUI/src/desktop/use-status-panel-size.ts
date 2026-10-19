import { useLayoutEffect, type RefObject } from "react"
import { invoke } from "@tauri-apps/api/core"

/** The tallest the status panel grows; keep equal to `--status-panel-height` in index.css. */
export const STATUS_PANEL_MAX_HEIGHT = 520

export function useStatusPanelSize(content: RefObject<HTMLDivElement | null>) {
  useLayoutEffect(() => {
    if (!content.current) return
    const element = content.current
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)")
    let disposed = false
    let running = false
    let current: number | undefined
    let target = 0
    let page: Element | null = null
    let frame = 0

    // A CSS height transition can outrun IPC. Resize the native window before
    // displaying each frame, with at most one native request in flight.
    async function resize() {
      if (running || disposed) return
      running = true
      try {
        while (!disposed && current !== target) {
          const destination = target
          const start = current ?? destination
          const instant = current === undefined || motion.matches || element.dataset.reduceMotion === "true"
          const started = performance.now()
          let progress = 0
          do {
            if (!instant) await new Promise<void>((resolve) => { frame = requestAnimationFrame(() => resolve()) })
            if (disposed) return
            if (target !== destination) break
            progress = instant ? 1 : Math.min((performance.now() - started) / 160, 1)
            const height = Math.round(start + (destination - start) * (1 - (1 - progress) ** 3))
            if (height !== current) {
              await invoke("resize_status", { height })
              if (disposed) return
              element.style.height = `${height}px`
              current = height
            }
          } while (progress < 1)
        }
      } catch (error) {
        if (disposed) return
        console.error("Silo status resize:", error)
        element.style.height = ""
        current = undefined
      } finally {
        running = false
      }
    }

    function measure() {
      if (!page) return
      const border = element.offsetHeight - element.clientHeight
      target = Math.ceil(Math.min(STATUS_PANEL_MAX_HEIGHT, Math.max(1, page.getBoundingClientRect().height + border)))
      void resize()
    }
    const observer = new ResizeObserver(measure)
    function observePage() {
      const next = element.firstElementChild
      if (next !== page) {
        observer.disconnect()
        page = next
        if (page) observer.observe(page)
      }
      measure()
    }
    const mutations = new MutationObserver(observePage)
    mutations.observe(element, { childList: true, attributes: true, attributeFilter: ["data-reduce-motion"] })
    motion.addEventListener("change", measure)
    observePage()
    return () => {
      disposed = true
      cancelAnimationFrame(frame)
      observer.disconnect()
      mutations.disconnect()
      motion.removeEventListener("change", measure)
    }
  }, [content])
}
