import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { ApplicationShell } from "./application-shell"

function renderSidebar() {
  render(<ApplicationShell
    activeTab="computers"
    computerSection="overview"
    settingsSection="general"
    systemIssueStatus={null}
    computerAttention={{ errors: 0, warnings: 0 }}
    onTabChange={vi.fn()}
    onComputerSectionChange={vi.fn()}
    onSettingsSectionChange={vi.fn()}
    canGoBack={false}
    canGoForward={false}
    onGoBack={vi.fn()}
    onGoForward={vi.fn()}
  ><button>Page content</button></ApplicationShell>)
  fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }))
  return {
    toggle: screen.getByRole("button", { name: "Expand sidebar" }),
    sidebar: screen.getByRole("navigation", { name: "Silo navigation" }),
  }
}

function renderAttention(props: Partial<Parameters<typeof ApplicationShell>[0]> = {}) {
  render(<ApplicationShell
    activeTab="computers"
    computerSection="overview"
    settingsSection="general"
    systemIssueStatus={null}
    computerAttention={{ errors: 1, warnings: 2 }}
    onTabChange={vi.fn()}
    onComputerSectionChange={vi.fn()}
    onSettingsSectionChange={vi.fn()}
    canGoBack={false}
    canGoForward={false}
    onGoBack={vi.fn()}
    onGoForward={vi.fn()}
    {...props}
  ><button>Page content</button></ApplicationShell>)
  const computers = () => screen.getByRole("button", { name: "Computers" })
  const mark = () => computers().querySelector("[data-navigation-attention]")
  return { computers, mark }
}

describe("sidebar attention", () => {
  afterEach(cleanup)

  it("counts computers, not errors, next to Overview", () => {
    renderAttention()
    const overview = within(screen.getByRole("button", { name: /^All computers/ }))
    expect(overview.getByRole("status", { name: "1 computer has an error" })).toHaveTextContent("1")
    expect(overview.getByRole("status", { name: "2 computers have warnings" })).toHaveTextContent("2")
  })

  it("mirrors attention and section work on Computers when its menu is collapsed", () => {
    const { computers, mark } = renderAttention({ navigationLoading: { computerSections: { files: true } } })
    expect(mark()).toBeNull()
    expect(computers()).not.toHaveAttribute("aria-busy")
    expect(screen.queryByRole("status", { name: /need attention/ })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Collapse Computers menu" }))
    expect(screen.queryByRole("button", { name: /^All computers/ })).not.toBeInTheDocument()
    expect(screen.getByRole("status", { name: "3 computers need attention" })).toBeInTheDocument()
    expect(computers()).toHaveAccessibleDescription("3 computers need attention")
    expect(mark()).toHaveTextContent("3")
    expect(mark()).toHaveClass("text-destructive")
    expect(computers()).toHaveAttribute("aria-busy", "true")
  })

  it("keeps the mirrored signal on the icon when the sidebar is collapsed too", () => {
    const { computers, mark } = renderAttention({ computerAttention: { errors: 0, warnings: 1 } })
    fireEvent.click(screen.getByRole("button", { name: "Collapse Computers menu" }))
    fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }))
    expect(computers()).toHaveAccessibleDescription("1 computer needs attention")
    expect(mark()).toHaveClass("bg-warning")
    expect(mark()).toBeEmptyDOMElement()
  })

  it("names the collapsed-sidebar Overview dot by the computers that need attention", () => {
    renderAttention()
    fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }))
    expect(within(screen.getByRole("button", { name: /^All computers/ })).getByRole("status", { name: "3 computers need attention" })).toHaveClass("bg-destructive")
  })
})

function wait(milliseconds: number) {
  act(() => vi.advanceTimersByTime(milliseconds))
}

