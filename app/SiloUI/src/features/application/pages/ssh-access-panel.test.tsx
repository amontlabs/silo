import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { setupFakeTimerUser } from "@/test/fake-timer-user"
import { toast } from "sonner"
import { Toaster } from "@/components/ui/sonner"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { SshAccessBadges, SshAccessPanel, SshAccessRow } from "./ssh-access-panel"
import type { ApplicationActions, ApplicationComputer, SshAccessComputer } from "../model/application-source"
const computer = applicationSourceForScenario("complete").computers[0]
const publicKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOV89nMlTnLLFa2UlVuqssPU56E2EbdIg1XmcraGpVXQ laptop"
const base: SshAccessComputer = { computer: "dev", enabled: true, port: 2222, bindAddress: "127.0.0.1", keys: [publicKey], state: "listening", message: null, fingerprint: "SHA256:example", deviceName: "Ada’s Mac mini", addresses: ["127.0.0.1", "192.168.1.42"] }
function setup(patch: Partial<SshAccessComputer> = {}, save = vi.fn().mockResolvedValue(undefined), error?: string, displayedComputer: ApplicationComputer = computer) {
  const access = { ...base, ...patch }
  const actions = { sshConnection: vi.fn().mockImplementation((_computer: string, download: boolean) => Promise.resolve(download ? null : "ssh -i '/managed/client_key' -p 2222 root@127.0.0.1")), saveSshAccess: save, refreshSshAccess: vi.fn().mockResolvedValue(undefined) } as unknown as ApplicationActions
  const view = render(<><Toaster /><SshAccessPanel computers={[displayedComputer]} state={{ computers: [access] }} error={error} actions={actions} active /></>)
  return { user: vi.isFakeTimers() ? setupFakeTimerUser() : userEvent.setup(), access, save, actions, ...view }
}
afterEach(() => { toast.dismiss(); vi.useRealTimers() })
async function expand(user: ReturnType<typeof userEvent.setup>) { await user.click(screen.getByRole("button", { name: "SSH access controls for dev" })) }
async function selectAction(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(screen.getByRole("button", { name: name.includes("network") ? "More network SSH actions" : "More local SSH actions" }))
  await user.click(screen.getByRole("menuitem", { name }))
}
describe("managed SSH access", () => {
  it("clears command copy feedback after a short delay", async () => {
    vi.useFakeTimers()
    const { user } = setup()
    await expand(user)
    await selectAction(user, "Copy local SSH command")
    await user.click(screen.getByRole("button", { name: "More local SSH actions" }))
    expect(screen.getByRole("menuitem", { name: "Copy local SSH command" })).toHaveTextContent("Command copied")
    await act(async () => { await vi.advanceTimersByTimeAsync(1_199) })
    expect(screen.getByRole("menuitem", { name: "Copy local SSH command" })).toHaveTextContent("Command copied")
    await act(async () => { await vi.advanceTimersByTimeAsync(1) })
    expect(screen.getByRole("menuitem", { name: "Copy local SSH command" })).toHaveTextContent("Copy terminal command")
  })
  it("shows two toggles and a ready connection without key setup or advanced settings", async () => {
    const { user, actions } = setup({ keys: [] })
    expect(screen.getByText("SSH listening")).toBeVisible()
    await expand(user)
    expect(screen.getAllByRole("switch")).toHaveLength(2)
    expect(screen.getByText("ssh -p 2222 silo@127.0.0.1")).toBeVisible()
    expect(screen.getByRole("switch", { name: "Allow SSH from Ada’s Mac mini" })).toBeChecked()
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument()
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument()
    expect(screen.queryByText("Advanced")).not.toBeInTheDocument()
    expect(screen.queryByText(/Keys \(/)).not.toBeInTheDocument()
    await selectAction(user, "Copy local SSH command")
    expect(actions.sshConnection).toHaveBeenCalledWith("dev", false, false)
    expect(await navigator.clipboard.readText()).toBe("ssh -i '/managed/client_key' -p 2222 root@127.0.0.1")
    expect(screen.queryByRole("menu")).not.toBeInTheDocument()
    await expand(user)
    expect(screen.getByText("SSH listening")).toBeVisible()
  })
  it("only changes access when the switch itself is clicked", async () => {
    const { user, save } = setup()
    await expand(user)
    await user.click(screen.getByText("Allow SSH from Ada’s Mac mini"))
    await user.click(screen.getByText("Allow SSH from other devices"))
    expect(save).not.toHaveBeenCalled()
    await user.click(screen.getByRole("switch", { name: "Allow SSH from Ada’s Mac mini" }))
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ enabled: false }))
  })
  it("downloads the generated key through the native save action", async () => {
    const { user, actions } = setup()
    await expand(user)
    await selectAction(user, "Save local SSH key file")
    expect(actions.sshConnection).toHaveBeenCalledWith("dev", true, false)
    expect(await navigator.clipboard.readText()).toBe("")
  })
  it.each([
    ["Copy SSH address", "Copy address"],
    ["More local SSH actions", "More local SSH actions"],
  ])("uses the app tooltip for %s without native titles", async (name, caption) => {
    const { user } = setup()
    await expand(user)
    await user.hover(screen.getByRole("button", { name }))
    expect(await screen.findByRole("tooltip")).toHaveTextContent(caption)
    expect(screen.getByRole("button", { name }).closest("[title]")).toBeNull()
  })
  it("copies the displayed network address and keeps the fingerprint in its tooltip", async () => {
    const { user } = setup({ bindAddress: "192.168.1.42" })
    await expand(user)
    await user.click(screen.getByRole("button", { name: "Copy network SSH address" }))
    expect(await navigator.clipboard.readText()).toBe("ssh -p 2222 silo@192.168.1.42")
    await user.hover(screen.getByText("ssh -p 2222 silo@192.168.1.42"))
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Host key: SHA256:example")
  })
  it("groups each address with its toggle and provides network editing and endpoint-specific commands", async () => {
    const { user, actions } = setup({ bindAddress: "192.168.1.42" })
    await expand(user)
    const local = within(screen.getByRole("group", { name: "Allow SSH from Ada’s Mac mini" }))
    const network = within(screen.getByRole("group", { name: "Allow SSH from other devices" }))
    expect(local.getByText("ssh -p 2222 silo@127.0.0.1")).toBeVisible()
    expect(network.getByText("ssh -p 2222 silo@192.168.1.42")).toBeVisible()
    await selectAction(user, "Copy local SSH command")
    expect(actions.sshConnection).toHaveBeenLastCalledWith("dev", false, false)
    await selectAction(user, "Copy network SSH command")
    expect(actions.sshConnection).toHaveBeenLastCalledWith("dev", false, true)
    await selectAction(user, "Edit network connection")
    expect(screen.getByRole("combobox", { name: "LAN or VPN address" })).toHaveValue("192.168.1.42")
  })
  it("enables a stopped computer without key setup or a start action", async () => {
    const { user, save } = setup({ enabled: false, state: "disabled", keys: [] })
    await expand(user)
    expect(screen.getByRole("switch", { name: "Allow SSH from other devices" })).toBeDisabled()
    expect(screen.queryByRole("button", { name: "Copy local SSH command" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("switch", { name: "Allow SSH from Ada’s Mac mini" }))
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ enabled: true }))
  })
  it("omits the redundant waiting caption", async () => {
    const { user } = setup({ state: "waiting" })
    await expand(user)
    expect(screen.queryByRole("status")).not.toBeInTheDocument()
  })
  it("warns before exposing SSH to other devices and saves the single address once confirmed", async () => {
    const { user, save } = setup()
    await expand(user)
    await user.click(screen.getByRole("switch", { name: "Allow SSH from other devices" }))
    expect(save).not.toHaveBeenCalled()
    const dialog = screen.getByRole("dialog")
    expect(dialog).toHaveTextContent("Allow SSH from other devices?")
    expect(dialog).toHaveTextContent("192.168.1.42")
    expect(dialog).toHaveTextContent("port 2222")
    await user.click(within(dialog).getByRole("button", { name: "Allow" }))
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ bindAddress: "192.168.1.42" })))
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument()
  })
  it("keeps SSH local when the network warning is cancelled", async () => {
    const { user, save } = setup()
    await expand(user)
    await user.click(screen.getByRole("switch", { name: "Allow SSH from other devices" }))
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }))
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    expect(screen.getByRole("switch", { name: "Allow SSH from other devices" })).not.toBeChecked()
    expect(save).not.toHaveBeenCalled()
  })
  it("warns before re-enabling SSH that was allowed from other devices", async () => {
    const { user, save } = setup({ enabled: false, state: "disabled", bindAddress: "192.168.1.42" })
    await expand(user)
    await user.click(screen.getByRole("switch", { name: "Allow SSH from Ada’s Mac mini" }))
    expect(save).not.toHaveBeenCalled()
    const dialog = screen.getByRole("dialog")
    expect(dialog).toHaveTextContent("Allow SSH from other devices too?")
    await user.click(within(dialog).getByRole("button", { name: "Allow" }))
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ enabled: true, bindAddress: "192.168.1.42" })))
  })
  it("can limit SSH to this device while it is off", async () => {
    const { user, save } = setup({ enabled: false, state: "disabled", bindAddress: "192.168.1.42" })
    await expand(user)
    const network = screen.getByRole("switch", { name: "Allow SSH from other devices" })
    expect(network).toBeEnabled()
    await user.click(network)
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ enabled: false, bindAddress: "127.0.0.1" }))
  })
  it("asks for an address when there are multiple interfaces and can cancel", async () => {
    const { user, save } = setup({ addresses: [...base.addresses, "10.77.77.2"] })
    await expand(user)
    await user.click(screen.getByRole("switch", { name: "Allow SSH from other devices" }))
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Allow" }))
    expect(await screen.findByRole("combobox", { name: "LAN or VPN address" })).toHaveValue("192.168.1.42")
    expect(save).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument()
    expect(screen.getByRole("switch", { name: "Allow SSH from other devices" })).not.toBeChecked()
  })
  it("rejects wildcard addresses and saves a selected interface", async () => {
    const { user, save } = setup({ addresses: [...base.addresses, "10.77.77.2"] })
    await expand(user)
    await user.click(screen.getByRole("switch", { name: "Allow SSH from other devices" }))
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Allow" }))
    const address = await screen.findByRole("combobox", { name: "LAN or VPN address" })
    await user.clear(address); await user.type(address, "0.0.0.0")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).not.toHaveBeenCalled()
    expect(screen.getByRole("alert")).toHaveTextContent("specific LAN or VPN IPv4")
    expect(address).toHaveAccessibleDescription(/IPv4 address on .+\./)
    await user.clear(address); await user.type(address, "10.77.77.2")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ bindAddress: "10.77.77.2" }))
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument()
  })
  it("disables network access while retaining keys and local access", async () => {
    const { user, save } = setup({ bindAddress: "192.168.1.42" })
    await expand(user)
    await user.click(screen.getByRole("switch", { name: "Allow SSH from other devices" }))
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ enabled: true, bindAddress: "127.0.0.1" }))
  })
  it("edits and validates the port beside the connection", async () => {
    const { user, save } = setup()
    await expand(user)
    await selectAction(user, "Edit connection")
    const port = screen.getByRole("spinbutton", { name: "SSH port" })
    await user.clear(port); await user.type(port, "65536")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).not.toHaveBeenCalled()
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a port from 1 to 65535")
    expect(port).toBeInvalid()
    expect(port).toHaveAccessibleDescription("Enter a port from 1 to 65535.")
    await user.clear(port); await user.type(port, "2223")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ port: 2223 }))
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument()
  })
  it("retains a newly registered controller key when editing from an older snapshot", async () => {
    const controllerKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEB silo-controller:00000000-0000-4000-8000-000000000002"
    let ownerKeys = [publicKey, controllerKey]
    const save = vi.fn().mockImplementation(async (request: { keys?: string[] }) => {
      if (request.keys !== undefined) ownerKeys = request.keys
    })
    const { user } = setup({}, save)
    await expand(user)
    await selectAction(user, "Edit connection")
    const port = screen.getByRole("spinbutton", { name: "SSH port" })
    await user.clear(port); await user.type(port, "2223")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(save).toHaveBeenCalledOnce()
    expect(ownerKeys).toContain(controllerKey)
  })
  it("preserves a failed port edit and displays its error", async () => {
    const { user } = setup({}, vi.fn().mockRejectedValue(new Error("Port is in use.")))
    await expand(user)
    await selectAction(user, "Edit connection")
    await user.click(screen.getByRole("button", { name: "Save" }))
    expect(await screen.findByText("Port is in use.")).toBeInTheDocument()
    expect(screen.getByText("SSH settings not saved")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument()
    expect(screen.getByRole("spinbutton")).toBeVisible()
  })
  it("reports connection preparation failures without copying an unusable command", async () => {
    const { user, actions } = setup()
    vi.mocked(actions.sshConnection!).mockRejectedValue(new Error("Enable access from other devices first."))
    await expand(user)
    await selectAction(user, "Copy local SSH command")
    expect(await screen.findByText("Enable access from other devices first.")).toBeInTheDocument()
    expect(await navigator.clipboard.readText()).toBe("")
  })
  it("prevents concurrent changes while preparing the connection", async () => {
    const { user, actions } = setup()
    let resolve: (value: string | null) => void = () => {}
    vi.mocked(actions.sshConnection!).mockImplementation(() => new Promise(done => { resolve = done }))
    await expand(user)
    await selectAction(user, "Copy local SSH command")
    await user.click(screen.getByRole("button", { name: "More local SSH actions" }))
    expect(screen.getByRole("menuitem", { name: "Save local SSH key file" })).toHaveAttribute("data-disabled")
    resolve(null)
    expect(await screen.findByRole("menuitem", { name: "Copy local SSH command" })).toBeVisible()
  })
  it("reports a clipboard failure instead of claiming a copied connection", async () => {
    const { user } = setup()
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValueOnce(new Error("Clipboard unavailable."))
    await expand(user)
    await selectAction(user, "Copy local SSH command")
    expect(await screen.findByText("Clipboard unavailable.")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "SSH host command copied" })).not.toBeInTheDocument()
  })
  it("routes connection preparation and changes to the remote owner", async () => {
    const target = "silo-remote:office:vm-immutable-id"
    const remote = { ...computer, device: { id: "office", computerId: "vm-immutable-id", name: "Office Mac", address: "user@office", connected: true } }
    const { user, actions, save } = setup({ computer: target, deviceName: "Office Mac", bindAddress: "192.168.1.42" }, undefined, undefined, remote)
    await expand(user)
    expect(screen.getByRole("switch", { name: "Allow SSH from Office Mac" })).toBeChecked()
    await selectAction(user, "Copy network SSH command")
    expect(actions.sshConnection).toHaveBeenCalledWith(target, false, true)
    await user.click(screen.getByRole("switch", { name: "Allow SSH from Office Mac" }))
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ computer: target, enabled: false }))
  })
  it("hides the owner's loopback address on remote rows and uses the reported account", async () => {
    const remote = { ...computer, device: { id: "office", computerId: "vm-immutable-id", name: "Office Mac", address: "user@office", connected: true } }
    const { user } = setup({ computer: "silo-remote:office:vm-immutable-id", deviceName: "Office Mac", bindAddress: "192.168.1.42", user: "guest" }, undefined, undefined, remote)
    await expand(user)
    const local = within(screen.getByRole("group", { name: "Allow SSH from Office Mac" }))
    expect(local.getByText("Only on Office Mac")).toBeVisible()
    expect(screen.queryByText(/127\.0\.0\.1/)).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Copy SSH address" })).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Copy network SSH address" }))
    expect(await navigator.clipboard.readText()).toBe("ssh -p 2222 guest@192.168.1.42")
  })
  it("disables stale remote connections and changes", async () => {
    const remote = { ...computer, device: { id: "office", computerId: "vm-immutable-id", name: "Office Mac", address: "user@office", connected: true } }
    const { user } = setup({ computer: "silo-remote:office:vm-immutable-id", deviceName: "Office Mac", unavailable: "SSH status on Office Mac is unavailable." }, undefined, undefined, remote)
    await expand(user)
    expect(screen.getByText("SSH status unavailable")).toBeVisible()
    expect(screen.getByRole("switch", { name: "Allow SSH from Office Mac" })).toBeDisabled()
    await user.click(screen.getByRole("button", { name: "More local SSH actions" }))
    expect(screen.getByRole("menuitem", { name: "Copy local SSH command" })).toHaveAttribute("data-disabled")
    expect(screen.getByRole("menuitem", { name: "Save local SSH key file" })).toHaveAttribute("data-disabled")
    expect(screen.getByRole("menuitem", { name: "Edit connection" })).toHaveAttribute("data-disabled")
  })
  it("keeps healthy remote controls writable when local status fails", async () => {
    const remote = { ...computer, device: { id: "office", computerId: "vm-immutable-id", name: "Office Mac", address: "user@office", connected: true } }
    const { user } = setup({ computer: "silo-remote:office:vm-immutable-id", deviceName: "Office Mac" }, undefined, "Could not check SSH access.", remote)
    await expand(user)
    expect(screen.getByRole("switch", { name: "Allow SSH from Office Mac" })).toBeEnabled()
  })
  it("does not present stale local state as listening or allow changes", async () => {
    const { user } = setup({}, undefined, "Could not check SSH access.")
    expect(screen.getByText("SSH status unavailable")).toBeVisible()
    expect(screen.queryByText("SSH listening")).not.toBeInTheDocument()
    await expand(user)
    expect(screen.getByRole("switch", { name: "Allow SSH from Ada’s Mac mini" })).toBeDisabled()
  })
})

