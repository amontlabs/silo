import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { setupFakeTimerUser } from "@/test/fake-timer-user"
import { TooltipProvider } from "@/components/ui/tooltip"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { ComputerConfigurationList } from "@/features/computers/components/computer-configuration-list"
import { computerUseFixtureNames, createFixtureComputerUseBackend, fixtureComputerUse, fixtureDesktopState } from "@/fixtures/computer-use"
import { createComputerUseBridge, type ComputerUseBackend } from "./computer-use-bridge"
import { ComputerUseProvider } from "./computer-use-provider"
import { ComputerUsePanel, ComputerUseSection } from "./computer-use-panel"
import { ComputerUseApprovalSwitch, useComputerUseApproval, type ComputerUseApprovalController } from "./computer-use-approval"
import { LinuxDesktopViewer } from "./linux-desktop-viewer"
import { parseLinuxDesktopState, type ComputerUseState, type LinuxDesktopState } from "./linux-desktop-state"

const ready = fixtureComputerUse("ready")

function backend(overrides: Partial<ComputerUseBackend> = {}): ComputerUseBackend {
  const base = createFixtureComputerUseBackend("ready")
  return { ...base, ...overrides }
}
const wrap = (bridgeBackend: ComputerUseBackend, children: React.ReactNode) =>
  <ComputerUseProvider bridge={createComputerUseBridge(bridgeBackend)}>{children}</ComputerUseProvider>

describe("computer use schemas", () => {
  const desktop = { installed: true, autoStart: true, state: "running" }
  it("parses the computerUse object of a v4 computer", () => {
    const parsed = parseLinuxDesktopState({ ...desktop, computerUse: { ...ready, agents: ["Codex"] } })
    expect(parsed.computerUse).toMatchObject({ state: "ready", approval: "ask", driverVersion: "0.38.0", agents: ["Codex"] })
  })
  it("leaves older computers without computerUse", () => {
    expect(parseLinuxDesktopState({ ...desktop }).computerUse).toBeUndefined()
  })
  it("tolerates unknown or malformed diagnostics without losing the desktop state", () => {
    const parsed = parseLinuxDesktopState({ ...desktop, computerUse: { state: "rebooting", approval: "sometimes", agents: "none", driverVersion: 7, extra: true } })
    expect(parsed.state).toBe("running")
    expect(parsed.computerUse).toMatchObject({ state: "unavailable", approval: "unknown", agents: null, driverVersion: null })
    expect(parseLinuxDesktopState({ ...desktop, computerUse: "broken" }).computerUse).toBeNull()
  })
  it("has no consent state in computer use either", () => {
    expect(parseLinuxDesktopState({ installed: true, autoStart: true, state: "running", computerUse: { state: "needs-consent" } }).computerUse?.state).toBe("unavailable")
    expect(computerUseFixtureNames).not.toContain("needs-consent")
  })
})

function controller(computerUse: ComputerUseState | null | undefined, extra: Partial<ComputerUseApprovalController> = {}): ComputerUseApprovalController {
  return {
    state: null, computerUse, running: true, busy: false, error: null, loadError: null,
    setApproval: vi.fn(), setup: vi.fn(), refresh: vi.fn(), dismissError: vi.fn(), ...extra,
  }
}
const renderPanel = (computerUse: ComputerUseState, extra: Partial<ComputerUseApprovalController> = {}) => {
  const approval = controller(computerUse, extra)
  render(<ComputerUsePanel approval={approval} />)
  return approval
}

