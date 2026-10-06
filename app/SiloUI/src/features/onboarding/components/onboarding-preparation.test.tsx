import { fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { DependencyDisclosure } from "@/features/onboarding/components/dependency-disclosure"
import { projectOnboarding } from "@/features/onboarding/model/onboarding-state"
import { GitHubStep } from "@/features/onboarding/steps/github-step"
import { onboardingScenarios } from "@/fixtures/scenarios"

describe("onboarding preparation interactions", () => {
  it("projects only the approved dependency inventory with one bundled runtime result", () => {
    const view = projectOnboarding(onboardingScenarios.complete, "connected")

    expect(view.dependencies.map(({ id }) => id)).toEqual(["system", "bundled-tools"])
    expect(view.dependencies[0].items.map(({ name }) => name)).toEqual([
      "Supported OS", "Virtualization",
    ])
    expect(view.dependencies[1].items.map(({ name }) => name)).toEqual([
      "Computer runtime", "Git", "Git LFS",
    ])
    expect(view.dependencies[1].items[0].check).toMatchObject({
      id: "runtime-microsandbox",
      status: "pass",
      detail: "Bundled msb 0.7.6 · libkrunfw 5.6.1",
    })
  })

  it("treats an absent required check as unsuccessful", () => {
    const source = {
      ...onboardingScenarios.complete,
      preflightChecks: onboardingScenarios.complete.preflightChecks.filter(({ id }) => id !== "runtime-microsandbox"),
    }
    const view = projectOnboarding(source, "connected")

    expect(view.dependencies[1].status).toBe("failed")
    expect(view.dependencies[1].items[0].check).toMatchObject({ status: "unavailable", detail: "No check result was reported." })
    expect(view.dependencyStatus).toBe("failed")
    expect(view.finishEnabled).toBe(false)
  })

  it("summarizes closed dependency groups and supports keyboard disclosure", async () => {
    const user = userEvent.setup()
    const group = projectOnboarding(onboardingScenarios.running, "connected").dependencies[0]
    render(<DependencyDisclosure group={group} />)

    expect(screen.getByText("2 of 2 checks passed")).toBeVisible()
    expect(screen.queryByText("Supported OS")).not.toBeInTheDocument()
    const trigger = screen.getByRole("button", { name: "System" })
    trigger.focus()
    await user.keyboard(" ")
    expect(trigger).toHaveAttribute("aria-expanded", "true")
    expect(screen.getByText("Supported OS")).toBeVisible()
    expect(screen.queryByText("Supported operating system and build architecture")).not.toBeInTheDocument()
    await user.keyboard(" ")
    expect(trigger).toHaveAttribute("aria-expanded", "false")
    expect(screen.queryByText("Supported OS")).not.toBeInTheDocument()
  })

  it("opens a failed bundled runtime check without offering onboarding repair", () => {
    const group = projectOnboarding(onboardingScenarios["dependency-failure"], "connected").dependencies[1]
    render(<DependencyDisclosure group={group} />)

    expect(screen.getByText("Checks unavailable")).toBeVisible()
    const trigger = screen.getByRole("button", { name: "Bundled tools" })
    expect(trigger).toHaveAttribute("aria-expanded", "true")
    const notice = screen.getByRole("alert")
    expect(screen.getByText("Check unavailable")).toHaveClass("truncate", "whitespace-nowrap")
    expect(screen.getAllByText("The bundled MicroSandbox runtime failed its integrity check.")).toHaveLength(1)
    expect(within(notice).getByText("Bundled MicroSandbox runtime")).toBeVisible()
    expect(within(notice).getByText("Reinstall this Silo build from a trusted package.")).toBeVisible()
    expect(within(notice).queryByRole("button", { name: /Repair/ })).not.toBeInTheDocument()
  })

  it("asks for confirmation before clearing onboarding repository access", async () => {
    const changeSelections = vi.fn()
    render(<GitHubStep
      computers={[{ name: "dev" }]}
      connectionState="connected"
      repositoryOptions={["acme/silo"]}
      computerSelections={{ dev: [{ repository: "acme/silo", allowPushes: false }] }}
      computerIdentities={{ dev: { name: "Taylor", email: "taylor@example.com", apply: true } }}
      currentDeviceGitIdentity={{ name: "Taylor", email: "taylor@example.com" }}
      onConnect={vi.fn()}
      onComputerSelectionsChange={changeSelections}
      onComputerIdentityChange={vi.fn()}
      onResetComputerIdentity={vi.fn()}
    />)

    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Clear repositories from dev" }))
    expect(changeSelections).not.toHaveBeenCalled()
    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByText("Remove all repositories from dev?")).not.toBeInTheDocument())
    expect(changeSelections).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Clear repositories from dev" }))
    expect(screen.getByText("Remove all repositories from dev?")).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Remove all" }))
    await waitFor(() => expect(changeSelections).toHaveBeenCalledExactlyOnceWith("dev", []))
  })
})
