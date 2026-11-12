import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { SecretEditor } from "./secret-editor"

function editor() {
  const source = structuredClone(applicationSourceForScenario("running"))
  const secret = { ...source.secrets[0], allowedDomains: ["*.netlify.app"] }
  const save = vi.fn()
  render(<SecretEditor secret={secret} source={{ ...source, secrets: [secret] }} onSave={save} onCancel={vi.fn()} />)
  fireEvent.change(screen.getByLabelText("Replacement value"), { target: { value: "replacement-fixture" } })
  return { save, secret }
}

describe("SecretEditor shared-hosting hosts", () => {
  it("warns about a wildcard over shared hosting and saves without confirming it by default", () => {
    const { save, secret } = editor()
    expect(screen.getByText(/lets this computer reach any site hosted under netlify\.app/)).toBeInTheDocument()
    expect(screen.getByRole("checkbox", { name: "Let computers reach every site under these" })).not.toBeChecked()
    fireEvent.submit(screen.getByRole("form"))
    expect(save).toHaveBeenCalledExactlyOnceWith({ operation: "edit", id: secret.id, name: secret.name, value: "replacement-fixture", computers: secret.computers, allowedDomains: ["*.netlify.app"] })
  })

  it("sends the confirmation once the person ticks the box", () => {
    const { save } = editor()
    fireEvent.click(screen.getByRole("checkbox", { name: "Let computers reach every site under these" }))
    fireEvent.submit(screen.getByRole("form"))
    expect(save).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ allowSharedHosting: true }))
  })

  it("shows no warning for ordinary hosts", () => {
    const source = structuredClone(applicationSourceForScenario("running"))
    render(<SecretEditor secret={source.secrets[0]} source={source} onSave={vi.fn()} onCancel={vi.fn()} />)
    expect(screen.queryByRole("checkbox", { name: "Let computers reach every site under these" })).not.toBeInTheDocument()
  })
})