describe("computer use panel", () => {
  it.each(["installing", "ready", "auto"] as const)("shows only the approval switch and one hint while %s", name => {
    const { container } = render(<ComputerUsePanel approval={controller(fixtureComputerUse(name))} />)
    expect(screen.getByRole("switch", { name: "Allow without asking" })).toBeVisible()
    expect(screen.getByText(/use this computer's desktop without asking first/)).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    expect(screen.queryByRole("note")).not.toBeInTheDocument()
    expect(screen.queryByRole("button")).not.toBeInTheDocument()
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument()
    expect(container).not.toHaveTextContent(/Details|Set up computer use|Installing|Ready|Preparing|0\.38/)
  })
  it("keeps the switch while the computer's computer use is still unavailable", () => {
    renderPanel(fixtureComputerUse("unavailable"))
    expect(screen.getByRole("switch", { name: "Allow without asking" })).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })
  it("binds the switch to the approval mode", async () => {
    const user = userEvent.setup()
    const approval = renderPanel({ ...ready, approval: "auto" })
    const toggle = screen.getByRole("switch", { name: "Allow without asking" })
    expect(toggle).toBeChecked()
    await user.click(toggle)
    expect(approval.setApproval).toHaveBeenCalledWith("ask")
  })
  it("keeps the switch usable while the computer is stopped", async () => {
    const approval = renderPanel(ready, { running: false })
    await userEvent.setup().click(screen.getByRole("switch", { name: "Allow without asking" }))
    expect(approval.setApproval).toHaveBeenCalledWith("auto")
  })
  it("offers Try again for a failed setup, running setup only while the computer runs", async () => {
    const approval = renderPanel(fixtureComputerUse("failed"))
    expect(screen.getByRole("alert")).toHaveTextContent("setup failed")
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }))
    expect(approval.setup).toHaveBeenCalledOnce()
  })
  it("explains instead of running setup when the computer is stopped", () => {
    renderPanel(fixtureComputerUse("failed"), { running: false })
    expect(screen.getByRole("button", { name: "Try again" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Try again" })).toHaveAttribute("title", "Start the computer to try again.")
  })
  it("keeps the backend reason of a failure in a tooltip", () => {
    renderPanel(fixtureComputerUse("failed"))
    expect(screen.getByText("Computer use setup failed.")).toHaveAttribute("title", fixtureComputerUse("failed").reason)
  })
  it("shows errors from the owning device as they came, and reads that fail with Try again", async () => {
    const approval = renderPanel(ready, { error: "Device disconnected: office-mac", loadError: "Connection lost" })
    expect(screen.getAllByRole("alert").map(alert => alert.textContent)).toEqual([expect.stringContaining("Device disconnected: office-mac"), expect.stringContaining("Connection lost")])
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }))
    expect(approval.refresh).toHaveBeenCalledOnce()
    await userEvent.setup().click(screen.getByRole("button", { name: "Dismiss error" }))
    expect(approval.dismissError).toHaveBeenCalledOnce()
  })
})

describe("approval switch for other surfaces", () => {
  it("renders nothing until the computer's computer use is known", () => {
    const { container } = render(<ComputerUseApprovalSwitch approval={controller(null)} />)
    expect(container).toBeEmptyDOMElement()
  })
  it("embeds on its own with a hook", async () => {
    const setApproval = vi.fn(async (_computer: string, mode: "ask" | "auto") => ({ ...fixtureDesktopState("ready"), computerUse: { ...ready, approval: mode, appliedApproval: mode } }))
    function Toast() { return <ComputerUseApprovalSwitch approval={useComputerUseApproval("office/vm-1", 60_000)} /> }
    render(wrap(backend({ readDesktopState: async () => fixtureDesktopState("ready"), setApproval }), <Toast />))
    await userEvent.setup().click(await screen.findByRole("switch", { name: "Allow without asking" }))
    await waitFor(() => expect(screen.getByRole("switch", { name: "Allow without asking" })).toBeChecked())
    expect(setApproval).toHaveBeenCalledWith("office/vm-1", "auto")
  })
})


