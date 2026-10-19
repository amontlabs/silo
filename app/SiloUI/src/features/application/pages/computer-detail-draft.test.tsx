import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { ComputerEditorDraftsProvider } from "@/features/computers/model/editor-drafts"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions } from "../model/application-source"
import { OverviewPage } from "./overview-page"

it("keeps an edited computer's changes when Escape is pressed after leaving and returning to its page", async () => {
  const user = userEvent.setup()
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  render(<TooltipProvider><ComputerEditorDraftsProvider><OverviewPage source={source} actions={{} as ApplicationActions} onConfigurationsChange={vi.fn()} /></ComputerEditorDraftsProvider></TooltipProvider>)
  const open = () => user.click(within(screen.getByText("dev").closest("li")!).getByRole("button", { name: "Open dev" }))
  await open()
  await user.click(screen.getByRole("button", { name: "Edit" }))
  const cpus = screen.getByRole("combobox", { name: "CPUs at start" }) as HTMLSelectElement
  const other = [...cpus.options].map(option => option.value).find(value => value !== cpus.value)!
  await user.selectOptions(cpus, other)
  await user.click(within(screen.getByRole("navigation", { name: "Breadcrumb" })).getByRole("button", { name: "Computers" }))
  await open()
  await user.click(screen.getByRole("textbox", { name: "Computer name" }))
  await user.keyboard("{Escape}")
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue(other)
})
