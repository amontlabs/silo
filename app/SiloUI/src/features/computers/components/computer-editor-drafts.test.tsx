import { act, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import type { ComponentProps } from "react"
import { TooltipProvider } from "@/components/ui/tooltip"
import { Toaster } from "@/components/ui/sonner"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { ComputerEditorDraftsProvider } from "@/features/computers/model/editor-drafts"
import { ComputerConfigurationList } from "./computer-configuration-list"

type ComputerConfigurationListProps = ComponentProps<typeof ComputerConfigurationList>

const configuration = productionComputerDefaults[0]!

function Surface({ shown, withProvider = true, onConfigurationsChange, onCommitComputer, configurations = [configuration] }: { shown: boolean; withProvider?: boolean; onConfigurationsChange: () => void; onCommitComputer?: ComputerConfigurationListProps["onCommitComputer"]; configurations?: ComputerConfigurationListProps["configurations"] }) {
  const list = shown ? <ComputerConfigurationList configurations={configurations} onConfigurationsChange={onConfigurationsChange} onCommitComputer={onCommitComputer} isComputerCreated={() => true}
    isComputerRunning={() => false} editorDraftKey="computer-list" getRowPresentation={() => ({ menuActions: [] })} /> : <p>Files</p>
  return <TooltipProvider>{withProvider ? <ComputerEditorDraftsProvider>{list}</ComputerEditorDraftsProvider> : list}</TooltipProvider>
}

async function editCpuLimit(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: `More actions for ${configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Edit ${configuration.name}` }))
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
}