describe("SSH badge", () => {
  const badge = () => screen.getByLabelText(/^SSH from Ada’s Mac mini/)
  it("keeps a listening badge plain", () => {
    render(<SshAccessBadges access={base} />)
    expect(badge()).toHaveAccessibleName("SSH from Ada’s Mac mini only")
    expect(badge()).toHaveTextContent(/^SSH$/)
    expect(badge().querySelector(".lucide-triangle-alert")).toBeNull()
  })
  it("shows an SSH error on the badge itself, not only in its tooltip", () => {
    render(<SshAccessBadges access={{ ...base, bindAddress: "192.168.1.42", state: "error", message: "SSH could not listen." }} />)
    expect(badge()).toHaveTextContent("SSH error")
    expect(badge()).toHaveAccessibleName("SSH from Ada’s Mac mini and other devices: SSH could not listen.")
    expect(badge().querySelector(".lucide-triangle-alert")).toBeInTheDocument()
    expect(badge().className).toContain("text-destructive")
  })
  it("marks unavailable status with a visible warning", () => {
    render(<SshAccessBadges access={base} stale />)
    expect(badge()).toHaveAccessibleName("SSH from Ada’s Mac mini only: Status unavailable")
    expect(badge().querySelector(".lucide-triangle-alert")).toBeInTheDocument()
    expect(badge().className).toContain("text-warning")
  })
  it("names a waiting listener without alarming", () => {
    render(<SshAccessBadges access={{ ...base, state: "waiting" }} />)
    expect(badge()).toHaveAccessibleName("SSH from Ada’s Mac mini only: Waiting for computer")
    expect(badge().querySelector(".lucide-triangle-alert")).toBeNull()
  })
})


