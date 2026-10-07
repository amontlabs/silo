import { render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { EmptyState } from "@/components/empty-state"
import { InlineAlert } from "@/components/inline-alert"
import { ListRow, ListRowIcon, ListRowSkeleton } from "@/components/list-row"
import { PageContainer, PageHeader, SectionHeading } from "@/components/page"
import { progressStatuses, statusTones, type StatusTone } from "@/components/status-tone"
import { Input } from "@/components/ui/input"
import { ReduceMotionContext } from "@/components/ui/reduce-motion"
import { Select, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"

describe("Spinner", () => {
  it("is decorative unless labelled", () => {
    const { rerender } = render(<Spinner data-testid="spinner" />)
    expect(screen.getByTestId("spinner")).toHaveAttribute("aria-hidden", "true")
    rerender(<Spinner label="Saving" />)
    expect(screen.getByRole("img", { name: "Saving" })).not.toHaveAttribute("aria-hidden")
  })

  it("turns unless reduced motion is requested by the app or the caller", () => {
    const { rerender } = render(<Spinner data-testid="spinner" />)
    expect(screen.getByTestId("spinner")).toHaveClass("animate-spin", "motion-reduce:animate-none", "size-3.5")
    rerender(<ReduceMotionContext value><Spinner data-testid="spinner" size="sm" /></ReduceMotionContext>)
    expect(screen.getByTestId("spinner")).not.toHaveClass("animate-spin")
    expect(screen.getByTestId("spinner")).toHaveClass("size-3")
    rerender(<Spinner data-testid="spinner" reduceMotion />)
    expect(screen.getByTestId("spinner")).not.toHaveClass("animate-spin")
  })
})

describe("status tones", () => {
  it("gives every tone text, dot, chip, row and notice classes from theme tokens", () => {
    const tones: StatusTone[] = ["success", "warning", "danger", "neutral"]
    for (const tone of tones) for (const value of Object.values(statusTones[tone])) expect(value).not.toMatch(/emerald|amber|green|red-/)
  })

  it("maps every progress status to a tone and label", () => {
    expect(progressStatuses.running).toEqual({ tone: "warning", label: "In progress" })
    expect(progressStatuses.failed.tone).toBe("danger")
  })
})

describe("InlineAlert", () => {
  it("is an alert, or a status when asked, with the tone's tint", () => {
    const { rerender } = render(<InlineAlert>Something failed</InlineAlert>)
    expect(screen.getByRole("alert")).toHaveClass("border-destructive/30", "bg-destructive/10")
    rerender(<InlineAlert tone="warning" role="status">Heads up</InlineAlert>)
    expect(screen.getByRole("status")).toHaveClass("border-warning/30", "bg-warning/10")
  })

  it("shows long runtime output behind details", () => {
    render(<InlineAlert error={{ message: `Failed\n${"line\n".repeat(30)}`, fallbackSummary: "Failed" }} />)
    expect(screen.getByRole("alert")).toHaveTextContent("Failed")
    expect(screen.getByRole("button", { name: "Show details" })).toBeVisible()
  })
})

describe("page layout", () => {
  it("renders a title, subtitle and actions with section headings below it", () => {
    render(<PageContainer data-testid="page"><PageHeader title="Secrets" subtitle="2 configured" actions={<button>Add</button>} /><SectionHeading>Group</SectionHeading></PageContainer>)
    expect(screen.getByTestId("page")).toHaveClass("mx-auto", "max-w-4xl")
    expect(screen.getByRole("heading", { level: 2, name: "Secrets" })).toHaveClass("text-sm")
    expect(screen.getByRole("heading", { level: 3, name: "Group" })).toHaveClass("text-xs")
    expect(screen.getByText("2 configured")).toBeVisible()
    expect(screen.getByRole("button", { name: "Add" })).toBeVisible()
  })
})

describe("rows and empty states", () => {
  it("renders a list row without a detail line", () => {
    const { container } = render(<ListRow icon={<ListRowIcon />} title="Only a title" />)
    expect(screen.getByText("Only a title")).toBeVisible()
    expect(container.querySelector(".text-muted-foreground.truncate")).toBeNull()
  })

  it("renders a skeleton row with a status name", () => {
    render(<ListRowSkeleton label="Loading things" />)
    expect(screen.getByRole("status", { name: "Loading things" })).toBeVisible()
  })

  it("has an unframed inline variant", () => {
    const { rerender } = render(<EmptyState title="Nothing here" />)
    expect(screen.getByText("Nothing here").closest("[data-slot=empty-state]")).toHaveClass("border-dashed")
    rerender(<EmptyState variant="inline" title="Nothing here" />)
    expect(screen.getByText("Nothing here").closest("[data-slot=empty-state]")).not.toHaveClass("border-dashed")
  })
})

describe("fields", () => {
  it("shares one height scale between inputs and select triggers", () => {
    render(<>
      <Input aria-label="Default" />
      <Input aria-label="Small" size="sm" />
      <Select><SelectTrigger aria-label="Pick" size="sm"><SelectValue /></SelectTrigger></Select>
    </>)
    expect(screen.getByLabelText("Default")).toHaveClass("h-8", "rounded-md", "text-xs")
    expect(screen.getByLabelText("Small")).toHaveClass("h-7")
    expect(screen.getByRole("combobox", { name: "Pick" })).toHaveClass("h-7", "rounded-md", "text-xs")
  })
})
