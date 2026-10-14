import { useState } from "react"
import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { validateComputerName } from "@/features/onboarding/model/computer-configuration"
import { ImportPopover, type ImportReview } from "./import-popover"

describe("import name validation accessibility", () => {
  it.each(["dev", "invalid name"])("keeps the name label stable and describes the error for %s", async (newName) => {
    render(<ImportPopover
      source={applicationSourceForScenario("running")}
      review={{ kind: "review", archive: { name: "export", archivePath: "/fixtures/export", completedLabel: "Today", size: "1 GiB", destination: "/fixtures", computers: ["dev"] }, sourceName: "dev", newName }}
      anchor={<button type="button">Add</button>}
      onReview={vi.fn()} onImport={vi.fn()} onClose={vi.fn()} onRetry={vi.fn()}
    />)
    const input = await screen.findByRole("textbox", { name: /New computer name/ })
    expect(input).toHaveAccessibleName("New computer name")
    expect(input).toHaveAttribute("aria-invalid", "true")
    expect(input).toHaveAccessibleDescription(validateComputerName(newName) ?? `A computer named ${newName} already exists.`)
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled()
  })

  it("shows portability warnings before the user starts an import", async () => {
    render(<ImportPopover
      source={applicationSourceForScenario("running")}
      review={{ kind: "review", archive: { name: "export", archivePath: "/fixtures/export", completedLabel: "Today", size: "1 GiB", destination: "/fixtures", computers: ["dev"] }, sourceName: "dev", newName: "dev-imported", warnings: ["Saved memory cannot be reused on this Mac; the computer will import from its disk."] }}
      anchor={<button type="button">Add</button>}
      onReview={vi.fn()} onImport={vi.fn()} onClose={vi.fn()} onRetry={vi.fn()}
    />)
    expect(await screen.findByRole("note")).toHaveTextContent(/Warning: Saved memory cannot be reused/)
  })
})

it.each([
  { review: { kind: "checking" } as ImportReview, focus: "Cancel" },
  { review: { kind: "invalid", reason: "Unsupported archive" } as ImportReview, focus: "Choose another file" },
])("focuses an available action while the import review is $review.kind", async ({ review: initialReview, focus }) => {
  const user = userEvent.setup()
  const onImport = vi.fn()
  const onRetry = vi.fn()
  function Harness() {
    const [review, setReview] = useState<ImportReview | null>(null)
    return <ImportPopover source={applicationSourceForScenario("running")} review={review}
      anchor={<button onClick={() => setReview(initialReview)}>Add computer</button>}
      onReview={setReview} onImport={onImport} onClose={() => setReview(null)} onRetry={onRetry} />
  }
  render(<Harness />)
  const add = screen.getByRole("button", { name: "Add computer" })
  await user.click(add)
  await waitFor(() => expect(screen.getByRole("button", { name: focus })).toHaveFocus())
  await user.keyboard("{Enter}")
  if (initialReview.kind === "invalid") {
    expect(onRetry).toHaveBeenCalledOnce()
    await user.keyboard("{Escape}")
  }
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument())
  expect(add).toHaveFocus()
  expect(onImport).not.toHaveBeenCalled()
})