it.each(["save", "connection"])("blocks an old SSH Retry while another %s is pending", async operation => {
  let finish!: () => void
  const pending = new Promise<void>(resolve => { finish = resolve })
  const save = vi.fn().mockRejectedValueOnce(new Error("SSH save failed."))
    .mockImplementationOnce(() => pending).mockResolvedValue(undefined)
  const { user, actions } = setup({}, save)
  await expand(user)
  const toggle = screen.getByRole("switch", { name: "Allow SSH from Ada’s Mac mini" })
  await user.click(toggle)
  const retry = await screen.findByRole("button", { name: "Retry" })
  if (operation === "save") await user.click(toggle)
  else {
    vi.mocked(actions.sshConnection!).mockImplementationOnce(() => pending.then(() => null))
    await selectAction(user, "Copy local SSH command")
  }
  const saves = operation === "save" ? 2 : 1
  expect(toggle).toBeDisabled()
  await user.click(retry)
  expect(save).toHaveBeenCalledTimes(saves)
  expect(toggle).toBeDisabled()
  await act(async () => finish())
  expect(toggle).toBeEnabled()
})


it("blocks an old SSH Retry for connection preparation while a save is pending", async () => {
  let finish!: () => void
  const save = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
  const { user, actions } = setup({}, save)
  vi.mocked(actions.sshConnection!).mockRejectedValueOnce(new Error("SSH preparation failed."))
  await expand(user)
  await selectAction(user, "Copy local SSH command")
  const retry = await screen.findByRole("button", { name: "Retry" })
  const toggle = screen.getByRole("switch", { name: "Allow SSH from Ada’s Mac mini" })
  await user.click(toggle)
  expect(toggle).toBeDisabled()
  await user.click(retry)
  expect(actions.sshConnection).toHaveBeenCalledOnce()
  expect(toggle).toBeDisabled()
  await act(async () => finish())
  expect(toggle).toBeEnabled()
})


