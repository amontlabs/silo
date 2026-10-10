import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { SecretsPage } from "@/features/application/pages/secrets-page"
import { useApplicationFixture } from "@/fixtures/application-state"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"

function SecretsPreview({ source }: { source: ApplicationSource }) {
  const fixture = useApplicationFixture(source)
  return <SecretsPage source={fixture.source} onSaveSecret={fixture.saveSecret} onRemoveSecret={fixture.removeSecret} />
}

describe("SecretsPage", () => {
  it("explains an oversized replacement before submitting it to the native controller", async () => {
    const user = userEvent.setup()
    const save = vi.fn()
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={save} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    const value = screen.getByLabelText("Replacement value")
    fireEvent.change(value, { target: { value: "é".repeat(32769) } })
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).not.toHaveBeenCalled()
    expect(screen.getByRole("alert")).toHaveTextContent("64 KiB")
    expect(value).toHaveFocus()
    expect(value).toHaveValue("é".repeat(32769))
  })

  it.each([true, false])("keeps secret assignment badges local when a same-named remote exists (local present: %s)", (localPresent) => {
    const source = structuredClone(applicationSourceForScenario("running"))
    const local = source.computers.find(({ configuration, device }) => configuration.name === "dev" && !device)!
    local.state = "running"
    const remote = {
      ...local, state: "failed" as const,
      device: { id: "office", computerId: "remote-computer", name: "Office", address: "office", connected: true },
    }
    source.computers = [remote, ...source.computers.filter(computer => localPresent || computer !== local)]
    source.secrets = [{ ...source.secrets[0], computers: ["dev"] }]
    render(<SecretsPage source={source} onSaveSecret={vi.fn()} onRemoveSecret={vi.fn()} />)
    const assignments = screen.getByRole("group", { name: "Computers for PACKAGE_TOKEN" })
    expect(assignments).toHaveTextContent(/^dev$/)
    if (localPresent) expect(within(assignments).getByLabelText("dev, Running")).toBeVisible()
    else expect(within(assignments).queryByLabelText(/dev,/)).not.toBeInTheDocument()
  })

  it("requires a new wildcard acknowledgement after the allowed domains change", async () => {
    const user = userEvent.setup()
    const save = vi.fn()
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={save} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    const domains = screen.getByRole("textbox", { name: "Allowed domains" })
    fireEvent.change(domains, { target: { value: "*" } })
    await user.click(screen.getByRole("checkbox", { name: "Allow any HTTPS destination" }))
    fireEvent.change(domains, { target: { value: "api.example.test" } })
    fireEvent.change(domains, { target: { value: "*" } })
    expect(screen.getByRole("checkbox", { name: "Allow any HTTPS destination" })).not.toBeChecked()
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).not.toHaveBeenCalled()
    expect(screen.getByRole("alert")).toHaveTextContent("Confirm access to any HTTPS destination.")
    await user.click(screen.getByRole("checkbox", { name: "Allow any HTTPS destination" }))
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ allowedDomains: ["*"], operation: "edit" }))
  })

  it("keeps a pending secret save open on Escape and ignores a second submission", async () => {
    const user = userEvent.setup()
    let finish!: () => void
    const save = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={save} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    await user.type(screen.getByLabelText("Replacement value"), "fixture-replacement")
    await user.click(screen.getByRole("button", { name: "Save" }))
    const form = screen.getByRole("form", { name: "Edit PACKAGE_TOKEN" })
    fireEvent.keyDown(form, { key: "Escape" })
    fireEvent.submit(form)
    expect(form).toBeVisible()
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled()
    expect(save).toHaveBeenCalledOnce()
    await act(async () => finish())
    expect(screen.queryByRole("form")).not.toBeInTheDocument()
  })

  it("offers only local virtual configurations when assigning a secret", async () => {
    const user = userEvent.setup()
    const source = structuredClone(applicationSourceForScenario("running"))
    const remote = { ...source.computers[0], configuration: { ...source.computers[0].configuration, name: "remote-only" }, device: { id: "office", computerId: "remote-computer", name: "Office", address: "office", connected: true } }
    source.computers.push(remote)
    render(<SecretsPage source={source} onSaveSecret={vi.fn()} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Add secret" }))
    await user.click(screen.getByRole("combobox", { name: "Add computer" }))
    expect(screen.getAllByRole("option").map(option => option.textContent)).toEqual(source.computers.filter(computer => !computer.device).map(computer => computer.configuration.name))
    expect(screen.queryByRole("option", { name: "remote-only" })).not.toBeInTheDocument()
  })

  it("keeps Edit and Remove available while computer secret application is pending", () => {
    const source = applicationSourceForScenario("running")
    const applying = { ...source, secrets: source.secrets.map((secret) => ({ ...secret, state: "applying" as const })) }
    const props = { onSaveSecret: vi.fn(), onRemoveSecret: vi.fn() }
    const { rerender } = render(<SecretsPage source={applying} {...props} />)
    expect(screen.getAllByText("Applying…").length).toBeGreaterThan(0)
    expect(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })).toBeEnabled()
    expect(screen.getByRole("button", { name: "Remove PACKAGE_TOKEN" })).toBeEnabled()
    rerender(<SecretsPage source={source} {...props} />)
    expect(screen.queryByText("Applying…")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })).toBeEnabled()
  })

  it("removes an applying secret even when its computer cannot be reached, blocking only the in-flight request", async () => {
    const user = userEvent.setup()
    const source = structuredClone(applicationSourceForScenario("running"))
    source.secrets[0] = { ...source.secrets[0], state: "applying", removing: true, error: "Could not reach dev." }
    source.computers[0].freshness = "stale"
    let finish!: () => void
    const remove = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
    render(<SecretsPage source={source} onSaveSecret={vi.fn()} onRemoveSecret={remove} />)
    expect(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Remove PACKAGE_TOKEN" }))
    await user.click(screen.getByRole("button", { name: /^Remove$/ }))
    expect(remove).toHaveBeenCalledExactlyOnceWith("package-token")
    expect(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Remove PACKAGE_TOKEN" })).toBeDisabled()
    await act(async () => finish())
    expect(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })).toBeEnabled()
  })

  it("closes the removal confirmation on one Escape after hovering its trigger", async () => {
    const user = userEvent.setup()
    const remove = vi.fn()
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={vi.fn()} onRemoveSecret={remove} />)
    const trigger = screen.getByRole("button", { name: "Remove PACKAGE_TOKEN" })
    await user.hover(trigger)
    await user.click(trigger)
    expect(screen.getByText("Remove PACKAGE_TOKEN?")).toBeVisible()
    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByText("Remove PACKAGE_TOKEN?")).not.toBeInTheDocument())
    expect(trigger).toHaveFocus()
    expect(remove).not.toHaveBeenCalled()
  })

  it("keeps a failed save draft across native refresh, then closes after successful retry", async () => {
    const user = userEvent.setup()
    let reject!: (error: Error) => void
    const save = vi.fn().mockImplementationOnce(() => new Promise<void>((_, fail) => { reject = fail })).mockResolvedValue(undefined)
    const source = applicationSourceForScenario("running")
    const { rerender } = render(<SecretsPage source={source} onSaveSecret={save} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    await user.type(screen.getByLabelText("Replacement value"), "private-test-value")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(screen.getByRole("button", { name: "Saving…" })).toBeDisabled()
    expect(screen.getByLabelText("Replacement value")).toBeDisabled()
    rerender(<SecretsPage source={structuredClone(source)} onSaveSecret={save} onRemoveSecret={vi.fn()} />)
    expect(screen.getByRole("form")).toBeVisible()
    await act(async () => reject(new Error("private-test-value")))
    expect(screen.getByRole("alert")).toHaveTextContent("Could not save this secret.")
    expect(screen.getByRole("alert")).not.toHaveTextContent("private-test-value")
    expect(screen.getByLabelText("Replacement value")).toHaveValue("private-test-value")
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(save).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole("form")).not.toBeInTheDocument()
  })

  it("explains when credential storage needs to be unlocked", async () => {
    const user = userEvent.setup()
    const save = vi.fn().mockRejectedValue("Cannot access secrets in the system credential store. Unlock it and retry.")
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={save} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    await user.type(screen.getByLabelText("Replacement value"), "test-value")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(screen.getByRole("alert")).toHaveTextContent("Unlock it and retry.")
    expect(screen.getByRole("form")).toBeVisible()
  })

  it.each([
    "Secret settings are too large. Reduce assignments or allowed domains and retry. No settings were overwritten.",
    "A selected computer was removed while saving this secret. Select computers again and retry.",
  ])("preserves an actionable native save failure without discarding the draft: %s", async message => {
    const user = userEvent.setup()
    const save = vi.fn().mockRejectedValue(message)
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={save} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    await user.type(screen.getByLabelText("Replacement value"), "fixture-replacement")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(screen.getByRole("alert")).toHaveTextContent(message)
    expect(screen.getByLabelText("Replacement value")).toHaveValue("fixture-replacement")
    expect(screen.getByRole("form")).toBeVisible()
  })

  it("retries a failed removal without claiming the secret disappeared", async () => {
    const user = userEvent.setup()
    const remove = vi.fn().mockRejectedValueOnce(new Error("private failure")).mockResolvedValue(undefined)
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={vi.fn()} onRemoveSecret={remove} />)
    await user.click(screen.getByRole("button", { name: "Remove PACKAGE_TOKEN" }))
    await user.click(screen.getByRole("button", { name: /^Remove$/ }))
    expect(screen.getByText("PACKAGE_TOKEN")).toBeVisible()
    expect(screen.getByRole("alert")).toHaveTextContent("Could not update this secret.")
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(remove).toHaveBeenCalledTimes(2)
  })

  it("shows the affected computers and retries partial runtime application", async () => {
    const user = userEvent.setup()
    const source = structuredClone(applicationSourceForScenario("running"))
    source.secrets[0] = { ...source.secrets[0], state: "restart-required", pendingComputers: ["dev"], error: "Could not apply access to playgrounds." }
    const retry = vi.fn().mockResolvedValue(undefined)
    render(<SecretsPage source={source} onSaveSecret={vi.fn()} onRemoveSecret={vi.fn()} onRetrySecret={retry} />)
    expect(screen.getByText("Restart to apply: dev")).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(retry).toHaveBeenCalledExactlyOnceWith("package-token")
  })

  it.each(["Save", "Cancel", "Escape"])("restores focus after %s without reopening the Edit tooltip", async (action) => {
    const user = userEvent.setup()
    render(<SecretsPreview source={applicationSourceForScenario("running")} />)
    const edit = screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })
    await user.click(edit)
    await user.type(screen.getByLabelText("Replacement value"), "fixture-replacement")
    if (action === "Escape") await user.keyboard("{Escape}")
    else await user.click(screen.getByRole("button", { name: action }))

    expect(screen.queryByRole("form")).not.toBeInTheDocument()
    expect(edit).toHaveFocus()
    expect(screen.queryByRole("tooltip", { name: "Edit PACKAGE_TOKEN" })).not.toBeInTheDocument()

    // Returning focus should stay quiet; deliberately focusing Edit still helps keyboard users.
    await user.tab()
    await user.tab({ shift: true })
    expect(edit).toHaveFocus()
    expect(screen.getByRole("tooltip", { name: "Edit PACKAGE_TOKEN" })).toBeVisible()
  })

  it("adds a secret through the fixture and reopens its metadata without loading the value", async () => {
    const user = userEvent.setup()
    render(<SecretsPreview source={applicationSourceForScenario("running")} />)

    await user.click(screen.getByRole("button", { name: "Add secret" }))
    const form = within(screen.getByRole("form", { name: "Add secret" }))
    expect(form.getByRole("textbox", { name: "Name" })).toHaveFocus()
    await user.type(form.getByRole("textbox", { name: "Name" }), "SERVICE_TOKEN")
    expect(form.getByLabelText("Value")).toHaveAttribute("type", "password")
    await user.type(form.getByLabelText("Value"), "fixture-token")
    await user.click(form.getByRole("combobox", { name: "Add computer" }))
    await user.click(screen.getByRole("option", { name: "dev" }))
    await user.type(form.getByRole("textbox", { name: "Allowed domains" }), "API.Example.test, *.example.test")
    await user.click(form.getByRole("button", { name: "Save" }))

    expect(screen.queryByRole("form")).not.toBeInTheDocument()
    expect(screen.getByText("3 configured")).toBeVisible()
    expect(screen.getByLabelText("Allowed domains for SERVICE_TOKEN")).toHaveTextContent("api.example.test, *.example.test")
    await user.click(screen.getByRole("button", { name: "Edit SERVICE_TOKEN" }))
    const editor = within(screen.getByRole("form", { name: "Edit SERVICE_TOKEN" }))
    expect(editor.getByRole("textbox", { name: "Name" })).toHaveValue("SERVICE_TOKEN")
    expect(editor.getByLabelText("Replacement value")).toHaveValue("")
    expect(editor.getByRole("button", { name: "Remove dev" })).toBeVisible()
    expect(editor.getByRole("textbox", { name: "Allowed domains" })).toHaveValue("api.example.test, *.example.test")
  })

  it("requests removal after confirmation and waits for the source to publish the change", async () => {
    const user = userEvent.setup()
    const source = applicationSourceForScenario("running")
    const onRemoveSecret = vi.fn()
    const { rerender } = render(<SecretsPage source={source} onSaveSecret={vi.fn()} onRemoveSecret={onRemoveSecret} />)
    await user.click(screen.getByRole("button", { name: "Remove PACKAGE_TOKEN" }))
    expect(screen.getByText("Remove PACKAGE_TOKEN?")).toBeVisible()
    expect(onRemoveSecret).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: /^Remove$/ }))
    expect(onRemoveSecret).toHaveBeenCalledExactlyOnceWith("package-token")
    expect(screen.getByText("PACKAGE_TOKEN")).toBeVisible()

    rerender(<SecretsPage source={{ ...source, secrets: source.secrets.filter(({ id }) => id !== "package-token") }} onSaveSecret={vi.fn()} onRemoveSecret={onRemoveSecret} />)
    expect(screen.queryByText("PACKAGE_TOKEN")).not.toBeInTheDocument()
  })

  it("validates required fields, duplicate names, and domain rules before publishing a save", async () => {
    const user = userEvent.setup()
    const onSaveSecret = vi.fn()
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={onSaveSecret} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Add secret" }))
    const form = within(screen.getByRole("form", { name: "Add secret" }))
    await user.click(form.getByRole("button", { name: "Save" }))
    expect(form.getAllByRole("alert")).toHaveLength(4)
    expect(form.getByRole("textbox", { name: "Name" })).toHaveFocus()

    await user.type(form.getByRole("textbox", { name: "Name" }), "PACKAGE_TOKEN")
    await user.type(form.getByLabelText("Value"), "fixture-token")
    await user.click(form.getByRole("combobox", { name: "Add computer" }))
    await user.click(screen.getByRole("option", { name: "personal" }))
    await user.type(form.getByRole("textbox", { name: "Allowed domains" }), "https://api.example.test/path")
    await user.click(form.getByRole("button", { name: "Save" }))
    expect(form.getByText("A secret with this name already exists.")).toBeVisible()
    expect(form.getByRole("textbox", { name: "Allowed domains" })).toHaveAttribute("aria-invalid", "true")
    expect(onSaveSecret).not.toHaveBeenCalled()

    await user.clear(form.getByRole("textbox", { name: "Name" }))
    await user.type(form.getByRole("textbox", { name: "Name" }), "SERVICE_TOKEN")
    await user.clear(form.getByRole("textbox", { name: "Allowed domains" }))
    await user.type(form.getByRole("textbox", { name: "Allowed domains" }), "API.Example.test, api.example.test")
    await user.keyboard("{Enter}")
    expect(onSaveSecret).toHaveBeenCalledExactlyOnceWith({ operation: "add", name: "SERVICE_TOKEN", value: "fixture-token", computers: ["personal"], allowedDomains: ["api.example.test"] })
    // The page waits for the source to publish the saved metadata.
    expect(screen.getByText("2 configured")).toBeVisible()
    expect(screen.getByRole("button", { name: "Add secret" })).toHaveFocus()
  })

  it("keeps the existing value on metadata edits and sends a replacement only when entered", async () => {
    const user = userEvent.setup()
    const onSaveSecret = vi.fn()
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={onSaveSecret} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    let form = within(screen.getByRole("form", { name: "Edit PACKAGE_TOKEN" }))
    expect(form.getByRole("textbox", { name: "Name" })).toBeDisabled()
    expect(form.getByLabelText("Replacement value")).toHaveFocus()
    await user.click(form.getByRole("combobox", { name: "Add computer" }))
    await user.click(screen.getByRole("option", { name: "personal" }))
    await user.click(form.getByRole("button", { name: "Save" }))
    expect(onSaveSecret).toHaveBeenLastCalledWith({ operation: "edit", id: "package-token", name: "PACKAGE_TOKEN", computers: ["dev", "playgrounds", "personal"], allowedDomains: ["registry.npmjs.org"] })
    expect(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })).toHaveFocus()

    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    form = within(screen.getByRole("form", { name: "Edit PACKAGE_TOKEN" }))
    await user.type(form.getByLabelText("Replacement value"), "replacement-fixture")
    await user.click(form.getByRole("button", { name: "Save" }))
    expect(onSaveSecret).toHaveBeenLastCalledWith(expect.objectContaining({ operation: "edit", id: "package-token", value: "replacement-fixture" }))
  })

  it("closes an unchanged edit without staging a restart", async () => {
    const user = userEvent.setup()
    const onSaveSecret = vi.fn()
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={onSaveSecret} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(onSaveSecret).not.toHaveBeenCalled()
    expect(screen.queryByRole("form")).not.toBeInTheDocument()
  })

  it("searches and clears computer selections without submitting or dismissing the editor", async () => {
    const user = userEvent.setup()
    const onSaveSecret = vi.fn()
    render(<SecretsPage source={applicationSourceForScenario("running")} onSaveSecret={onSaveSecret} onRemoveSecret={vi.fn()} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    const form = within(screen.getByRole("form", { name: "Edit PACKAGE_TOKEN" }))
    await user.click(form.getByRole("button", { name: "Clear" }))
    expect(onSaveSecret).not.toHaveBeenCalled()
    expect(form.queryByRole("button", { name: "Remove dev" })).not.toBeInTheDocument()

    const input = form.getByRole("combobox", { name: "Add computer" })
    await user.type(input, "personal")
    await user.keyboard("{Enter}")
    expect(form.getByRole("button", { name: "Remove personal" })).toBeVisible()
    expect(onSaveSecret).not.toHaveBeenCalled()
    await user.type(input, "unmatched")
    await user.keyboard("{Enter}")
    expect(onSaveSecret).not.toHaveBeenCalled()
    await user.keyboard("{Escape}")
    expect(input).toHaveAttribute("aria-expanded", "false")
    expect(screen.getByRole("form", { name: "Edit PACKAGE_TOKEN" })).toBeVisible()
    await user.keyboard("{Escape}")
    expect(screen.queryByRole("form")).not.toBeInTheDocument()
  })

  it("requires acknowledgement before allowing every HTTPS destination", async () => {
    const user = userEvent.setup()
    render(<SecretsPreview source={applicationSourceForScenario("running")} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    const form = within(screen.getByRole("form", { name: "Edit PACKAGE_TOKEN" }))
    await user.clear(form.getByRole("textbox", { name: "Allowed domains" }))
    await user.type(form.getByRole("textbox", { name: "Allowed domains" }), "*")
    await user.click(form.getByRole("button", { name: "Save" }))
    expect(form.getByRole("alert")).toHaveTextContent("Confirm access to any HTTPS destination.")
    await user.click(form.getByRole("checkbox", { name: "Allow any HTTPS destination" }))
    await user.click(form.getByRole("button", { name: "Save" }))
    expect(screen.queryByRole("form")).not.toBeInTheDocument()
    expect(screen.getByLabelText("Allowed domains for PACKAGE_TOKEN")).toHaveTextContent("*")
  })

  it("discards cancelled edits and values, with Escape and Cancel returning focus to the trigger", async () => {
    const user = userEvent.setup()
    render(<SecretsPreview source={applicationSourceForScenario("running")} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    await user.type(screen.getByLabelText("Replacement value"), "discard-fixture")
    await user.clear(screen.getByRole("textbox", { name: "Allowed domains" }))
    await user.keyboard("{Escape}")
    expect(screen.queryByRole("form")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })).toHaveFocus()
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    expect(screen.getByLabelText("Replacement value")).toHaveValue("")
    expect(screen.getByRole("textbox", { name: "Allowed domains" })).toHaveValue("registry.npmjs.org")
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(screen.queryByRole("form")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" })).toHaveFocus()
  })

  it("requires confirmation and lets Cancel or Escape dismiss removal", async () => {
    const user = userEvent.setup()
    render(<SecretsPreview source={applicationSourceForScenario("running")} />)
    const list = within(screen.getByRole("list", { name: "Configured secrets" }))
    const confirm = () => screen.getByRole("button", { name: /^Remove$/ })

    await user.click(list.getByRole("button", { name: "Remove PACKAGE_TOKEN" }))
    expect(screen.getByText("Remove PACKAGE_TOKEN?")).toBeVisible()
    expect(screen.getByText("Silo deletes the stored value immediately. Computers that cannot revoke access may keep it until they restart.")).toBeVisible()
    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByText("Remove PACKAGE_TOKEN?")).not.toBeInTheDocument())
    expect(list.getAllByRole("listitem")).toHaveLength(2)
    await user.click(list.getByRole("button", { name: "Remove PACKAGE_TOKEN" }))
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    await waitFor(() => expect(screen.queryByText("Remove PACKAGE_TOKEN?")).not.toBeInTheDocument())
    expect(list.getAllByRole("listitem")).toHaveLength(2)

    await user.click(list.getByRole("button", { name: "Remove PACKAGE_TOKEN" }))
    await user.click(confirm())
    expect(list.queryByText("PACKAGE_TOKEN")).not.toBeInTheDocument()
    expect(list.getByText("DATABASE_URL")).toBeVisible()
    expect(list.getByText("Restart to apply")).toBeVisible()
    expect(screen.getByText("1 configured")).toBeVisible()

    await user.click(list.getByRole("button", { name: "Remove DATABASE_URL" }))
    await user.click(confirm())
    expect(screen.getByText("No secrets configured")).toBeVisible()
    expect(screen.getByText("0 configured")).toBeVisible()
    expect(screen.getByRole("button", { name: "Add secret" })).toBeVisible()
  })

  it("preserves local changes when a status snapshot republishes unchanged secret metadata", async () => {
    const user = userEvent.setup()
    const source = applicationSourceForScenario("running")
    const { rerender } = render(<SecretsPreview source={source} />)
    await user.click(screen.getByRole("button", { name: "Edit PACKAGE_TOKEN" }))
    await user.clear(screen.getByRole("textbox", { name: "Allowed domains" }))
    await user.type(screen.getByRole("textbox", { name: "Allowed domains" }), "packages.example.test")
    await user.click(screen.getByRole("button", { name: "Save" }))
    rerender(<SecretsPreview source={structuredClone(source)} />)
    expect(screen.getByLabelText("Allowed domains for PACKAGE_TOKEN")).toHaveTextContent("packages.example.test")
  })

  it("replaces preview removals when the source metadata changes", async () => {
    const user = userEvent.setup()
    const source = applicationSourceForScenario("running")
    const { rerender } = render(<SecretsPreview source={source} />)
    await user.click(screen.getByRole("button", { name: "Remove PACKAGE_TOKEN" }))
    await user.click(screen.getByRole("button", { name: /^Remove$/ }))

    rerender(<SecretsPreview source={{ ...source, secrets: source.secrets.map((secret) => ({ ...secret, state: "active" })) }} />)
    expect(screen.getByText("2 configured")).toBeVisible()
    expect(screen.getByRole("button", { name: "Remove PACKAGE_TOKEN" })).toBeVisible()
  })
})
