import { act, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { setupFakeTimerUser } from "@/test/fake-timer-user"
import { ApplicationPreview } from "@/fixtures/application-preview"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"

beforeEach(() => { vi.useFakeTimers({ now: new Date("2026-10-02T12:00:00Z") }) })
afterEach(() => { vi.useRealTimers() })

const navigation = () => within(screen.getByRole("navigation", { name: "Silo navigation" }))

it("mounts a secondary page on its first visit and keeps its state while hidden", async () => {
  const user = setupFakeTimerUser()
  render(<ApplicationPreview source={applicationSourceForScenario("running")} />)
  const secrets = () => document.getElementById("application-panel-secrets")!
  expect(within(secrets()).queryByRole("heading", { name: "Secrets" })).not.toBeInTheDocument()

  await user.click(navigation().getByRole("button", { name: "Secrets" }))
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  const add = within(secrets()).getByRole("button", { name: "Add secret" })
  await user.click(add)
  expect(within(secrets()).getByRole("heading", { name: "Secrets" })).toBeInTheDocument()
  const editor = secrets().querySelector("form, [role=group], input")
  expect(editor).not.toBeNull()

  await user.click(navigation().getByRole("button", { name: "Computers" }))
  expect(secrets()).toHaveAttribute("hidden")
  expect(secrets().contains(editor)).toBe(true)
})
