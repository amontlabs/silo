import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions } from "../model/application-source"
import { computerTarget } from "../model/connections"
import { TooltipProvider } from "@/components/ui/tooltip"
import { OverviewPage } from "../pages/overview-page"

function openComputerPage(active: boolean, refreshNetwork: ApplicationActions["refreshNetwork"]) {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  const computer = source.computers.find(item => !item.device)!
  computer.state = "running"
  computer.freshness = "fresh"
  source.network = { computers: [{ computer: computerTarget(computer), error: null, ports: [] }] }
  const actions = { refreshNetwork, saveNetworkPort: vi.fn(), removeNetworkPort: vi.fn(), openNetworkPort: vi.fn() } as unknown as ApplicationActions
  const page = (visible: boolean) => <OverviewPage active={visible} source={source} actions={actions} onConfigurationsChange={vi.fn()}
    selectedComputerId={computer.configuration.id} computerTab="overview" onOpenComputer={vi.fn()} onCloseComputer={vi.fn()} onSelectComputerTab={vi.fn()} />
  const view = render(page(active))
  return { ...view, show: () => view.rerender(page(true)) }
}

it("does not poll a computer's ports while the Computers panel is hidden", async () => {
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] })
  try {
    const refreshNetwork = vi.fn(async () => {})
    const { show } = openComputerPage(false, refreshNetwork)
    vi.advanceTimersByTime(20_000)
    expect(refreshNetwork).not.toHaveBeenCalled()

    show()
    await waitFor(() => expect(refreshNetwork).toHaveBeenCalledTimes(1))
    vi.advanceTimersByTime(12_000)
    expect(refreshNetwork).toHaveBeenCalledTimes(2)
  } finally { vi.useRealTimers() }
})


it("names each remote device in the port form's computer selector", async () => {
  const { NetworkPortForm } = await import("./network-ports")
  const { useNetworkPorts } = await import("./network-ports-state")
  const source = structuredClone(applicationSourceForScenario("running"))
  const local = source.computers.find(computer => !computer.device)!
  const remote = { ...local, configuration: { ...local.configuration, id: "silo-remote:office:vm" }, device: { id: "office", name: "Office Mac", address: "user@office", connected: true, computerId: "vm" } }
  function Form() {
    const controller = useNetworkPorts({ computers: [local, remote], actions: {} as ApplicationActions, active: false })
    return <><button onClick={() => controller.add()}>Add port</button><NetworkPortForm controller={controller} fieldID="test" /></>
  }
  render(<TooltipProvider><Form /></TooltipProvider>)
  await userEvent.setup().click(screen.getByRole("button", { name: "Add port" }))
  expect(screen.getByRole("option", { name: `${local.configuration.name} · Office Mac` })).toHaveValue(computerTarget(remote))
  expect(screen.getByRole("option", { name: local.configuration.name })).toHaveValue(local.configuration.name)
})
