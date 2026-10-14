import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it, vi } from "vitest"

import type { QuitRequest } from "@/desktop/settings"
import { ApplicationApp } from "@/features/application/application-app"
import { ApplicationCatalogProvider } from "@/features/preferences/application-catalog"
import { SettingsProvider, useSettings } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"
import { fixtureApplicationCatalog } from "@/fixtures/application-catalog"
import { useUnavailableBackup } from "@/fixtures/application-backup"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions, ApplicationSource } from "../model/application-source"
import { QuitRequestConfirmation, type ConnectQuitConfirmation } from "./quit-request-confirmation"

function fakeConnection() {
  let ask: ((request: QuitRequest) => Promise<boolean>) | undefined
  const stop = vi.fn()
  const connect: ConnectQuitConfirmation = vi.fn(async (receive) => { ask = receive; return stop })
  return { connect, stop, ask: (request: QuitRequest) => {
    let reply!: Promise<boolean>
    act(() => { reply = ask!(request) })
    return reply
  } }
}

it.each(["Escape", "Cancel"])("restores the previous keyboard position after cancelling Quit with %s", async (dismissal) => {
  const user = userEvent.setup()
  const connection = fakeConnection()
  render(<><button>Computer action</button><QuitRequestConfirmation connect={connection.connect} /></>)
  await waitFor(() => expect(connection.connect).toHaveBeenCalled())
  const previous = screen.getByRole("button", { name: "Computer action" })
  await user.click(previous)
  const reply = connection.ask({ requestId: 1, computers: ["dev"] })
  const dialog = await screen.findByRole("alertdialog", { name: "Quit Silo?" })
  expect(within(dialog).getByRole("button", { name: "Keep running" })).toHaveFocus()
  await user.tab()
  expect(within(dialog).getByRole("button", { name: "Stop and quit" })).toHaveFocus()
  await user.tab({ shift: true })
  expect(within(dialog).getByRole("button", { name: "Keep running" })).toHaveFocus()
  if (dismissal === "Escape") await user.keyboard("{Escape}")
  else await user.click(within(dialog).getByRole("button", { name: "Cancel" }))
  await expect(reply).resolves.toBe("cancel")
  await waitFor(() => expect(previous).toHaveFocus())
})

it("asks before Quit stops running computers, with the tray's wording, and answers each request", async () => {
  const user = userEvent.setup()
  const connection = fakeConnection()
  render(<QuitRequestConfirmation connect={connection.connect} />)
  await waitFor(() => expect(connection.connect).toHaveBeenCalledTimes(1))

  const quit = connection.ask({ requestId: 1, computers: ["dev", "api"] })
  const dialog = await screen.findByRole("alertdialog", { name: "Quit Silo?" })
  expect(dialog).toHaveAccessibleDescription(/^2 computers are running: dev, api\./)
  expect(within(dialog).getByRole("button", { name: "Keep running" })).toHaveFocus()
  await user.click(within(dialog).getByRole("button", { name: "Keep running" }))
  await expect(quit).resolves.toBe("keep")
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument())

  const stopped = connection.ask({ requestId: 4, computers: ["dev"] })
  await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Stop and quit" }))
  await expect(stopped).resolves.toBe("stop")
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument())

  const cancelled = connection.ask({ requestId: 2, computers: ["dev"] })
  await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Cancel" }))
  await expect(cancelled).resolves.toBe("cancel")

  const escaped = connection.ask({ requestId: 3, computers: ["dev"] })
  await screen.findByRole("alertdialog")
  await user.keyboard("{Escape}")
  await expect(escaped).resolves.toBe("cancel")
})

it("still asks when the running computers could not be read", async () => {
  const connection = fakeConnection()
  render(<QuitRequestConfirmation connect={connection.connect} />)
  await waitFor(() => expect(connection.connect).toHaveBeenCalled())
  void connection.ask({ requestId: 1, computers: [] })
  expect(await screen.findByRole("alertdialog", { name: "Quit Silo?" })).toHaveAccessibleDescription(/could not check which computers are running/)
})

it.each(["Cancel", "Escape"])("restores the previous keyboard position after dismissing Quit with %s", async (dismissal) => {
  const user = userEvent.setup()
  const connection = fakeConnection()
  render(<><input aria-label="Computer search" /><QuitRequestConfirmation connect={connection.connect} /></>)
  await waitFor(() => expect(connection.connect).toHaveBeenCalled())
  const search = screen.getByRole("textbox", { name: "Computer search" })
  await user.click(search)
  const quit = connection.ask({ requestId: 1, computers: ["dev"] })
  const dialog = await screen.findByRole("alertdialog", { name: "Quit Silo?" })
  if (dismissal === "Cancel") await user.click(within(dialog).getByRole("button", { name: "Cancel" }))
  else await user.keyboard("{Escape}")
  await expect(quit).resolves.toBe("cancel")
  await waitFor(() => expect(search).toHaveFocus())
})

it("keeps Silo open and stops listening when the window's UI goes away mid-question", async () => {
  const connection = fakeConnection()
  const view = render(<QuitRequestConfirmation connect={connection.connect} />)
  await waitFor(() => expect(connection.connect).toHaveBeenCalled())
  const quit = connection.ask({ requestId: 1, computers: ["dev"] })
  await screen.findByRole("alertdialog")
  view.unmount()
  await expect(quit).resolves.toBe("cancel")
  expect(connection.stop).toHaveBeenCalled()
})

function MainWindow({ source, connect }: { source: ApplicationSource; connect: ConnectQuitConfirmation }) {
  const { store } = useSettings(source.preferences)
  const [systemIntegrations] = useState(() => createFixtureSystemIntegrationStore(store))
  const backup = useUnavailableBackup(source)
  return <SettingsProvider store={store}><SystemIntegrationProvider store={systemIntegrations}><ApplicationCatalogProvider initialCatalog={fixtureApplicationCatalog}>
    <ApplicationApp source={source} backup={backup} actions={{} as ApplicationActions} connectQuitConfirmation={connect} />
  </ApplicationCatalogProvider></SystemIntegrationProvider></SettingsProvider>
}

it("connects the main window's Quit requests (⌘Q, menus, Dock) to the confirmation", async () => {
  const user = userEvent.setup()
  const connection = fakeConnection()
  render(<MainWindow source={applicationSourceForScenario("running")} connect={connection.connect} />)
  await waitFor(() => expect(connection.connect).toHaveBeenCalledTimes(1))
  const quit = connection.ask({ requestId: 7, computers: ["dev"] })
  await user.click(within(await screen.findByRole("alertdialog", { name: "Quit Silo?" })).getByRole("button", { name: "Stop and quit" }))
  await expect(quit).resolves.toBe("stop")
})
