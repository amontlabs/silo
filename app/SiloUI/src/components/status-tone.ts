export type StatusTone = "success" | "warning" | "danger" | "neutral"

/** The classes every status presentation draws from, so a tone looks the same in text, dots, chips, tinted rows and notices. */
export const statusTones: Record<StatusTone, { text: string; dot: string; chip: string; row: string; notice: string }> = {
  success: {
    text: "text-success",
    dot: "bg-success",
    chip: "bg-success/10 text-success",
    row: "bg-success/5 hover:bg-success/10 focus-within:bg-success/10",
    notice: "border-success/30 bg-success/10 text-success",
  },
  warning: {
    text: "text-warning",
    dot: "bg-warning",
    chip: "bg-warning/10 text-warning",
    row: "bg-warning/5 hover:bg-warning/10 focus-within:bg-warning/10",
    notice: "border-warning/30 bg-warning/10 text-warning",
  },
  danger: {
    text: "text-destructive",
    dot: "bg-destructive",
    chip: "bg-destructive/10 text-destructive",
    row: "bg-destructive/5 hover:bg-destructive/10 focus-within:bg-destructive/10",
    notice: "border-destructive/30 bg-destructive/10 text-destructive",
  },
  neutral: {
    text: "text-muted-foreground",
    dot: "bg-muted-foreground/55",
    chip: "bg-muted text-muted-foreground",
    row: "row-hover",
    notice: "border-border bg-muted text-muted-foreground",
  },
}

export type ProgressStatus = "waiting" | "running" | "succeeded" | "failed"

export const progressStatuses: Record<ProgressStatus, { tone: StatusTone; label: string }> = {
  waiting: { tone: "neutral", label: "Waiting" },
  running: { tone: "warning", label: "In progress" },
  succeeded: { tone: "success", label: "Complete" },
  failed: { tone: "danger", label: "Failed" },
}