describe("computer use section", () => {
  function section(b: ComputerUseBackend) {
    render(wrap(b, <ComputerUseSection computer="office/vm-1" pollMs={60_000} />))
  }
  it("sets the approval mode through the owning device's command and shows the result", async () => {
    const setApproval = vi.fn(async (_computer: string, mode: "ask" | "auto") => ({ ...fixtureDesktopState("ready"), computerUse: { ...ready, approval: mode } }))
    section(backend({ readDesktopState: async () => fixtureDesktopState("ready"), setApproval }))
    const toggle = await screen.findByRole("switch", { name: /Allow without asking/ })
    expect(toggle).not.toBeChecked()
    await userEvent.setup().click(toggle)
    await waitFor(() => expect(screen.getByRole("switch", { name: /Allow without asking/ })).toBeChecked())
    expect(setApproval).toHaveBeenCalledWith("office/vm-1", "auto")
  })
  it("reports a failed approval change and shows the state read afterwards", async () => {
    section(backend({ readDesktopState: async () => fixtureDesktopState("ready"), setApproval: async () => { throw new Error("office-mac is offline") } }))
    await userEvent.setup().click(await screen.findByRole("switch", { name: /Allow without asking/ }))
    expect(await screen.findByRole("alert")).toHaveTextContent("office-mac is offline")
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).not.toBeChecked()
  })
  it("runs setup from Try again after a failed setup and returns to the plain switch", async () => {
    const setup = vi.fn(async () => fixtureDesktopState("ready"))
    section(backend({ readDesktopState: async () => fixtureDesktopState("failed"), setup }))
    await screen.findByText(/setup failed/)
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }))
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument())
    expect(setup).toHaveBeenCalledWith("office/vm-1")
  })
  it("shows a rejected setup", async () => {
    section(backend({ readDesktopState: async () => fixtureDesktopState("failed"), setup: async () => { throw new Error("The computer stopped.") } }))
    await screen.findByText(/setup failed/)
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }))
    expect(await screen.findByText("The computer stopped.")).toBeVisible()
  })
  it("keeps a setup failure on Try again", async () => {
    section(backend({ readDesktopState: async () => fixtureDesktopState("failed") }))
    expect(await screen.findByText(/setup failed/)).toBeVisible()
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled()
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument()
  })
  it("renders nothing for a pre-v4 computer", async () => {
    const read = vi.fn(async () => fixtureDesktopState("pre-v4"))
    section(backend({ readDesktopState: read }))
    await waitFor(() => expect(read).toHaveBeenCalled())
    expect(screen.queryByRole("region", { name: "Computer use" })).not.toBeInTheDocument()
  })
  it("renders nothing without a bridge", () => {
    render(<ComputerUseSection computer="dev" />)
    expect(screen.queryByRole("region", { name: "Computer use" })).not.toBeInTheDocument()
  })
})

describe("v3 and v4 desktop viewer", () => {
  function viewer(state: LinuxDesktopState) {
    const onAction = vi.fn()
    render(<LinuxDesktopViewer name="dev" state={state} busy={false} error={null} onAction={onAction} onRetry={vi.fn()} onFullscreen={vi.fn()} />)
    return onAction
  }
  it("shows nothing about computer use for a healthy v4 computer", () => {
    viewer(fixtureDesktopState("ready"))
    expect(screen.queryByText(/Computer use/)).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: /Set up/ })).not.toBeInTheDocument()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })
  it("offers Try again for a failed setup of the computer itself", async () => {
    const onAction = viewer(fixtureDesktopState("failed"))
    expect(screen.getByRole("alert")).toHaveTextContent("Computer use setup failed")
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }))
    expect(onAction).toHaveBeenCalledWith("setup-computer-use")
  })
  it("shows nothing while computer use is installing", () => {
    viewer(fixtureDesktopState("installing"))
    expect(screen.queryByRole("status")).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: /Set up|Try again/ })).not.toBeInTheDocument()
  })
  it("does not send a v4 computer to Add Linux desktop", () => {
    viewer({ ...fixtureDesktopState("ready"), state: "uninstalled", installed: false })
    expect(screen.queryByText(/Add Linux desktop/)).not.toBeInTheDocument()
    expect(screen.getByText("Desktop unavailable")).toBeVisible()
  })
  it("says a computer whose desktop is off has no desktop and where to turn it on", () => {
    viewer({ installed: true, autoStart: false, state: "stopped" })
    expect(screen.getByText("dev has no desktop")).toBeVisible()
    expect(screen.getByText(/Turn on Desktop in the computer's settings/)).toBeVisible()
  })
  it("still points an older computer to Add Linux desktop", () => {
    viewer({ installed: false, autoStart: false, state: "uninstalled" })
    expect(screen.getByText(/Add Linux desktop/)).toBeVisible()
  })
})

