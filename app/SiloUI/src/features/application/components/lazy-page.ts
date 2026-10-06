import { lazy, type ComponentType } from "react"

export interface LazyPage<Component> {
  Component: Component
  preload: () => Promise<unknown>
}

/** A named export loaded on first render, whose chunk `preload` can fetch ahead of time. */
export function lazyPage<Module, Name extends keyof Module>(load: () => Promise<Module>, name: Name): LazyPage<Module[Name]> {
  let pending: Promise<{ default: Module[Name] }> | undefined
  const resolve = () => (pending ??= load().then((module) => ({ default: module[name] })))
  // The loaded export is a component typed by its own module.
  return { Component: lazy(resolve as unknown as () => Promise<{ default: ComponentType }>) as unknown as Module[Name], preload: resolve }
}

/** Fetches the given chunks once the main thread is idle, so a first visit does not wait on the network. */
export function preloadWhenIdle(pages: readonly { preload: () => Promise<unknown> }[]) {
  const run = () => { for (const page of pages) void page.preload().catch(() => {}) }
  if (typeof requestIdleCallback === "function") {
    const handle = requestIdleCallback(run, { timeout: 4000 })
    return () => cancelIdleCallback(handle)
  }
  const handle = setTimeout(run, 1000)
  return () => clearTimeout(handle)
}
