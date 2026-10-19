import { useState } from "react"
import { act, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

const applications = vi.hoisted(() => ({ useApplications: vi.fn() }))
vi.mock("@/features/preferences/application-catalog", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/features/preferences/application-catalog")>(),
  ...applications,
}))

import { ApplicationPreferenceFields } from "./application-preference-fields"
import type { ApplicationPreferenceSelection } from "@/features/preferences/model/application-preferences"

const initialSelection: ApplicationPreferenceSelection = { terminal: "Terminal", editor: "Cursor", browser: "Firefox" }
const testIcon = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII="

function setup(value: ApplicationPreferenceSelection = initialSelection) {
  const onChange = vi.fn()
  function Fields() {
    const [current, setCurrent] = useState(value)
    return <ApplicationPreferenceFields value={current} onChange={(next) => { onChange(next); setCurrent(next) }} />
  }
  const view = render(<Fields />)
  return { user: userEvent.setup(), onChange, refreshView: () => view.rerender(<Fields />), unmount: view.unmount }
}

function catalog() {
  return {
    terminal: [{ name: "Terminal", path: "/System/Applications/Utilities/Terminal.app" }, { name: "iTerm", path: "/Applications/iTerm.app" }],
    editor: [{ name: "Cursor", path: "/Applications/Cursor.app" }],
    browser: [{ name: "Firefox", path: "/Applications/Firefox.app" }],
    defaults: { terminal: "/System/Applications/Utilities/Terminal.app", editor: "/Applications/Cursor.app", browser: "/Applications/Firefox.app" },
  }
}

beforeEach(() => {
  applications.useApplications.mockReturnValue({ catalog: catalog(), refresh: vi.fn().mockResolvedValue(undefined), choose: vi.fn().mockResolvedValue(null), available: true, loaded: true })
})