describe("unsaved computer edits across navigation", () => {
  it("reports a rejected save after leaving and keeps the unsaved draft", async () => {
    let reject!: (cause: unknown) => void
    const pending = new Promise<void>((_resolve, fail) => { reject = fail })
    const props = { onConfigurationsChange: vi.fn(), onCommitComputer: vi.fn(() => pending) }
    const surface = (shown: boolean) => <><Toaster /><Surface shown={shown} {...props} /></>
    const user = userEvent.setup()
    const { rerender } = render(surface(true))
    await editCpuLimit(user)
    await user.click(screen.getByRole("button", { name: "Save" }))
    rerender(surface(false))
    const error = new Error("This computer changed while your edit was waiting. Review it and try again.")
    await act(async () => reject(error))
    expect(await screen.findByText(`Could not save ${configuration.name}`)).toBeVisible()
    expect(screen.getByText(error.message)).toBeVisible()
    rerender(surface(true))
    expect(screen.getByRole("alert")).toHaveTextContent("This computer changed since you opened it.")
    expect(screen.getByRole("button", { name: "Review changes" })).toBeVisible()
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
    expect(props.onCommitComputer).toHaveBeenCalledTimes(1)
  })

  it("keeps a restored draft editable after a pending save is rejected", async () => {
    let reject!: (cause: unknown) => void
    const pending = new Promise<void>((_resolve, fail) => { reject = fail })
    const props = { onConfigurationsChange: vi.fn(), onCommitComputer: vi.fn().mockReturnValueOnce(pending).mockResolvedValue(undefined) }
    const user = userEvent.setup()
    const { rerender } = render(<Surface shown {...props} />)
    await editCpuLimit(user)
    await user.click(screen.getByRole("button", { name: "Save" }))
    rerender(<Surface shown={false} {...props} />)
    rerender(<Surface shown {...props} />)
    await act(async () => reject(new Error("This computer changed while your edit was waiting. Review it and try again.")))
    expect(screen.getByRole("alert")).toHaveTextContent("This computer changed since you opened it.")
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Review changes" }))
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(props.onCommitComputer).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole("combobox", { name: "CPUs at start" })).not.toBeInTheDocument()
  })

  it("forgets a saved edit when the save completes after leaving", async () => {
    let resolve!: () => void
    const pending = new Promise<void>(done => { resolve = done })
    const props = { onConfigurationsChange: vi.fn(), onCommitComputer: vi.fn(() => pending) }
    const user = userEvent.setup()
    const { rerender } = render(<Surface shown {...props} />)
    await editCpuLimit(user)
    await user.click(screen.getByRole("button", { name: "Save" }))
    rerender(<Surface shown={false} {...props} />)
    await act(async () => resolve())
    rerender(<Surface shown configurations={[{ ...configuration, cpus: 4 }]} {...props} />)
    expect(screen.queryByRole("combobox", { name: "CPUs at start" })).not.toBeInTheDocument()
    expect(props.onCommitComputer).toHaveBeenCalledTimes(1)
  })

  it("keeps a save pending when returning before it completes", async () => {
    let resolve!: () => void
    const pending = new Promise<void>(done => { resolve = done })
    const props = { onConfigurationsChange: vi.fn(), onCommitComputer: vi.fn(() => pending) }
    const user = userEvent.setup()
    const { rerender } = render(<Surface shown {...props} />)
    await editCpuLimit(user)
    await user.click(screen.getByRole("button", { name: "Save" }))
    rerender(<Surface shown={false} {...props} />)
    rerender(<Surface shown {...props} />)
    expect(screen.getByRole("button", { name: "Saving…" })).toBeDisabled()
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toBeDisabled()
    await act(async () => resolve())
    expect(screen.queryByRole("combobox", { name: "CPUs at start" })).not.toBeInTheDocument()
    rerender(<Surface shown={false} {...props} />)
    rerender(<Surface shown {...props} />)
    expect(screen.queryByRole("combobox", { name: "CPUs at start" })).not.toBeInTheDocument()
    expect(props.onCommitComputer).toHaveBeenCalledTimes(1)
  })

  it("keeps an edit when the Add menu is opened and dismissed", async () => {
    const onConfigurationsChange = vi.fn()
    const user = userEvent.setup()
    const { rerender } = render(<Surface shown onConfigurationsChange={onConfigurationsChange} />)
    await editCpuLimit(user)
    await user.click(screen.getByRole("button", { name: "Add" }))
    await user.keyboard("{Escape}")
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
    rerender(<Surface shown={false} onConfigurationsChange={onConfigurationsChange} />)
    rerender(<Surface shown onConfigurationsChange={onConfigurationsChange} />)
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(onConfigurationsChange).toHaveBeenCalledExactlyOnceWith([{ ...configuration, cpus: 4 }], [configuration])
  })

  it("restores the open editor and its baseline after leaving and returning", async () => {
    const onConfigurationsChange = vi.fn()
    const user = userEvent.setup()
    const { rerender } = render(<Surface shown onConfigurationsChange={onConfigurationsChange} />)
    await editCpuLimit(user)
    rerender(<Surface shown={false} onConfigurationsChange={onConfigurationsChange} />)
    rerender(<Surface shown onConfigurationsChange={onConfigurationsChange} />)
    expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
    // The restored edit still saves against the configuration it started from.
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(onConfigurationsChange).toHaveBeenCalledExactlyOnceWith([{ ...configuration, cpus: 4 }], [configuration])
    rerender(<Surface shown={false} onConfigurationsChange={onConfigurationsChange} />)
    rerender(<Surface shown onConfigurationsChange={onConfigurationsChange} />)
    expect(screen.queryByRole("combobox", { name: "CPUs at start" })).not.toBeInTheDocument()
  })

  it("forgets a cancelled edit", async () => {
    const onConfigurationsChange = vi.fn()
    const user = userEvent.setup()
    const { rerender } = render(<Surface shown onConfigurationsChange={onConfigurationsChange} />)
    await editCpuLimit(user)
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    rerender(<Surface shown={false} onConfigurationsChange={onConfigurationsChange} />)
    rerender(<Surface shown onConfigurationsChange={onConfigurationsChange} />)
    expect(screen.queryByRole("combobox", { name: "CPUs at start" })).not.toBeInTheDocument()
  })

  it("keeps nothing outside a provider", async () => {
    const onConfigurationsChange = vi.fn()
    const user = userEvent.setup()
    const { rerender } = render(<Surface shown withProvider={false} onConfigurationsChange={onConfigurationsChange} />)
    await editCpuLimit(user)
    rerender(<Surface shown={false} withProvider={false} onConfigurationsChange={onConfigurationsChange} />)
    rerender(<Surface shown withProvider={false} onConfigurationsChange={onConfigurationsChange} />)
    expect(screen.queryByRole("combobox", { name: "CPUs at start" })).not.toBeInTheDocument()
  })
})


it.each(["conflict", "review"])("restores the %s notice with the unsaved draft after navigation", async notice => {
  const props = { onConfigurationsChange: vi.fn(), onCommitComputer: vi.fn().mockRejectedValue(new Error("This computer changed while your edit was waiting. Review it and try again.")) }
  const user = userEvent.setup()
  const { rerender } = render(<Surface shown {...props} />)
  await editCpuLimit(user)
  const latest = { ...configuration, cpus: 6, maxMemoryGiB: 64 }
  rerender(<Surface shown configurations={[latest]} {...props} />)
  await user.click(screen.getByRole("button", { name: "Save" }))
  await screen.findByRole("button", { name: "Review changes" })
  if (notice === "review") await user.click(screen.getByRole("button", { name: "Review changes" }))
  rerender(<Surface shown={false} configurations={[latest]} {...props} />)
  rerender(<Surface shown configurations={[latest]} {...props} />)
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
  if (notice === "conflict") {
    expect(screen.getByRole("alert")).toHaveTextContent("This computer changed since you opened it.")
    expect(screen.getByRole("button", { name: "Review changes" })).toBeVisible()
  } else {
    expect(screen.getByRole("status", { name: "Review changes" })).toHaveTextContent("CPUs at start: yours 4 CPUs, elsewhere 6 CPUs")
    expect(screen.getByRole("status", { name: "Review changes" })).toHaveTextContent("Updated from elsewhere: Maximum memory.")
  }
})