describe("sidebar hover preview", () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    cleanup()
    vi.useRealTimers()
  })

  it("previews on deliberate hover without moving the page, then closes after leaving", () => {
    const { toggle, sidebar } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(100)
    expect(sidebar).toHaveAttribute("data-collapsed", "true")
    wait(100)
    expect(sidebar).toHaveAttribute("data-previewing", "true")
    expect(sidebar).toHaveAttribute("data-collapsed", "false")
    expect(sidebar.parentElement).toHaveAttribute("data-sidebar-layout", "collapsed")
    expect(toggle).toHaveAccessibleName("Keep sidebar open")

    fireEvent.pointerLeave(toggle)
    wait(80)
    fireEvent.pointerEnter(sidebar)
    wait(250)
    expect(sidebar).toHaveAttribute("data-previewing", "true")
    fireEvent.pointerLeave(sidebar)
    wait(80)
    expect(sidebar).toHaveAttribute("data-previewing", "true")
    wait(120)
    expect(sidebar).toHaveAttribute("data-collapsed", "true")
    expect(toggle).toHaveAttribute("aria-expanded", "false")
  })

  it("ignores a passing hover and does not reopen immediately after a collapse click", () => {
    const { toggle, sidebar } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(60)
    fireEvent.pointerLeave(toggle)
    wait(250)
    expect(sidebar).toHaveAttribute("data-previewing", "false")

    fireEvent.click(toggle)
    fireEvent.pointerEnter(toggle)
    fireEvent.click(toggle)
    wait(300)
    expect(sidebar).toHaveAttribute("data-collapsed", "true")
  })

  it("pins a preview on click and stays open after the pointer leaves", () => {
    const { toggle, sidebar } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(200)
    fireEvent.click(toggle)
    fireEvent.pointerLeave(toggle)
    wait(300)
    expect(sidebar).toHaveAttribute("data-previewing", "false")
    expect(sidebar).toHaveAttribute("data-collapsed", "false")
    expect(sidebar.parentElement).toHaveAttribute("data-sidebar-layout", "expanded")
    expect(toggle).toHaveAccessibleName("Collapse sidebar")
  })

  it("cancels dismissal when the pointer returns to the preview", () => {
    const { toggle, sidebar } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(200)
    fireEvent.pointerLeave(toggle)
    fireEvent.pointerEnter(sidebar)
    fireEvent.pointerLeave(sidebar)
    wait(100)
    fireEvent.pointerEnter(sidebar)
    wait(200)
    expect(sidebar).toHaveAttribute("data-previewing", "true")
  })

  it("restores focus after Escape without reopening the preview or tooltip", () => {
    const { toggle, sidebar } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(200)
    act(() => screen.getByRole("button", { name: "Files" }).focus())
    fireEvent.keyDown(window, { key: "Escape" })
    expect(sidebar).toHaveAttribute("data-collapsed", "true")
    expect(toggle).toHaveFocus()
    wait(300)
    expect(sidebar).toHaveAttribute("data-previewing", "false")
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument()

    act(() => screen.getByRole("button", { name: "Page content" }).focus())
    act(() => toggle.focus())
    expect(screen.getByRole("tooltip")).toHaveTextContent("Expand sidebar")
  })

  it.each(["handled", "composing"])("keeps a preview open when Escape is %s", (mode) => {
    const { toggle, sidebar } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(200)
    const event = new KeyboardEvent("keydown", { key: "Escape", cancelable: true, isComposing: mode === "composing" })
    if (mode === "handled") event.preventDefault()
    fireEvent(window, event)
    expect(sidebar).toHaveAttribute("data-previewing", "true")
    fireEvent.keyDown(window, { key: "Escape" })
    expect(sidebar).toHaveAttribute("data-previewing", "false")
  })

  it("retains a preview during keyboard navigation and closes when focus leaves", () => {
    const { toggle, sidebar } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(200)
    fireEvent.keyDown(sidebar, { key: "Tab" })
    act(() => screen.getByRole("button", { name: "Files" }).focus())
    fireEvent.pointerLeave(toggle)
    wait(250)
    expect(sidebar).toHaveAttribute("data-previewing", "true")
    act(() => screen.getByRole("button", { name: "Page content" }).focus())
    wait(200)
    expect(sidebar).toHaveAttribute("data-collapsed", "true")
  })

  it("retains a preview when Tab moves from the toggle into the sidebar", () => {
    const { toggle, sidebar } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(200)
    act(() => toggle.focus())
    fireEvent.pointerLeave(toggle)
    fireEvent.keyDown(toggle, { key: "Tab" })
    act(() => screen.getByRole("button", { name: "Files" }).focus())
    wait(250)
    expect(sidebar).toHaveAttribute("data-previewing", "true")
    expect(screen.getByRole("button", { name: "Files" })).toHaveFocus()
    act(() => screen.getByRole("button", { name: "Page content" }).focus())
    wait(200)
    expect(sidebar).toHaveAttribute("data-previewing", "false")
  })

  it("does not reveal previously hovered tooltips when the sidebar collapses", () => {
    const { toggle } = renderSidebar()
    fireEvent.click(toggle)

    for (const name of ["Files", "GitHub", "Secrets"]) {
      const item = screen.getByRole("button", { name })
      fireEvent.pointerMove(item)
      wait(350)
      expect(screen.queryByRole("tooltip")).not.toBeInTheDocument()
      fireEvent.pointerLeave(item)
      fireEvent.pointerMove(screen.getByRole("button", { name: "Page content" }), { clientX: 500, clientY: 500 })
    }

    fireEvent.click(toggle)
    wait(350)
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument()
  })

  it("dismisses tooltips hovered during a temporary sidebar preview", () => {
    const { toggle } = renderSidebar()
    fireEvent.pointerEnter(toggle)
    wait(200)
    fireEvent.pointerLeave(toggle)
    const item = screen.getByRole("button", { name: "Files" })
    fireEvent.pointerEnter(item)
    fireEvent.pointerMove(item)
    wait(350)
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument()

    fireEvent.pointerLeave(item)
    fireEvent.pointerMove(screen.getByRole("button", { name: "Page content" }), { clientX: 500, clientY: 500 })
    wait(350)
    expect(toggle).toHaveAccessibleName("Expand sidebar")
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument()
  })

  it("keeps collapsed tooltips available on hover and keyboard focus", () => {
    renderSidebar()
    const item = screen.getByRole("button", { name: "Files" })
    fireEvent.pointerMove(item)
    wait(350)
    expect(screen.getByRole("tooltip")).toHaveTextContent("Files")
    fireEvent.pointerLeave(item)
    fireEvent.pointerMove(screen.getByRole("button", { name: "Page content" }), { clientX: 500, clientY: 500 })
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument()

    act(() => item.focus())
    expect(screen.getByRole("tooltip")).toHaveTextContent("Files")
    fireEvent.keyDown(item, { key: "Escape" })
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument()
  })
})