describe("application preference choices", () => {
  it("keeps the selects disabled and neutral until the applications have been read", () => {
    applications.useApplications.mockReturnValue({ catalog: { terminal: [], editor: [], browser: [], defaults: {} }, refresh: vi.fn().mockResolvedValue(undefined), choose: vi.fn(), available: true, loaded: false })
    setup()
    for (const name of ["Terminal", "Code editor"]) expect(screen.getByRole("combobox", { name })).toBeDisabled()
    expect(screen.queryByText(/unavailable/)).not.toBeInTheDocument()
  })

  it("shows the current system default and its icon as the first option and selected value", async () => {
    const source = applications.useApplications()
    source.catalog.editor[0].icon = testIcon
    const { user } = setup({ ...initialSelection, editorUseSystemDefault: true })
    const editor = screen.getByRole("combobox", { name: "Code editor" })
    expect(editor).toHaveTextContent("Cursor (default)")
    expect(editor.querySelector("img")).toHaveAttribute("src", testIcon)
    expect(editor.querySelectorAll("img")).toHaveLength(1)
    await user.click(editor)
    const option = screen.getAllByRole("option")[0]
    expect(option).toHaveAccessibleName("System default (Cursor)")
    expect(option).toHaveAttribute("data-state", "checked")
    expect(option.querySelector("img")).toHaveAttribute("src", testIcon)
  })

  it("switches between following the system default and an exact custom application", async () => {
    const source = applications.useApplications()
    source.catalog.editor.push({ name: "Zed", path: "/Applications/Zed.app" })
    source.catalog.defaults.editor = "/Applications/Zed.app"
    const { user, onChange } = setup()
    await user.click(screen.getByRole("combobox", { name: "Code editor" }))
    await user.click(screen.getByRole("option", { name: "System default (Zed)" }))
    expect(onChange).toHaveBeenLastCalledWith({ ...initialSelection, editorUseSystemDefault: true })
    expect(screen.getByRole("combobox", { name: "Code editor" })).toHaveTextContent("Zed (default)")
    await user.click(screen.getByRole("combobox", { name: "Code editor" }))
    await user.click(screen.getByRole("option", { name: "Cursor" }))
    expect(onChange).toHaveBeenLastCalledWith({ ...initialSelection, editorPath: "/Applications/Cursor.app", editorUseSystemDefault: false })
    expect(screen.getByRole("combobox", { name: "Code editor" })).toHaveTextContent("Cursor")
    expect(screen.getByRole("combobox", { name: "Code editor" })).not.toHaveTextContent("System default")
  })

  it("shows an unset system default as disabled even when it is the selected mode", async () => {
    delete applications.useApplications().catalog.defaults.editor
    const { user, onChange } = setup({ ...initialSelection, editorUseSystemDefault: true })
    const editor = screen.getByRole("combobox", { name: "Code editor" })
    expect(editor).toHaveTextContent("System default (not set)")
    await user.click(editor)
    const system = screen.getByRole("option", { name: "System default (not set)" })
    expect(system).toHaveAttribute("aria-disabled", "true")
    expect(system).toHaveAttribute("data-state", "checked")
    expect(onChange).not.toHaveBeenCalled()
  })

  it("follows a refreshed default catalog without changing the saved system-default mode", async () => {
    const source = applications.useApplications()
    const { user, onChange, refreshView } = setup({ ...initialSelection, editorUseSystemDefault: true })
    source.refresh.mockImplementation(async () => {
      source.catalog.editor.push({ name: "Zed", path: "/Applications/Zed.app" })
      source.catalog.defaults.editor = "/Applications/Zed.app"
    })
    const editor = screen.getByRole("combobox", { name: "Code editor" })
    await user.click(editor)
    refreshView()
    expect(editor).toHaveTextContent("Zed (default)")
    expect(screen.getByRole("option", { name: "System default (Zed)" })).toHaveAttribute("data-state", "checked")
    expect(onChange).not.toHaveBeenCalled()
  })

  it("keeps system-default mode when the native chooser is cancelled", async () => {
    const { user, onChange } = setup({ ...initialSelection, browserUseSystemDefault: true })
    await user.click(screen.getByRole("combobox", { name: "Browser" }))
    await user.click(screen.getByRole("option", { name: "Choose…" }))
    expect(onChange).not.toHaveBeenCalled()
    expect(screen.getByRole("combobox", { name: "Browser" })).toHaveTextContent("Firefox (default)")
  })

  it("shows decorative app icons in choices and the selected value without changing accessible names", async () => {
    const icon = testIcon
    applications.useApplications().catalog.editor[0].icon = icon
    const { user } = setup()
    const editor = screen.getByRole("combobox", { name: "Code editor" })
    const selectedIcon = editor.querySelector("img")
    expect(selectedIcon).toHaveAttribute("src", icon)
    expect(selectedIcon).toHaveAttribute("alt", "")
    expect(selectedIcon).toHaveAttribute("aria-hidden", "true")
    expect(selectedIcon).toHaveClass("size-4", "shrink-0", "object-contain")
    await user.click(editor)
    const option = screen.getByRole("option", { name: "Cursor" })
    expect(option.querySelector("img")).toHaveAttribute("src", icon)
    expect(option).toHaveAccessibleName("Cursor")
  })

  it("uses a decorative category fallback for apps without icons and unavailable saved apps", async () => {
    const { user } = setup({ ...initialSelection, editorPath: "/removed/Cursor.app" })
    const editor = screen.getByRole("combobox", { name: "Code editor" })
    expect(editor.querySelector("svg.size-4")).toHaveAttribute("aria-hidden", "true")
    await user.click(editor)
    expect(screen.getByRole("option", { name: "Cursor" }).querySelector("svg.size-4")).toHaveAttribute("aria-hidden", "true")
    expect(screen.getByRole("option", { name: "Cursor (unavailable)" }).querySelector("svg.size-4")).toHaveAttribute("aria-hidden", "true")
  })

  it("refreshes installed choices on open and saves the selected name and exact path", async () => {
    const { user, onChange } = setup()
    const source = applications.useApplications()
    expect(screen.getByRole("combobox", { name: "Terminal" })).toHaveTextContent("Terminal")
    await user.click(screen.getByRole("combobox", { name: "Terminal" }))
    expect(source.refresh).toHaveBeenCalledOnce()
    expect(screen.getAllByRole("option").map(({ textContent }) => textContent)).toEqual(["System default (Terminal)", "Terminal", "iTerm", "Choose…"])
    expect(screen.queryByRole("option", { name: "Warp" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("option", { name: "iTerm" }))
    expect(onChange).toHaveBeenCalledExactlyOnceWith({ ...initialSelection, terminal: "iTerm", terminalPath: "/Applications/iTerm.app", terminalUseSystemDefault: false })
    expect(screen.getByRole("combobox", { name: "Terminal" })).toHaveTextContent("iTerm")
  })

  it("uses exact saved paths to distinguish installed apps with the same name", async () => {
    const source = applications.useApplications()
    source.catalog.editor = [{ name: "Code", path: "/Applications/Code.app" }, { name: "Code", path: "/Users/example/Applications/Code.app" }]
    const { user, onChange } = setup({ ...initialSelection, editor: "Code", editorPath: "/Users/example/Applications/Code.app" })
    await user.click(screen.getByRole("combobox", { name: "Code editor" }))
    const choices = screen.getAllByRole("option", { name: "Code" })
    expect(choices[0]).toHaveAttribute("data-state", "unchecked")
    expect(choices[1]).toHaveAttribute("data-state", "checked")
    await user.click(choices[0])
    expect(onChange).toHaveBeenCalledWith({ ...initialSelection, editor: "Code", editorPath: "/Applications/Code.app", editorUseSystemDefault: false })
  })

  it("matches a legacy saved name to its installed bundle basename without rewriting the preference", async () => {
    const source = applications.useApplications()
    source.catalog.editor = [{ name: "Code", path: "/Applications/Visual Studio Code.app" }]
    const { user, onChange } = setup({ ...initialSelection, editor: "Visual Studio Code" })
    const editor = screen.getByRole("combobox", { name: "Code editor" })
    expect(editor).not.toHaveTextContent("unavailable")
    await user.click(editor)
    expect(screen.getByRole("option", { name: "Code" })).toHaveAttribute("data-state", "checked")
    expect(onChange).not.toHaveBeenCalled()
  })

  it("keeps a missing saved path disabled instead of selecting another app with its name", async () => {
    const { user, onChange } = setup({ ...initialSelection, editorPath: "/removed/Cursor.app" })
    const editor = screen.getByRole("combobox", { name: "Code editor" })
    expect(editor).toHaveTextContent("Cursor (unavailable)")
    expect(editor).not.toHaveTextContent("/removed/")
    await user.click(editor)
    expect(screen.getByRole("option", { name: "Cursor (unavailable)" })).toHaveAttribute("aria-disabled", "true")
    await user.click(screen.getByRole("option", { name: "Cursor" }))
    expect(onChange).toHaveBeenCalledWith({ ...initialSelection, editorPath: "/Applications/Cursor.app", editorUseSystemDefault: false })
  })

  it("saves a native chooser result and displays its name without the filesystem path", async () => {
    const source = applications.useApplications()
    const chosen = { name: "Nova", path: "/Users/example/Applications/Nova.app" }
    source.choose.mockImplementation(async () => {
      source.catalog.editor.push(chosen)
      return chosen
    })
    const { user, onChange } = setup({ ...initialSelection, editorUseSystemDefault: true })
    await user.click(screen.getByRole("combobox", { name: "Code editor" }))
    await user.click(screen.getByRole("option", { name: "Choose…" }))
    expect(source.choose).toHaveBeenCalledExactlyOnceWith("editor")
    expect(onChange).toHaveBeenCalledExactlyOnceWith({ ...initialSelection, editor: "Nova", editorPath: chosen.path, editorUseSystemDefault: false })
    expect(screen.getByRole("combobox", { name: "Code editor" })).toHaveTextContent("Nova")
    expect(screen.getByRole("combobox", { name: "Code editor" })).not.toHaveTextContent(chosen.path)
  })

  it("keeps the current choice when the native chooser is cancelled or fails", async () => {
    const source = applications.useApplications()
    const { user, onChange } = setup()
    await user.click(screen.getByRole("combobox", { name: "Browser" }))
    await user.click(screen.getByRole("option", { name: "Choose…" }))
    expect(onChange).not.toHaveBeenCalled()
    expect(screen.getByRole("combobox", { name: "Browser" })).toHaveTextContent("Firefox")
    source.choose.mockRejectedValueOnce("The selected item is not an available application")
    await user.click(screen.getByRole("combobox", { name: "Browser" }))
    await user.click(screen.getByRole("option", { name: "Choose…" }))
    expect(onChange).not.toHaveBeenCalled()
    expect(screen.getByRole("combobox", { name: "Browser" })).toHaveTextContent("Firefox")
    // The failure is explained beside the select instead of only in the console.
    const alert = screen.getByRole("alert")
    expect(alert).toHaveTextContent("Could not use the chosen browser. The selected item is not an available application.")
    expect(screen.getByRole("combobox", { name: "Browser" })).toHaveAccessibleDescription(alert.textContent!)
    await user.click(screen.getByRole("combobox", { name: "Browser" }))
    await user.click(screen.getByRole("option", { name: "System default (Firefox)" }))
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })

  it("disables the native chooser in browser fixtures while preserving fixture options", async () => {
    const source = applications.useApplications()
    source.available = false
    const { user } = setup()
    await user.click(screen.getByRole("combobox", { name: "Terminal" }))
    expect(screen.getByRole("option", { name: "Choose…" })).toHaveAttribute("aria-disabled", "true")
    expect(screen.getByRole("option", { name: "iTerm" })).not.toHaveAttribute("aria-disabled")
    expect(source.choose).not.toHaveBeenCalled()
  })
})


function pendingChoice() {
  let resolve!: (value: { name: string; path: string }) => void
  let reject!: (cause: Error) => void
  const promise = new Promise<{ name: string; path: string }>((done, fail) => { resolve = done; reject = fail })
  applications.useApplications().choose.mockReturnValueOnce(promise)
  return { resolve, reject }
}

it("keeps another preference's newer selection when an earlier chooser returns", async () => {
  const pending = pendingChoice()
  const { user, onChange } = setup()
  await user.click(screen.getByRole("combobox", { name: "Code editor" }))
  await user.click(screen.getByRole("option", { name: "Choose…" }))
  await user.click(screen.getByRole("combobox", { name: "Terminal" }))
  await user.click(screen.getByRole("option", { name: "iTerm" }))
  await act(async () => pending.resolve({ name: "Nova", path: "/Applications/Nova.app" }))
  expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ terminal: "iTerm", editor: "Nova" }))
  expect(screen.getByRole("combobox", { name: "Terminal" })).toHaveTextContent("iTerm")
})

