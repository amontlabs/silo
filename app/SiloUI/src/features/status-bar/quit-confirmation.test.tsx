import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import type { ApplicationSource } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { fixtureDirectoryLoader } from "@/fixtures/directory-loader"
import { TooltipProvider } from "@/components/ui/tooltip"
import { StatusBarContent } from "./status-bar"
import type { StatusBarActions } from "./status-bar-types"

function setup(source: ApplicationSource, quitRequest?: number) {
  const actions: StatusBarActions = {
    listComputerDirectory: fixtureDirectoryLoader(source.computers),
    openSilo: vi.fn(), quit: vi.fn(), refresh: vi.fn(), pushRepository: vi.fn(), dismissRepositoryPush: vi.fn(),
    startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn(),
    openTerminal: vi.fn(), openEditor: vi.fn(), openSite: vi.fn(),
  }
  const view = render(<StatusBarContent source={source} actions={actions} focusContent={vi.fn()} quitRequest={quitRequest} />, { wrapper: TooltipProvider })
  return { user: userEvent.setup(), actions, ...view }
}

function withStates(states: ApplicationSource["computers"][number]["state"][]) {
  const base = applicationSourceForScenario("complete")
  const template = base.computers.find((computer) => !computer.device)!
  const computers = states.map((state, index) => ({ ...template, state, device: undefined, configuration: { ...template.configuration, id: `vm-${index}`, name: `box-${index}` } }))
  return { ...base, activities: [], computers }
}

describe("quit confirmation", () => {
  it("names running local computers before quitting and cancels cleanly", async () => {
    const { user, actions } = setup(withStates(["running", "stopped", "running"]))
    await user.click(screen.getByRole("button", { name: "Quit Silo" }))
    const prompt = screen.getByRole("alertdialog", { name: "Quit Silo?" })
    expect(prompt).toHaveTextContent("2 computers are running: box-0, box-2.")
    expect(actions.quit).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
    expect(actions.quit).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Quit Silo" }))
    await user.click(screen.getByRole("button", { name: "Keep running" }))
    expect(actions.quit).toHaveBeenCalledExactlyOnceWith("keep")

    await user.click(screen.getByRole("button", { name: "Quit Silo" }))
    await user.click(screen.getByRole("button", { name: "Stop and quit" }))
    expect(actions.quit).toHaveBeenLastCalledWith("stop")
  })

  it("quits without a prompt when nothing runs", async () => {
    const { user, actions } = setup(withStates(["stopped"]))
    await user.click(screen.getByRole("button", { name: "Quit Silo" }))
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
    expect(actions.quit).toHaveBeenCalledTimes(1)
  })

  it("opens the same confirmation for an external quit request", () => {
    const source = withStates(["running"])
    const { rerender, actions } = setup(source)
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
    rerender(<StatusBarContent source={source} actions={actions} focusContent={vi.fn()} quitRequest={1} />)
    expect(screen.getByRole("alertdialog", { name: "Quit Silo?" })).toHaveTextContent("1 computer is running: box-0.")
  })
})
