import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { ComputerUseSettings } from "./computer-use-settings"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createComputerUseBridge, type ComputerUseBackend } from "@/desktop/computer-use-bridge"
import { ComputerUseProvider } from "@/desktop/computer-use-provider"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import type { ApplicationSource } from "../model/application-source"

function source(connections: ApplicationSource["connections"]): ApplicationSource {
  return { ...applicationSourceForScenario("running"), connections, devices: [] }
}

describe("Computer use components", () => {
  const HOST = "11111111-1111-4111-8111-111111111111"
  const OFFLINE = "22222222-2222-4222-8222-222222222222"
  const devices = [
    { id: HOST, name: "Office Mac", address: "ana@office", connected: true },
    { id: OFFLINE, name: "Laptop", address: "ana@laptop", connected: false },
  ]
  function settings(statuses: Record<string, unknown>, retry = vi.fn(async (_device?: string) => ({}))) {
    const reads: Array<string | undefined> = []
    const backend: ComputerUseBackend = {
      readDesktopState: async () => ({}), setApproval: async () => ({}), setup: async () => ({}),
      chatGptStatus: async device => { reads.push(device); return statuses[device ?? "local"] },
      retry, listenStatus: async () => () => {},
    }
    render(<ComputerUseProvider bridge={createComputerUseBridge(backend)}>
      <ComputerUseSettings source={{ ...source(undefined), devices: devices }} active />
    </ComputerUseProvider>)
    return { reads, retry }
  }
  const section = () => screen.queryByRole("region", { name: "Computer use components" })
  const row = (name: string) => within(screen.getByRole("list", { name: "Devices that need attention" })).getByText(name).closest("li")!

  it("shows nothing while every device prepares ChatGPT for Linux, ready or not", async () => {
    const { reads } = settings({ local: { state: "downloading", receivedBytes: 42, totalBytes: 100 }, [HOST]: { state: "ready", path: "/p", version: "1" } })
    await waitFor(() => expect(reads).toContain(HOST))
    await waitFor(() => expect(reads).toContain(undefined))
    expect(section()).not.toBeInTheDocument()
    expect(screen.queryByText(/ChatGPT for Linux/)).not.toBeInTheDocument()
    // An offline device is not asked.
    expect(reads).not.toContain(OFFLINE)
  })

  it("lists only the devices with a failed download, with the reason and the disclosure", async () => {
    settings({ local: { state: "ready", path: "/p", version: "1" }, [HOST]: { state: "failed", reason: "Silo could not reach OpenAI.", retryable: true } })
    expect(await screen.findByRole("region", { name: "Computer use components" })).toBeVisible()
    expect(within(row("Office Mac")).getByRole("alert")).toHaveTextContent("Silo could not reach OpenAI. Silo tries again automatically.")
    expect(screen.queryByText("This device")).not.toBeInTheDocument()
    expect(screen.getByText("Silo downloads ChatGPT for Linux from OpenAI so agents in your computers can use the Linux desktop.")).toBeVisible()
    expect(screen.queryByRole("button", { name: /Accept|Not now|Download/ })).not.toBeInTheDocument()
  })

  it("retries a failed device by its device id and not through a computer", async () => {
    const { retry } = settings({ local: { state: "ready", path: "/p", version: "1" }, [HOST]: { state: "failed", reason: "Offline.", retryable: true } })
    fireEvent.click(await screen.findByRole("button", { name: "Retry ChatGPT for Linux on Office Mac" }))
    await waitFor(() => expect(retry).toHaveBeenCalledWith(HOST))
    expect(screen.getAllByRole("button", { name: /^Retry/ })).toHaveLength(1)
  })

  it("treats an owner on an older Silo as no problem", async () => {
    const { reads } = settings({ local: { state: "ready", path: "/p", version: "1" }, [HOST]: { state: "notConsented" } })
    await waitFor(() => expect(reads).toContain(HOST))
    expect(section()).not.toBeInTheDocument()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })

  it("reveals the complete name of a device with a problem", async () => {
    settings({ local: { state: "ready", path: "/p", version: "1" }, [HOST]: { state: "failed", reason: "Offline.", retryable: true } })
    await screen.findByRole("region", { name: "Computer use components" })
    expect(within(row("Office Mac")).getByText("Office Mac")).toHaveAttribute("title", "Office Mac")
  })

  it("offers Refresh for a status that cannot be read and clears once it can", async () => {
    let failing = true
    const backend: ComputerUseBackend = {
      readDesktopState: async () => ({}), setApproval: async () => ({}), setup: async () => ({}),
      chatGptStatus: async device => { if (device && failing) throw new Error("SSH connection lost."); return { state: "ready", path: "/p", version: "1" } },
      retry: async () => ({}), listenStatus: async () => () => {},
    }
    render(<ComputerUseProvider bridge={createComputerUseBridge(backend, { busy: 20000, idle: 20000 })}>
      <ComputerUseSettings source={{ ...source(undefined), devices: devices }} active />
    </ComputerUseProvider>)
    await waitFor(() => expect(within(row("Office Mac")).getByRole("alert")).toHaveTextContent("SSH connection lost."))
    expect(screen.queryByRole("button", { name: /^Retry/ })).not.toBeInTheDocument()
    failing = false
    fireEvent.click(within(row("Office Mac")).getByRole("button", { name: "Refresh ChatGPT for Linux status on Office Mac" }))
    await waitFor(() => expect(section()).not.toBeInTheDocument())
  })
})

