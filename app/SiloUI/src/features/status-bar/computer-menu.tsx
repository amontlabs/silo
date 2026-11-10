import type { ReactNode } from "react"
import { Bot, ChevronRight, Code, ExternalLink, Globe, MoreHorizontal, Play, RotateCw, Square, Terminal } from "lucide-react"
import { DropdownMenu } from "radix-ui"

import { CopyButton } from "@/components/copy-button"
import { Button } from "@/components/ui/button"
import { computerTarget } from "@/features/application/model/connections"
import { cn } from "@/lib/utils"
import { openableAgents } from "@/features/agents/model/agents"
import type { ComputerMenuProps } from "./status-bar-types"
import { computerMenuItems, type ComputerMenuItem } from "./computer-menu-items"

const menuClass = "silo-window z-50 min-w-48 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95"
const menuItemClass = "flex min-h-8 select-none items-center gap-2 rounded-sm px-2 text-xs outline-none data-[highlighted]:bg-accent data-[disabled]:pointer-events-none data-[disabled]:opacity-40 [&_svg]:size-3.5 [&_svg]:shrink-0 [&_svg]:text-muted-foreground"

const icons: Record<string, ReactNode> = {
  start: <Play />, stop: <Square />, restart: <RotateCw />, terminal: <Terminal />, editor: <Code />, sites: <Globe />,
}

function MenuItems({ items, reduceMotion }: { items: ComputerMenuItem[]; reduceMotion: boolean }) {
  return items.map((item, index) => {
    if (item.kind === "separator") return <DropdownMenu.Separator key={`separator:${index}`} className="my-1 border-t" />
    if (item.kind === "copy") {
      return <DropdownMenu.Item key={item.id} asChild onSelect={(event) => event.preventDefault()}>
        <CopyButton
          value={item.value}
          labels={{ idle: item.label, copied: item.copied, failed: item.failed }}
          text={{ idle: item.label, copied: "Copied", failed: "Copy failed" }}
          variant="ghost"
          size="sm"
          className={cn(menuItemClass, "w-full justify-start font-normal")}
        />
      </DropdownMenu.Item>
    }
    if (item.kind === "submenu") {
      return <DropdownMenu.Sub key={item.id}>
        <DropdownMenu.SubTrigger className={menuItemClass} disabled={!item.enabled}>{icons[item.id]} {item.label} <ChevronRight className="ml-auto" /></DropdownMenu.SubTrigger>
        <DropdownMenu.Portal>
          <DropdownMenu.SubContent className={menuClass} data-reduce-motion={reduceMotion} sideOffset={4} collisionPadding={10}>
            <MenuItems items={item.items} reduceMotion={reduceMotion} />
          </DropdownMenu.SubContent>
        </DropdownMenu.Portal>
      </DropdownMenu.Sub>
    }
    return <DropdownMenu.Item key={item.id} className={menuItemClass} disabled={!item.enabled} onSelect={item.run}>
      {icons[item.id] ?? (item.id.startsWith("site:") ? <ExternalLink /> : item.id.startsWith("agent:") ? <Bot /> : null)}{item.label}
    </DropdownMenu.Item>
  })
}

/**
 * The computer "…" menu rendered with Radix for the browser preview and tests. The desktop
 * app renders the same `computerMenuItems` natively (`desktop/native-computer-menu.tsx`).
 */
export function ComputerMenu({ computer, source, actions, onFolders, onConfirm, agents }: ComputerMenuProps) {
  const target = computerTarget(computer)
  const items = computerMenuItems(computer, source, {
    start: () => actions.startComputer(target),
    confirm: onConfirm,
    openTerminal: () => actions.openTerminal(target),
    agents: openableAgents(agents),
    openAgent: actions.openAgent && ((agent) => actions.openAgent?.(target, agent)),
    chooseFolder: onFolders,
    openSite: (port) => actions.openSite(target, port),
  })
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>
        <Button variant="ghost" size="icon-xs" aria-label={`Actions for ${computer.configuration.name}`}><MoreHorizontal /></Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className={menuClass} data-reduce-motion={source.preferences.reduceMotion} align="end" sideOffset={4} collisionPadding={10} aria-label={`Actions for ${computer.configuration.name}`}>
          <MenuItems items={items} reduceMotion={source.preferences.reduceMotion} />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}