describe("computer settings for v3 and v4", () => {
  const configuration = productionComputerDefaults[0]
  function editor(draft: typeof configuration, created: boolean, provider: boolean) {
    const list = <TooltipProvider><ComputerConfigurationList configurations={created ? [draft] : []} onConfigurationsChange={vi.fn()}
      isComputerCreated={() => created} isComputerRunning={() => created}
      initialEditorDraft={{ draft, originalID: created ? draft.id : undefined, insertAt: 0 }} /></TooltipProvider>
    render(provider ? wrap(backend(), list) : list)
  }
  it("offers one Desktop switch, on, for a built-in computer, and no install controls or note", () => {
    editor({ ...configuration, desktop: { startWithComputer: true, builtIn: true } }, true, true)
    expect(screen.getByRole("switch", { name: "Desktop" })).toBeChecked()
    expect(screen.queryByRole("switch", { name: "Start desktop with computer" })).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Add Linux desktop" })).not.toBeInTheDocument()
    expect(screen.queryByText(/Built in/)).not.toBeInTheDocument()
    expect(screen.queryByText(/Linux desktop and computer use/)).not.toBeInTheDocument()
  })
  it("keeps the startup switch for a computer created before v4", () => {
    editor({ ...configuration, desktop: { startWithComputer: true } }, true, true)
    expect(screen.getByRole("switch", { name: "Start desktop with computer" })).toBeVisible()
  })
  it("keeps Add Linux desktop for an older computer without a desktop", () => {
    editor(configuration, true, true)
    expect(screen.getByRole("button", { name: "Add Linux desktop" })).toBeVisible()
  })
  it("shows no download notice or consent when creating a computer, and saving is never blocked", async () => {
    editor(configuration, false, true)
    expect(screen.queryByRole("checkbox", { name: "Linux desktop" })).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: /Accept|Not now/ })).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Create" })).toBeEnabled()
  })
  it("keeps the desktop checkbox without computer use support", () => {
    editor(configuration, false, false)
    expect(screen.getByRole("checkbox", { name: "Linux desktop" })).toBeVisible()
  })
})