describe("New computer approval default", () => {
  function withBridge(store = createMemorySettingsStore()) {
    const backend: ComputerUseBackend = {
      readDesktopState: async () => ({}), setApproval: async () => ({}), setup: async () => ({}),
      chatGptStatus: async () => ({ state: "ready", path: "/p", version: "1" }), retry: async () => ({}), listenStatus: async () => () => {},
    }
    render(<SettingsProvider store={store}><ComputerUseProvider bridge={createComputerUseBridge(backend)}>
      <ComputerUseSettings source={source(undefined)} active />
    </ComputerUseProvider></SettingsProvider>)
    return store
  }

  it("is off by default and saves the choice", async () => {
    const store = withBridge()
    const toggle = screen.getByRole("switch", { name: "Allow agents to use the desktop without asking in new computers" })
    expect(toggle).not.toBeChecked()
    expect(screen.getByText("Claude Code, Codex and similar agents stop asking before using the computer’s desktop. Not a security boundary.")).toBeVisible()
    fireEvent.click(toggle)
    await waitFor(() => expect(store.getSnapshot().settings.computerUseAutoApproval).toBe(true))
    expect(toggle).toBeChecked()
  })

  it("is not offered when this build has no built-in computer use", () => {
    render(<ComputerUseSettings source={source(undefined)} active />)
    expect(screen.queryByRole("switch", { name: /without asking/ })).not.toBeInTheDocument()
  })
})




it("stops subscription recovery timers when device settings become inactive", async () => {
  vi.useFakeTimers()
  const listen = vi.fn().mockRejectedValue(new Error("Event bridge unavailable"))
  const read = vi.fn().mockResolvedValue({ state: "downloading", receivedBytes: 1, totalBytes: 10 })
  const backend: ComputerUseBackend = {
    readDesktopState: vi.fn(), setApproval: vi.fn(), setup: vi.fn(), retry: vi.fn(),
    chatGptStatus: read, listenStatus: listen,
  }
  const bridge = createComputerUseBridge(backend)
  const settings = (active: boolean) => <ComputerUseProvider bridge={bridge}>
    <ComputerUseSettings source={source(undefined)} active={active} />
  </ComputerUseProvider>
  const view = render(settings(true))
  try {
    await act(async () => vi.advanceTimersByTimeAsync(0))
    expect(screen.getByRole("alert")).toHaveTextContent("Event bridge unavailable")
    expect(screen.getByRole("button", { name: "Refresh ChatGPT for Linux status on This device" })).toBeEnabled()
    view.rerender(settings(false))
    expect(vi.getTimerCount()).toBe(0)
    await act(async () => vi.advanceTimersByTimeAsync(60_000))
    expect(listen).toHaveBeenCalledOnce()
    expect(read).toHaveBeenCalledOnce()
    view.rerender(settings(true))
    await act(async () => vi.advanceTimersByTimeAsync(0))
    expect(listen).toHaveBeenCalledTimes(2)
  } finally { view.unmount(); vi.useRealTimers() }
})


