import { fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { isTauri } from "@tauri-apps/api/core"
import { getCurrentWindow } from "@tauri-apps/api/window"

import { WindowControls } from "./window-toolbar"

vi.mock("@tauri-apps/api/core", () => ({ isTauri: vi.fn(() => false) }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }))

afterEach(() => vi.restoreAllMocks())

describe("window controls", () => {
  it("closes, minimizes, and maximizes the actual Linux window", () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Linux aarch64")
    const close = vi.fn().mockResolvedValue(undefined)
    const minimize = vi.fn().mockResolvedValue(undefined)
    const toggleMaximize = vi.fn().mockResolvedValue(undefined)
    vi.mocked(getCurrentWindow).mockReturnValue({ close, minimize, toggleMaximize } as unknown as ReturnType<typeof getCurrentWindow>)
    render(<WindowControls />)

    fireEvent.click(screen.getByRole("button", { name: "Close window" }))
    fireEvent.click(screen.getByRole("button", { name: "Minimize window" }))
    fireEvent.click(screen.getByRole("button", { name: "Maximize or restore window" }))
    expect(close).toHaveBeenCalledOnce()
    expect(minimize).toHaveBeenCalledOnce()
    expect(toggleMaximize).toHaveBeenCalledOnce()
    for (const button of screen.getAllByRole("button")) {
      expect(button).not.toHaveAttribute("data-tauri-drag-region")
    }
  })

  it("draws neutral controls on Linux instead of macOS traffic lights", () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Linux x86_64")
    render(<WindowControls />)
    const buttons = screen.getAllByRole("button")
    expect(buttons).toHaveLength(3)
    for (const button of buttons) {
      expect(button.className).not.toMatch(/#[0-9a-f]{6}/i)
      expect(button.querySelector("svg")).toHaveAttribute("aria-hidden", "true")
    }
    expect(screen.getByRole("button", { name: "Close window" })).toHaveClass("hover:text-destructive")
  })

  it("leaves room for the macOS system controls without drawing duplicates", () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel")
    const { container } = render(<WindowControls />)
    expect(screen.queryByRole("button")).not.toBeInTheDocument()
    expect(container.querySelector("[data-window-controls]")?.children).toHaveLength(0)
  })

  it("keeps browser preview controls decorative", () => {
    vi.mocked(isTauri).mockReturnValue(false)
    const { container } = render(<WindowControls />)
    expect(screen.queryByRole("button")).not.toBeInTheDocument()
    expect(container.querySelector("[data-window-controls]")?.children).toHaveLength(3)
  })
})
