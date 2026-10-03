import { Code, Download, Upload } from "lucide-react"
import { CopyButton } from "@/components/copy-button"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"

const hoverActions = "flex shrink-0 items-center gap-0.5 opacity-0 group-hover/folder:opacity-100 group-focus-within/folder:opacity-100 [@media(hover:none)]:opacity-100"

/** Download for a file row; it appears with the same hover and focus behavior as a folder's actions. */
export function FileActions({ name, onDownload, disabled = false }: { name: string; onDownload: () => void; disabled?: boolean }) {
  return <div className={hoverActions}>
    <TooltipProvider delayDuration={150}>
      <Tooltip>
        <TooltipTrigger asChild><Button variant="ghost" size="icon-xs" aria-label={`Download ${name}`} disabled={disabled} onClick={onDownload}><Download /></Button></TooltipTrigger>
        <TooltipContent>Download</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  </div>
}

export function FolderActions({ editor, path, onOpen, onUpload, disabled = false, uploadDisabled = disabled }: { editor: string; path: string; onOpen: () => void; onUpload?: () => void; disabled?: boolean; uploadDisabled?: boolean }) {
  return <div className={hoverActions}>
    <TooltipProvider delayDuration={150}>
      <Tooltip>
        <TooltipTrigger asChild><Button variant="ghost" size="icon-xs" aria-label={`Open in ${editor}`} disabled={disabled} onClick={onOpen}><Code /></Button></TooltipTrigger>
        <TooltipContent>Open in {editor}</TooltipContent>
      </Tooltip>
      {onUpload && <Tooltip>
        <TooltipTrigger asChild><Button variant="ghost" size="icon-xs" aria-label="Upload files here" disabled={uploadDisabled} onClick={onUpload}><Upload /></Button></TooltipTrigger>
        <TooltipContent>Upload files here</TooltipContent>
      </Tooltip>}
      <Tooltip>
        <TooltipTrigger asChild><CopyButton variant="ghost" size="icon-xs" value={path} labels={{ idle: "Copy path", copied: "Path copied", failed: "Could not copy path" }} /></TooltipTrigger>
        <TooltipContent>Copy path</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  </div>
}