describe("creating a computer on the selected device", () => {
  const configuration = productionComputerDefaults[0]
  const devices = [{ id: "11111111-1111-4111-8111-111111111111", name: "Linux box", connected: true }]
  function creating(draft: typeof configuration = configuration, onConfigurationsChange = vi.fn()) {
    render(wrap(backend(), <TooltipProvider><ComputerConfigurationList configurations={[]} devices={devices} getDeviceId={() => undefined} onConfigurationsChange={onConfigurationsChange}
      isComputerCreated={() => false} isComputerRunning={() => false}
      initialEditorDraft={{ draft, insertAt: 0 }} /></TooltipProvider>))
    return onConfigurationsChange
  }

  it("keeps a new built-in computer's desktop off when its settings chose that", async () => {
    const onConfigurationsChange = creating({ ...configuration, desktop: { startWithComputer: false } })
    expect(screen.getByRole("switch", { name: "Desktop" })).not.toBeChecked()
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Create" }))
    await waitFor(() => expect(onConfigurationsChange).toHaveBeenCalled())
    expect(onConfigurationsChange.mock.lastCall?.[0]).toEqual([expect.objectContaining({ desktop: { startWithComputer: false } })])
  })
  it("turns the desktop of a new built-in computer off with the Desktop switch", async () => {
    const onConfigurationsChange = creating()
    expect(screen.getByRole("switch", { name: "Desktop" })).toBeChecked()
    const user = userEvent.setup()
    await user.click(screen.getByRole("switch", { name: "Desktop" }))
    await user.click(screen.getByRole("button", { name: "Create" }))
    await waitFor(() => expect(onConfigurationsChange).toHaveBeenCalled())
    expect(onConfigurationsChange.mock.lastCall?.[0]).toEqual([expect.objectContaining({ desktop: { startWithComputer: false } })])
  })
  it("keeps a manual desktop start for a computer created before the built-in desktop", () => {
    const draft = { ...configuration, desktop: { startWithComputer: false } }
    render(wrap(backend(), <TooltipProvider><ComputerConfigurationList configurations={[draft]} onConfigurationsChange={vi.fn()}
      isComputerCreated={() => true} isComputerRunning={() => false}
      initialEditorDraft={{ draft, originalID: draft.id, insertAt: 0 }} /></TooltipProvider>))
    expect(screen.getByRole("switch", { name: "Start desktop with computer" })).not.toBeChecked()
  })
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}






describe("computer use section reads and errors", () => {
  const section = (b: ComputerUseBackend, pollMs = 60_000) => render(wrap(b, <ComputerUseSection computer="office/vm-1" pollMs={pollMs} />))
  it("never runs two reads at once, so a slow older read cannot overwrite a newer one", async () => {
    vi.useFakeTimers()
    const slow = deferred<unknown>()
    let active = 0
    let peak = 0
    let calls = 0
    section(backend({ readDesktopState: async () => {
      calls += 1
      active += 1
      peak = Math.max(peak, active)
      try { return calls === 1 ? await slow.promise : fixtureDesktopState("failed") } finally { active -= 1 }
    } }), 10)
    await act(async () => { await vi.advanceTimersByTimeAsync(80) })
    expect(calls).toBe(1)
    await act(async () => { slow.resolve(fixtureDesktopState("ready")) })
    expect(screen.getByRole("switch", { name: "Allow without asking" })).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    await act(async () => { await vi.advanceTimersByTimeAsync(10) })
    expect(calls).toBe(2)
    expect(screen.getByRole("alert")).toHaveTextContent("setup failed")
    expect(peak).toBe(1)
  })
  it("shows a failed first read instead of nothing, and recovers with Try again", async () => {
    const reads = [() => Promise.reject(new Error("office-mac is offline")), () => Promise.resolve(fixtureDesktopState("ready"))]
    section(backend({ readDesktopState: () => reads.shift()!() }))
    const alert = await screen.findByRole("alert")
    expect(alert).toHaveTextContent("office-mac is offline")
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }))
    expect(await screen.findByRole("switch", { name: "Allow without asking" })).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })
  it("keeps a failed change visible while polling succeeds, until dismissed", async () => {
    vi.useFakeTimers()
    const user = setupFakeTimerUser()
    const read = vi.fn(async () => fixtureDesktopState("ready"))
    section(backend({ readDesktopState: read, setApproval: async () => { throw new Error("office-mac is offline") } }), 20)
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    await user.click(screen.getByRole("switch", { name: /Allow without asking/ }))
    expect(screen.getByRole("alert")).toHaveTextContent("office-mac is offline")
    expect(read).toHaveBeenCalledTimes(2)
    await act(async () => { await vi.advanceTimersByTimeAsync(120) })
    expect(read).toHaveBeenCalledTimes(8)
    expect(screen.getByRole("alert")).toHaveTextContent("office-mac is offline")
    await user.click(screen.getByRole("button", { name: "Dismiss error" }))
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })
  it("keeps the panel and reports a poll that fails after a good read", async () => {
    vi.useFakeTimers()
    const reads: Array<Promise<unknown>> = [Promise.resolve(fixtureDesktopState("ready"))]
    section(backend({ readDesktopState: () => reads.shift() ?? Promise.reject(new Error("Connection lost")) }), 20)
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    expect(screen.getByRole("switch", { name: "Allow without asking" })).toBeVisible()
    await act(async () => { await vi.advanceTimersByTimeAsync(20) })
    expect(screen.getByRole("alert")).toHaveTextContent("Connection lost")
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).toBeVisible()
  })
})

describe("unreadable approval policy", () => {
  it("is unknown, not ask, when missing or malformed", () => {
    const desktop = { installed: true, autoStart: true, state: "running" }
    expect(parseLinuxDesktopState({ ...desktop, computerUse: { state: "ready" } }).computerUse?.approval).toBe("unknown")
    expect(parseLinuxDesktopState({ ...desktop, computerUse: { state: "ready", approval: 3 } }).computerUse?.approval).toBe("unknown")
    expect(parseLinuxDesktopState({ ...desktop, computerUse: { state: "ready", approval: "auto" } }).computerUse?.approval).toBe("auto")
  })
  it("disables the switch with a short warning", () => {
    renderPanel({ ...ready, approval: "unknown" })
    const toggle = screen.getByRole("switch", { name: /Allow without asking/ })
    expect(toggle).toBeDisabled()
    expect(toggle).not.toBeChecked()
    expect(screen.getByRole("note")).toHaveTextContent("could not read the approval setting. Agents may be running without asking.")
  })
})

