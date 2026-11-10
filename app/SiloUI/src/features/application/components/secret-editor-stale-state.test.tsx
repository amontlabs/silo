import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { SecretEditor } from "./secret-editor"

describe("SecretEditor current access settings", () => {
  it.each(["domains", "computers"])("requires reloading changed %s before saving a replacement value", (field) => {
    const source = structuredClone(applicationSourceForScenario("running"))
    const secret = source.secrets[0]
    const save = vi.fn()
    const props = { source, onSave: save, onCancel: vi.fn() }
    const { rerender } = render(<SecretEditor secret={secret} {...props} />)
    fireEvent.change(screen.getByLabelText("Replacement value"), { target: { value: "replacement-fixture" } })
    const current = { ...secret, ...(field === "domains" ? { allowedDomains: ["new.example.test"] } : { computers: ["playgrounds"] }) }
    const currentSource = { ...source, secrets: [current] }
    rerender(<SecretEditor secret={current} {...props} source={currentSource} />)

    fireEvent.submit(screen.getByRole("form"))
    expect(save).not.toHaveBeenCalled()
    expect(screen.getByRole("alert")).toHaveTextContent("This secret changed while you were editing.")
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()

    fireEvent.click(screen.getByRole("button", { name: "Reload settings" }))
    expect(screen.getByLabelText("Replacement value")).toHaveValue("replacement-fixture")
    expect(screen.getByRole("textbox", { name: "Allowed domains" })).toHaveValue(current.allowedDomains.join(", "))
    fireEvent.submit(screen.getByRole("form"))
    expect(save).toHaveBeenCalledExactlyOnceWith({ operation: "edit", id: current.id, name: current.name, value: "replacement-fixture", computers: current.computers, allowedDomains: current.allowedDomains })
  })

  it("preserves the draft when only runtime application status changes", () => {
    const source = structuredClone(applicationSourceForScenario("running"))
    const secret = source.secrets[0]
    const save = vi.fn()
    const props = { source, onSave: save, onCancel: vi.fn() }
    const { rerender } = render(<SecretEditor secret={secret} {...props} />)
    fireEvent.change(screen.getByLabelText("Replacement value"), { target: { value: "replacement-fixture" } })
    rerender(<SecretEditor secret={{ ...secret, state: "restart-required" }} {...props} />)
    expect(screen.queryByRole("button", { name: "Reload settings" })).not.toBeInTheDocument()
    fireEvent.submit(screen.getByRole("form"))
    expect(save).toHaveBeenCalledOnce()
  })

  it("clears a GitHub token when it is submitted and asks for it again after a failed save", () => {
    const source = structuredClone(applicationSourceForScenario("running"))
    const save = vi.fn()
    const props = { source, onSave: save, onCancel: vi.fn(), kind: "github-token" as const }
    const { rerender } = render(<SecretEditor {...props} />)
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "work" } })
    fireEvent.change(screen.getByLabelText("Token"), { target: { value: "synthetic_token" } })
    fireEvent.submit(screen.getByRole("form"))
    expect(save).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ kind: "github-token", name: "work", value: "synthetic_token" }))
    expect(screen.getByLabelText("Token")).toHaveValue("")

    rerender(<SecretEditor {...props} saveError="Silo could not reach GitHub to check this token. Try again." />)
    expect(screen.getByLabelText("Token")).toHaveValue("")
    expect(screen.getByLabelText("Name")).toHaveValue("work")
    fireEvent.submit(screen.getByRole("form"))
    expect(save).toHaveBeenCalledOnce()
    expect(screen.getByText("Enter the token again to retry.")).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText("Token"), { target: { value: "synthetic_token_2" } })
    fireEvent.submit(screen.getByRole("form"))
    expect(save).toHaveBeenCalledTimes(2)
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ value: "synthetic_token_2" }))
    expect(screen.getByLabelText("Token")).toHaveValue("")
  })

  it("keeps an ordinary secret value through a failed save", () => {
    const source = structuredClone(applicationSourceForScenario("running"))
    const save = vi.fn()
    render(<SecretEditor source={source} onSave={save} onCancel={vi.fn()} initialComputers={[]} />)
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "SERVICE_TOKEN" } })
    fireEvent.change(screen.getByLabelText("Value"), { target: { value: "synthetic-value" } })
    fireEvent.change(screen.getByRole("textbox", { name: "Allowed domains" }), { target: { value: "api.example.test" } })
    fireEvent.submit(screen.getByRole("form"))
    expect(screen.getByLabelText("Value")).toHaveValue("synthetic-value")
  })
})