it("ignores an old SSH Retry after a newer save completes", async () => {
  const save = vi.fn().mockRejectedValueOnce(new Error("Port is in use.")).mockResolvedValue(undefined)
  const { user } = setup({}, save)
  await expand(user)
  await selectAction(user, "Edit connection")
  await user.click(screen.getByRole("button", { name: "Save" }))
  const retry = await screen.findByRole("button", { name: "Retry" })
  const port = screen.getByRole("spinbutton", { name: "SSH port" })
  await user.clear(port); await user.type(port, "2223")
  await user.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument())
  await user.click(retry)
  expect(save).toHaveBeenCalledTimes(2)
  expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ port: 2223 }))
})

it("ignores an SSH Retry after its computer controls unmount", async () => {
  const save = vi.fn().mockRejectedValue(new Error("Port is in use."))
  const { user, rerender } = setup({}, save)
  await expand(user)
  await selectAction(user, "Edit connection")
  await user.click(screen.getByRole("button", { name: "Save" }))
  const retry = await screen.findByRole("button", { name: "Retry" })
  rerender(<Toaster />)
  await user.click(retry)
  expect(save).toHaveBeenCalledTimes(1)
})

it("checks current read-only state before retrying an SSH save", async () => {
  const save = vi.fn().mockRejectedValue(new Error("Port is in use."))
  const user = userEvent.setup()
  const { rerender } = render(<><Toaster /><SshAccessRow computer={computer} access={base} save={save} stale={false} embedded /></>)
  await user.click(screen.getByRole("switch", { name: "Allow SSH from Ada’s Mac mini" }))
  const retry = await screen.findByRole("button", { name: "Retry" })
  rerender(<><Toaster /><SshAccessRow computer={computer} access={base} save={save} stale={false} embedded readOnly /></>)
  await user.click(retry)
  expect(save).toHaveBeenCalledTimes(1)
})