describe("chosen and applied approval", () => {
  const desktop = { installed: true, autoStart: true, state: "running" }
  const parse = (computerUse: object) => parseLinuxDesktopState({ ...desktop, computerUse }).computerUse
  const render_ = (computerUse: ComputerUseState, extra: Partial<ComputerUseApprovalController> = {}) =>
    render(<ComputerUsePanel approval={controller(computerUse, extra)} />)
  it("parses appliedApproval and approvalApply tolerantly", () => {
    expect(parse({ state: "ready", approval: "ask" })?.appliedApproval).toBe("unknown")
    expect(parse({ state: "ready", approval: "ask", appliedApproval: 3 })?.appliedApproval).toBe("unknown")
    expect(parse({ state: "ready", approval: "ask", appliedApproval: "auto" })).toMatchObject({ approval: "ask", appliedApproval: "auto" })
    // An older Silo does not report how applying stands; absent or malformed status is unknown.
    expect(parse({ state: "ready", approval: "ask" })?.approvalApply).toBe("unknown")
    expect(parse({ state: "ready", approval: "ask", approvalApply: "sometime" })?.approvalApply).toBe("unknown")
    for (const value of ["applied", "pending", "failed", "partial"]) expect(parse({ state: "ready", approval: "ask", approvalApply: value, approvalApplyReason: "why" })).toMatchObject({ approvalApply: value, approvalApplyReason: "why" })
    expect(parse({ state: "ready", approvalApplyReason: 4 })?.approvalApplyReason).toBeNull()
  })
  it("shows the chosen mode, says Applying… while it is pending and warns that agents may still act without asking", () => {
    render_(fixtureComputerUse("approval-pending"))
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).not.toBeChecked()
    expect(screen.getByRole("status")).toHaveTextContent("Applying…")
    expect(screen.getByRole("note")).toHaveTextContent("Agents may still act without asking until this is applied.")
  })
  it("says agents still ask while a switch to auto is pending", () => {
    render_({ ...ready, approval: "auto", appliedApproval: "ask", approvalApply: "pending" })
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).toBeChecked()
    expect(screen.getByRole("status")).toHaveTextContent("Applying…")
    expect(screen.getByRole("note")).toHaveTextContent("Agents may still ask first until this is applied.")
    expect(screen.queryByText(/still act without asking/)).toBeNull()
  })
  it("does not say Applying… for a stopped computer, and keeps the warning until the change is applied at start", () => {
    render_(fixtureComputerUse("approval-pending"), { running: false })
    expect(screen.queryByText("Applying…")).toBeNull()
    expect(screen.getByRole("note")).toHaveTextContent("may still act without asking until this is applied")
    // A computer that never applied anything has nothing to wait for.
    const { unmount } = render_({ ...ready, appliedApproval: "unknown", approvalApply: "pending" }, { running: false })
    expect(screen.getAllByRole("note")).toHaveLength(1)
    unmount()
  })
  it("reports a failed apply briefly, keeps its reason in a tooltip and keeps the choice", () => {
    render_(fixtureComputerUse("approval-failed"))
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).not.toBeChecked()
    const note = screen.getByRole("note")
    expect(note).toHaveTextContent("Not applied to every agent. Some may still act without asking.")
    expect(within(note).getByText(/Not applied/)).toHaveAttribute("title", expect.stringContaining("Applying took too long. Silo tries again when the computer starts."))
    expect(screen.queryByText("Applying…")).toBeNull()
  })
  it("reports a partial apply and warns that some agents may still act without asking", () => {
    render_(fixtureComputerUse("approval-partial"))
    const note = screen.getByRole("note")
    expect(note).toHaveTextContent("Some may still act without asking.")
    expect(within(note).getByText(/Not applied/)).toHaveAttribute("title", expect.stringContaining("Some agents could not be configured"))
    // Partial is a warning whatever the previous mode was.
    const { unmount } = render_({ ...ready, appliedApproval: "unknown", approvalApply: "partial" })
    expect(screen.getAllByRole("note").at(-1)).toHaveTextContent("may still act without asking")
    unmount()
  })
  it("warns after a failure to apply ask when nothing says ask is in place, and not when it is", () => {
    const { unmount } = render_({ ...ready, appliedApproval: "unknown", approvalApply: "failed", approvalApplyReason: "Silo could not reach the computer to apply it." })
    expect(screen.getByRole("note")).toHaveTextContent("may still act without asking")
    unmount()
    render_({ ...ready, appliedApproval: "ask", approvalApply: "failed" })
    expect(screen.getByRole("note")).toHaveTextContent("Silo could not apply the approval change.")
    expect(screen.getByRole("note")).not.toHaveTextContent("may still act without asking")
  })
  it("says some agents may still ask when a switch to auto failed", () => {
    render_({ ...ready, approval: "auto", appliedApproval: "ask", approvalApply: "failed" })
    expect(screen.getByRole("note")).toHaveTextContent("Some may still ask first.")
    expect(screen.queryByText(/still act without asking/)).toBeNull()
  })
  it("warns for an older owner that omits approvalApply when the chosen mode differs from the applied one", () => {
    // Parsed from a payload without an apply status.
    const older = (approval: string, appliedApproval: string) => parse({ state: "ready", approval, appliedApproval })!
    expect(older("ask", "auto").approvalApply).toBe("unknown")
    const { unmount } = render_(older("ask", "auto"))
    expect(screen.getByRole("note")).toHaveTextContent("Agents may still act without asking until this is applied.")
    expect(screen.queryByText("Applying…")).toBeNull()
    unmount()
    const second = render_(older("auto", "ask"))
    expect(screen.getByRole("note")).toHaveTextContent("Agents may still ask first until this is applied.")
    second.unmount()
    // Matching modes, or an applied mode that is not known, stay quiet.
    for (const computerUse of [older("ask", "ask"), older("auto", "auto"), older("ask", "unknown")]) {
      const { unmount: done } = render_(computerUse)
      expect(screen.queryByRole("note")).toBeNull()
      done()
    }
  })
  it.each([undefined, null, "sometime", 3, "unknown", "applied", "pending", "failed", "partial"])("preserves both mismatch warnings for approvalApply %j", approvalApply => {
    for (const [approval, appliedApproval, warning] of [
      ["ask", "auto", /may still act without asking/],
      ["auto", "ask", /may still ask first/],
    ]) {
      const parsed = parse({ state: "ready", approval, appliedApproval, ...(approvalApply === undefined ? {} : { approvalApply }) })!
      expect(parsed.approvalApply).toBe(["applied", "pending", "failed", "partial"].includes(String(approvalApply)) ? approvalApply : "unknown")
      const { unmount } = render_(parsed)
      expect(screen.getByRole("note")).toHaveTextContent(warning)
      expect(screen.getByRole("switch", { name: /Allow without asking/ })).toBeEnabled()
      expect(screen.getByRole("switch", { name: /Allow without asking/ })).toHaveAttribute("aria-checked", String(approval === "auto"))
      expect(screen.getByText(/Not a security boundary/)).toBeVisible()
      if (approvalApply !== "pending") expect(screen.queryByText("Applying…")).toBeNull()
      unmount()
    }
  })
  it("shows no approval note when applied, or while a first apply waits with nothing known", () => {
    for (const computerUse of [ready, fixtureComputerUse("auto"), { ...ready, approval: "auto" as const, appliedApproval: "unknown" as const },
      { ...ready, appliedApproval: "unknown" as const, approvalApply: "pending" as const }]) {
      const { unmount } = render_(computerUse)
      expect(screen.queryByRole("note")).toBeNull()
      unmount()
    }
  })
  it("keeps the unreadable-policy diagnostic alone, never a second warning", () => {
    render_({ ...ready, approval: "unknown", appliedApproval: "auto", approvalApply: "failed" })
    expect(screen.getAllByRole("note")).toHaveLength(1)
    expect(screen.getByRole("note")).toHaveTextContent("could not read the approval setting")
  })
  it("shows Applying… as soon as the switch is used, then the result the computer reports", async () => {
    const user = userEvent.setup()
    const change = deferred<unknown>()
    const reads = vi.fn(async () => ({ ...fixtureDesktopState("ready"), computerUse: fixtureComputerUse("auto") }))
    render(wrap(backend({ readDesktopState: reads, setApproval: () => change.promise }), <ComputerUseSection computer="office/vm-1" pollMs={60_000} />))
    await user.click(await screen.findByRole("switch", { name: /Allow without asking/ }))
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).not.toBeChecked()
    expect(screen.getByText("Applying…")).toBeVisible()
    await act(async () => { change.resolve({ ...fixtureDesktopState("ready"), computerUse: fixtureComputerUse("approval-failed") }) })
    expect(await screen.findByText(/Not applied to every agent/)).toBeVisible()
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).not.toBeChecked()
    expect(screen.queryByText("Applying…")).toBeNull()
  })
  it("reads the computer's state again after a command error instead of restoring the old snapshot", async () => {
    const user = userEvent.setup()
    // The command stored the choice and started applying it, then the answer was lost.
    const states = [fixtureComputerUse("auto"), { ...fixtureComputerUse("approval-pending"), approval: "ask" as const }]
    const readDesktopState = vi.fn(async () => ({ ...fixtureDesktopState("ready"), computerUse: states.length > 1 ? states.shift()! : states[0] }))
    render(wrap(backend({ readDesktopState, setApproval: async () => { throw new Error("The connection to office-mac was lost.") } }), <ComputerUseSection computer="office/vm-1" pollMs={60_000} />))
    expect(await screen.findByRole("switch", { name: /Allow without asking/ })).toBeChecked()
    await user.click(screen.getByRole("switch", { name: /Allow without asking/ }))
    expect(await screen.findByRole("alert")).toHaveTextContent("The connection to office-mac was lost.")
    // Not restored to the snapshot from before the change (auto): the authoritative state is ask, still applying.
    await waitFor(() => expect(screen.getByRole("switch", { name: /Allow without asking/ })).not.toBeChecked())
    expect(screen.getByText("Applying…")).toBeVisible()
    expect(screen.getByRole("note")).toHaveTextContent("may still act without asking")
    expect(readDesktopState).toHaveBeenCalledTimes(2)
  })
  it("keeps the old snapshot and says the read failed when the re-read fails too", async () => {
    const user = userEvent.setup()
    let reads = 0
    render(wrap(backend({
      readDesktopState: async () => { reads += 1; if (reads > 1) throw new Error("office-mac is offline"); return { ...fixtureDesktopState("ready"), computerUse: fixtureComputerUse("auto") } },
      setApproval: async () => { throw new Error("office-mac is offline") },
    }), <ComputerUseSection computer="office/vm-1" pollMs={60_000} />))
    await user.click(await screen.findByRole("switch", { name: /Allow without asking/ }))
    await waitFor(() => expect(screen.getAllByRole("alert").length).toBe(2))
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible()
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).toBeChecked()
  })
  it("applies in the fixture backend like the native one: stored at once, applied shortly after", async () => {
    const fixture = createFixtureComputerUseBackend("auto")
    const answered = parseLinuxDesktopState(await fixture.setApproval("w", "ask")).computerUse
    expect(answered).toMatchObject({ approval: "ask", appliedApproval: "auto", approvalApply: "pending" })
    await waitFor(async () => expect(parseLinuxDesktopState(await fixture.readDesktopState("w")).computerUse).toMatchObject({ appliedApproval: "ask", approvalApply: "applied" }), { timeout: 3000 })
  })
})

describe("approval copy", () => {
  it("is one line that does not claim every agent asks, and says it is not a security boundary", () => {
    const { container } = render(<ComputerUsePanel approval={controller(ready)} />)
    const text = container.textContent ?? ""
    expect(text).toMatch(/Claude Code and Codex/)
    expect(text).toMatch(/Not a security boundary/)
    expect(text).not.toMatch(/stay out of reach/)
    expect(screen.getAllByText(/security boundary/)).toHaveLength(1)
  })
})
