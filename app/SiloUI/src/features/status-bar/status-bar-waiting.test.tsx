import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { approvalListSchema } from "@/desktop/production-schemas"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createApprovalFixture } from "@/fixtures/approvals"
import { fixtureDirectoryLoader } from "@/fixtures/directory-loader"
import { StatusBar } from "./status-bar-popover"
import type { StatusBarActions } from "./status-bar-types"

function setup(approvals: Partial<StatusBarActions>) {
  const source = { ...applicationSourceForScenario("complete"), activities: [] }
  const actions: StatusBarActions = {
    listComputerDirectory: fixtureDirectoryLoader(source.computers),
    openSilo: vi.fn(), quit: vi.fn(), refresh: vi.fn(), pushRepository: vi.fn(), dismissRepositoryPush: vi.fn(),
    startComputer: vi.fn(), stopComputer: vi.fn(), restartComputer: vi.fn(),
    openTerminal: vi.fn(), openEditor: vi.fn(), openSite: vi.fn(), ...approvals,
  }
  return { actions, user: userEvent.setup(), ...render(<StatusBar source={source} actions={actions} defaultOpen />) }
}

it("shows one row for everything waiting that opens the Pending page", async () => {
  const fixture = createApprovalFixture()
  const { actions, user } = setup({ listApprovals: fixture.actions.listApprovals, listReviewRequests: fixture.actions.listReviewRequests, watchApprovals: fixture.actions.watchApprovals })
  const row = await screen.findByRole("status", { name: "Waiting for you" })
  expect(row).toHaveTextContent("12 waiting for you")
  expect(row).toHaveTextContent("main: Merge auth-redirect into main")
  await user.click(within(row).getByRole("button", { name: "Review what is waiting for you" }))
  expect(actions.openSilo).toHaveBeenCalledExactlyOnceWith({ tab: "changes" })
})

it("opens the request itself when only one waits", async () => {
  const approvals = approvalListSchema.parse({ approvals: [{ id: "ap-7", kind: "port", computerId: "c", computerName: "dev", summary: "Publish port 8080 on this device", state: "pending", createdAt: new Date().toISOString() }] })
  const { actions, user } = setup({ listApprovals: async () => approvals, listReviewRequests: async () => ({ requests: [], unreachable: [] }) })
  const row = await screen.findByRole("status", { name: "Waiting for you" })
  expect(row).toHaveTextContent("1 waiting for you")
  await user.click(within(row).getByRole("button", { name: "Review what is waiting for you" }))
  expect(actions.openSilo).toHaveBeenCalledExactlyOnceWith({ tab: "changes", approval: "ap-7" })
})

it("shows no row when nothing waits, and when the panel cannot read what waits", async () => {
  const none = setup({ listApprovals: async () => ({ approvals: [] }), listReviewRequests: async () => ({ requests: [], unreachable: [] }) })
  await screen.findByRole("list", { name: "Computers" })
  expect(screen.queryByRole("status", { name: "Waiting for you" })).toBeNull()
  none.unmount()
  setup({})
  await screen.findByRole("list", { name: "Computers" })
  expect(screen.queryByRole("status", { name: "Waiting for you" })).toBeNull()
})
