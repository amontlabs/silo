import { clsx, type ClassValue } from "clsx"
import { extendTailwindMerge } from "tailwind-merge"

// The theme's own type sizes (index.css) must merge as font sizes, not as text colors.
const twMerge = extendTailwindMerge({ extend: { classGroups: { "font-size": [{ text: ["caption", "ui"] }] } } })

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
