export type ClipboardAction = "paste" | "copy"
export type ClipboardStatus = "pasted" | "copied" | "device-empty" | "computer-empty" | "too-large" | "not-connected" | "unsupported" | "busy" | "failed"
export type ClipboardReport = {
  action: ClipboardAction
  status: ClipboardStatus
  content?: "text" | "image" | null
  message?: string | null
}

export const CLIPBOARD_EVENT = "desktop-clipboard"
export const CLIPBOARD_UPDATE_MESSAGE = "Update the desktop to use the clipboard"

/** Words for the toolbar; `name` is the computer's display name. */
export function clipboardFeedback(report: ClipboardReport, name: string): { text: string; error: boolean } {
  const noun = report.content === "image" ? "image" : "text"
  switch (report.status) {
    case "pasted": return { text: `Pasted ${report.content === "image" ? "image " : ""}into ${name}`, error: false }
    case "copied": return { text: `Copied ${report.content === "image" ? "image " : ""}from ${name}`, error: false }
    case "device-empty": return { text: "This device's clipboard has no text or image", error: true }
    case "computer-empty": return { text: `Nothing to copy from ${name}`, error: true }
    case "too-large": return { text: report.content ? `That ${noun} is too large to ${report.action === "paste" ? "paste" : "copy"}` : `${name}'s clipboard is too large to copy`, error: true }
    case "not-connected": return { text: "The desktop is not connected", error: true }
    case "unsupported": return { text: CLIPBOARD_UPDATE_MESSAGE, error: true }
    case "busy": return { text: "A clipboard transfer is already running", error: true }
    default: return { text: report.message || "The clipboard transfer failed", error: true }
  }
}
