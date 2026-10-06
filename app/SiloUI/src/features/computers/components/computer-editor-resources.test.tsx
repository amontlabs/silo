import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { ComputerConfigurationList } from "./computer-configuration-list"

async function openNewComputer() {
  const onConfigurationsChange = vi.fn()
  render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={onConfigurationsChange} /></TooltipProvider>)
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(screen.getByRole("menuitem", { name: "New computer" }))
  return { user, onConfigurationsChange }
}

async function enterCustom(user: ReturnType<typeof userEvent.setup>, label: string, unit: string, value: string) {
  await user.selectOptions(screen.getByRole("combobox", { name: label }), "custom")
  const input = screen.getByRole("spinbutton", { name: `${label} custom (${unit})` })
  await user.clear(input)
  if (value) await user.type(input, value)
  return input
}

describe("configuration editor resource fields", () => {
  it("caps presets at the runtime limit on a device with more CPUs", async () => {
    const onConfigurationsChange = vi.fn()
    render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={onConfigurationsChange}
      getDeviceCapacity={() => ({ logicalCPUs: 512, memoryGiB: 64 })} /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Add" }))
    await user.click(screen.getByRole("menuitem", { name: "New computer" }))
    const ceiling = screen.getByRole("combobox", { name: "Maximum CPUs" })
    const values = [...ceiling.querySelectorAll("option")].map(option => option.value)
    expect(values).not.toContain("512")
    expect(values).toContain("255")
    await user.selectOptions(ceiling, "255")
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(onConfigurationsChange.mock.lastCall?.[0]).toEqual([expect.objectContaining({ maxCPUs: 255 })])
  })

  it("shows the selected ceiling when switching to a device with fewer CPUs", async () => {
    const onCommitComputer = vi.fn().mockResolvedValue(undefined)
    render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={vi.fn()} onCommitComputer={onCommitComputer}
      devices={[{ id: "office", name: "Office", connected: true }]}
      getDeviceCapacity={device => device === "" ? { logicalCPUs: 4, memoryGiB: 16 } : undefined} /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Add" }))
    await user.click(screen.getByRole("menuitem", { name: "New computer" }))
    await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "office")
    await user.selectOptions(screen.getByRole("combobox", { name: "Maximum CPUs" }), "12")
    await user.selectOptions(screen.getByRole("combobox", { name: "Run on" }), "")
    const ceiling = screen.getByRole("spinbutton", { name: "Maximum CPUs custom (CPUs)" })
    expect(ceiling).toHaveDisplayValue("12")
    await user.clear(ceiling)
    await user.type(ceiling, "4")
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(onCommitComputer).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ maxCPUs: 4 }), undefined, "", [])
  })

  it("caps custom CPU counts at what the runtime accepts", async () => {
    const { user } = await openNewComputer()
    const input = await enterCustom(user, "Maximum CPUs", "CPUs", "12")
    expect(input).toHaveAttribute("max", "255")
    expect(input).toHaveAttribute("step", "1")
  })

  it.each([
    ["an empty value", ""],
    ["a fraction", "1.5"],
    ["an exponent", "1e3"],
    ["more than the runtime accepts", "300"],
  ])("rejects %s with a readable message instead of saving", async (_case, value) => {
    const { user, onConfigurationsChange } = await openNewComputer()
    const input = await enterCustom(user, "Maximum CPUs", "CPUs", value)
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(onConfigurationsChange).not.toHaveBeenCalled()
    expect(input).toHaveAccessibleDescription("Enter a whole number of CPUs from 1 to 255.")
    // What the user typed stays visible so they can correct it. (jsdom drops the partial
    // "1e" while typing, which browsers keep as raw text, so skip the exponent case.)
    if (value !== "1e3") expect(input).toHaveDisplayValue(value)
    expect(screen.queryByText(/expected|Too (small|big)/)).not.toBeInTheDocument()
  })

  it("explains memory and storage ranges in the same words", async () => {
    const { user, onConfigurationsChange } = await openNewComputer()
    const memory = await enterCustom(user, "Memory at start", "GiB", "0")
    const storage = await enterCustom(user, "Workspace disk", "GiB", "2.5")
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(onConfigurationsChange).not.toHaveBeenCalled()
    expect(memory).toHaveAccessibleDescription(/^Enter a whole number of GiB from 1 to [\d,]+\.$/)
    expect(storage).toHaveAccessibleDescription("Enter a whole number of GiB from 1 to 4,194,303.")
  })

  it("fits a new computer to a small device so Create succeeds", async () => {
    const onConfigurationsChange = vi.fn()
    render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={onConfigurationsChange} getDeviceCapacity={(device) => device === "" ? { logicalCPUs: 8, memoryGiB: 16 } : undefined} /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Add" }))
    await user.click(screen.getByRole("menuitem", { name: "New computer" }))
    expect(screen.getByRole("combobox", { name: "Maximum CPUs" })).toHaveValue("8")
    expect(screen.getByRole("combobox", { name: "Maximum memory" })).toHaveValue("16")
    const options = (label: string) => [...screen.getByRole("combobox", { name: label }).querySelectorAll("option")].map(option => option.value)
    expect(options("Maximum CPUs")).toEqual(["1", "2", "4", "6", "8", "custom"])
    expect(options("Maximum memory")).toEqual(["1", "2", "4", "8", "12", "16", "custom"])
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(onConfigurationsChange.mock.lastCall?.[0]).toEqual([expect.objectContaining({ cpus: 4, maxCPUs: 8, memoryGiB: 8, maxMemoryGiB: 16 })])
  })

  it("rejects a ceiling above the device before the runtime does", async () => {
    const onConfigurationsChange = vi.fn()
    render(<TooltipProvider><ComputerConfigurationList configurations={[]} onConfigurationsChange={onConfigurationsChange} getDeviceCapacity={() => ({ logicalCPUs: 8, memoryGiB: 16 })} /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Add" }))
    await user.click(screen.getByRole("menuitem", { name: "New computer" }))
    const input = await enterCustom(user, "Maximum CPUs", "CPUs", "12")
    expect(input).toHaveAttribute("max", "8")
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(onConfigurationsChange).not.toHaveBeenCalled()
    expect(input).toHaveAccessibleDescription("This device has 8 CPUs. Choose 8 or fewer.")
  })

  it("saves a valid whole custom value", async () => {
    const { user, onConfigurationsChange } = await openNewComputer()
    await enterCustom(user, "Maximum CPUs", "CPUs", "10")
    await user.click(screen.getByRole("button", { name: "Create" }))
    expect(onConfigurationsChange.mock.lastCall?.[0]).toEqual([expect.objectContaining({ maxCPUs: 10 })])
  })
})
