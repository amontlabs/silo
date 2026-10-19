import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { lazyPage } from "./lazy-page"
import { LazyBoundary } from "./panel-content"

it("offers Retry after a failed chunk load and loads the page on the next attempt", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {})
  const load = vi.fn()
    .mockRejectedValueOnce(new Error("Failed to fetch module"))
    .mockResolvedValue({ Page: () => <p>Loaded page</p> })
  const page = lazyPage(load as () => Promise<{ Page: () => React.JSX.Element }>, "Page")
  const Page = page.Component
  render(<LazyBoundary><Page /></LazyBoundary>)
  expect(await screen.findByRole("alert")).toHaveTextContent("This page could not be loaded.")
  await userEvent.setup().click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByText("Loaded page")).toBeInTheDocument()
  expect(load).toHaveBeenCalledTimes(2)
})

it("forgets a rejected preload so it can be fetched again", async () => {
  const load = vi.fn<() => Promise<{ Page: () => null }>>().mockRejectedValueOnce(new Error("offline")).mockResolvedValue({ Page: () => null })
  const page = lazyPage(load, "Page")
  await expect(page.preload()).rejects.toThrow("offline")
  await expect(page.preload()).resolves.toBeDefined()
  expect(load).toHaveBeenCalledTimes(2)
})
