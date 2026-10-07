import { TerminalSquare } from "lucide-react"
import { useState } from "react"

import { CopyButton } from "@/components/copy-button"
import { DisclosureHeader } from "@/components/disclosure-header"
import { Collapsible, CollapsibleContent } from "@/components/ui/collapsible"
import { cn } from "@/lib/utils"

interface LogDisclosureProps {
  title: string
  output: string
  outputLabel?: string
  controlsLabel?: string
  emptyMessage?: string
  embedded?: boolean
  labels?: { expand: string; collapse: string; copy: string; copied: string; copyFailed: string }
}

export function LogDisclosure({ title, output, outputLabel, controlsLabel, emptyMessage = "No output yet.", embedded = false, labels }: LogDisclosureProps) {
  const [open, setOpen] = useState(false)
  const name = title.toLowerCase()

  return <Collapsible open={open} onOpenChange={setOpen} className={cn("collapsible-motion", embedded ? "border-t border-border" : "rounded-md border border-border")}>
    <DisclosureHeader
      title={title}
      label={open ? labels?.collapse ?? `Hide ${name}` : labels?.expand ?? `Show ${name}`}
      controlsLabel={controlsLabel}
      icon={<TerminalSquare className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />}
      className="py-1.5"
      titleClassName="text-caption text-muted-foreground"
      actions={<CopyButton
        variant="ghost"
        size="icon-xs"
        value={output}
        disabled={!output}
        labels={{ idle: labels?.copy ?? `Copy ${name}`, copied: labels?.copied ?? `${title} copied`, failed: labels?.copyFailed ?? `Copy ${name} failed` }}
      />}
    />
    <CollapsibleContent className="collapsible-content-motion">
      <pre role="region" tabIndex={0} className="max-h-32 overflow-auto whitespace-pre-wrap break-words border-t border-border bg-zinc-950 px-3 py-2.5 font-mono text-caption leading-5 text-zinc-200 select-text focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring dark:bg-black" aria-label={outputLabel ?? title}>{output || emptyMessage}</pre>
    </CollapsibleContent>
  </Collapsible>
}
