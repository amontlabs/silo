import { describe, expect, it } from "vitest"

import { cn } from "@/lib/utils"

describe("cn", () => {
  it("keeps the theme's type sizes next to text colors", () => {
    expect(cn("text-caption text-muted-foreground")).toBe("text-caption text-muted-foreground")
    expect(cn("text-ui font-medium text-foreground")).toBe("text-ui font-medium text-foreground")
  })

  it("lets a later type size replace an earlier one", () => {
    expect(cn("text-xs", "text-caption")).toBe("text-caption")
    expect(cn("text-caption", "text-sm")).toBe("text-sm")
  })
})
