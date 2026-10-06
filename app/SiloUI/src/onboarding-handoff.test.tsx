import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it } from "vitest"

import { FixtureApp } from "./fixtures/fixture-app"

const originalURL = window.location.href
afterEach(() => window.history.replaceState(null, "", originalURL))

describe("onboarding to application", () => {
  it("finishes default onboarding, exposes permissions, and opens Silo with saved preferences", async () => {
    window.history.replaceState(null, "", "?view=onboarding")
    const user = userEvent.setup()
    render(<FixtureApp />)
    await user.click(screen.getByRole("combobox", { name: "Browser" }))
    await user.click(screen.getByRole("option", { name: "Firefox" }))
    await user.click(screen.getByRole("tab", { name: /Computers/ }))
    await user.click(screen.getByRole("button", { name: `More actions for dev` }))
    await user.click(screen.getByRole("menuitem", { name: `Edit dev` }))
    await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
    await user.click(screen.getByRole("button", { name: "Save" }))
    await user.click(screen.getByRole("tab", { name: /Review/ }))
    expect(screen.getByRole("button", { name: "Finish" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Finish" }))
    expect(screen.getByRole("switch", { name: "Launch Silo at login" })).toBeVisible()
    expect(screen.getByRole("switch", { name: "Enable notifications" })).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Open Silo" }))
    expect(screen.queryByRole("navigation", { name: "Setup steps" })).not.toBeInTheDocument()
    const navigation = within(screen.getByRole("navigation", { name: "Silo navigation" }))
    const list = within(screen.getByRole("list", { name: "Configured computers" }))
    expect(list.getByText("dev", { exact: true })).toBeVisible()
    await user.click(list.getByRole("button", { name: "More actions for dev" }))
    await user.click(screen.getByRole("menuitem", { name: "Edit dev" }))
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    await user.click(navigation.getByRole("button", { name: "Settings" }))
    expect(await screen.findByRole("combobox", { name: "Browser" })).toHaveTextContent("Firefox")
    await user.click(navigation.getByRole("button", { name: "Computers" }))
    await user.click(screen.getByRole("button", { name: "Add" }))
    expect(screen.getByRole("menuitem", { name: "Import computer…" })).toBeVisible()
    expect(new URL(window.location.href).searchParams.get("view")).toBe("app")
  })
})
