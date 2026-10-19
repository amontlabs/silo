import { act, render } from "@testing-library/react"
import { describe, expect, it } from "vitest"
import { createMemorySettingsStore, SettingsProvider, shallowEqual, useSettingsSelector } from "./settings-store"

describe("useSettingsSelector", () => {
  it("applies a selector that closes over changing props", () => {
    const store = createMemorySettingsStore({ theme: "dark" })
    function Pick({ field }: { field: "theme" | "browser" }) {
      return <p>{String(useSettingsSelector((view) => view.settings[field]))}</p>
    }
    const view = render(<SettingsProvider store={store}><Pick field="theme" /></SettingsProvider>)
    expect(view.getByText("dark")).toBeInTheDocument()
    view.rerender(<SettingsProvider store={store}><Pick field="browser" /></SettingsProvider>)
    expect(view.queryByText("dark")).not.toBeInTheDocument()
    expect(view.getByText(String(store.getSnapshot().settings.browser))).toBeInTheDocument()
  })

  it("re-renders only when the selected value changes", async () => {
    const store = createMemorySettingsStore()
    let renders = 0
    function Theme() {
      renders++
      return <p>{useSettingsSelector((view) => view.settings.theme)}</p>
    }
    const view = render(<SettingsProvider store={store}><Theme /></SettingsProvider>)
    const initial = renders
    await act(() => store.updateSettings({ reduceMotion: true }))
    expect(renders).toBe(initial)
    await act(() => store.updateSettings({ theme: "dark" }))
    expect(view.getByText("dark")).toBeInTheDocument()
    expect(renders).toBe(initial + 1)
  })

  it("keeps the previous selection while the custom comparison holds", async () => {
    const store = createMemorySettingsStore()
    const selections: object[] = []
    function Pair() {
      selections.push(useSettingsSelector((view) => ({ theme: view.settings.theme, motion: view.settings.reduceMotion }), shallowEqual))
      return null
    }
    render(<SettingsProvider store={store}><Pair /></SettingsProvider>)
    await act(() => store.updateSettings({ alphaNoticeDismissed: true }))
    expect(new Set(selections).size).toBe(1)
    await act(() => store.updateSettings({ reduceMotion: true }))
    expect(new Set(selections).size).toBe(2)
  })
})
