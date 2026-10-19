/** The shared look of a sidebar row at either level: one hover tint, one focus ring. */
export const sidebarItemClass = "group/sidebar-item relative flex w-full min-w-0 items-center gap-2 rounded-md text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-ring"

export const sidebarItemLevels = {
  primary: "sidebar-primary h-10 flex-none py-2 text-ui",
  secondary: "sidebar-secondary h-8 text-xs",
} as const

export const sidebarItemActiveClass = "bg-sidebar-accent font-medium text-sidebar-accent-foreground"