it.each(["resolve", "reject"] as const)("ignores an older chooser that will %s after a newer selection", async settle => {
  const pending = pendingChoice()
  const { user, onChange } = setup()
  await user.click(screen.getByRole("combobox", { name: "Code editor" }))
  await user.click(screen.getByRole("option", { name: "Choose…" }))
  await user.click(screen.getByRole("combobox", { name: "Code editor" }))
  await user.click(screen.getByRole("option", { name: "System default (Cursor)" }))
  await act(async () => {
    if (settle === "resolve") pending.resolve({ name: "Nova", path: "/Applications/Nova.app" })
    else pending.reject(new Error("Old chooser failed"))
  })
  expect(onChange).toHaveBeenCalledOnce()
  expect(screen.getByRole("combobox", { name: "Code editor" })).toHaveTextContent("Cursor (default)")
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("does not save a chooser result after its preference fields unmount", async () => {
  const pending = pendingChoice()
  const { user, onChange, unmount } = setup()
  await user.click(screen.getByRole("combobox", { name: "Code editor" }))
  await user.click(screen.getByRole("option", { name: "Choose…" }))
  unmount()
  await act(async () => pending.resolve({ name: "Nova", path: "/Applications/Nova.app" }))
  expect(onChange).not.toHaveBeenCalled()
})
