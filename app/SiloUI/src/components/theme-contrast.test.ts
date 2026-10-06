import { describe, expect, it } from "vitest"
import { createElement } from "react"
import { render, screen } from "@testing-library/react"

import styles from "@/index.css?raw"
import { OperationToastBody } from "@/components/operation-toast-body"
import { buttonVariants } from "@/components/ui/button"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { SecretsPage } from "@/features/application/pages/secrets-page"

// Muted text sits on every neutral surface, including muted chips (kind badges) and
// 11 px captions, so it must meet WCAG AA for normal text (4.5:1) on each of them.
const surfaces = ["--background", "--card", "--popover", "--muted", "--accent", "--secondary", "--sidebar", "--sidebar-accent"]

function tokens(selector: ":root" | ".dark") {
  const start = styles.indexOf(`${selector} {`)
  const block = styles.slice(start, styles.indexOf("}", start))
  return new Map([...block.matchAll(/(--[\w-]+):\s*oklch\(([\d.]+) ([\d.]+) ([\d.]+)\)/g)].map(([, name, lightness, chroma, hue]) => [name, { lightness: Number(lightness), chroma: Number(chroma), hue: Number(hue) }]))
}

// OKLab's published inverse sRGB transform: https://bottosson.github.io/posts/oklab/
function linearRgb({ lightness: L, chroma: C, hue: H }: { lightness: number; chroma: number; hue: number }) {
  const a = C * Math.cos(H * Math.PI / 180)
  const b = C * Math.sin(H * Math.PI / 180)
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ].map(channel => Math.max(0, Math.min(1, channel)))
}

function rgbLuminance([r, g, b]: number[]) {
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

// For an achromatic OKLab colour the linear sRGB channels all equal L³, which is also its
// relative luminance.
function luminance(token: { lightness: number; chroma: number }) {
  expect(token.chroma).toBe(0)
  return token.lightness ** 3
}

function contrast(a: number, b: number) {
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)
}

function srgb(linear: number) {
  return linear <= 0.0031308 ? 12.92 * linear : 1.055 * linear ** (1 / 2.4) - 0.055
}

function linear(srgb: number) {
  return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4
}

describe("muted text contrast", () => {
  it.each([":root", ".dark"] as const)("meets WCAG AA on every neutral surface in %s", (selector) => {
    const theme = tokens(selector)
    const text = luminance(theme.get("--muted-foreground")!)
    for (const surface of surfaces) {
      const ratio = contrast(text, luminance(theme.get(surface)!))
      expect(ratio, `${selector} --muted-foreground on ${surface}: ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
    }
  })
})

describe("pending operation steps", () => {
  it.each([":root", ".dark"] as const)("keeps pending text readable on the toast surface in %s", (selector) => {
    const { container } = render(createElement(OperationToastBody, { steps: [{ label: "Boot computer", state: "pending" }] }))
    const row = container.querySelector("li[data-state=pending]")!
    const color = row.className.match(/\btext-muted-foreground(?:\/(\d+))?\b/)
    expect(color).not.toBeNull()
    const opacity = color?.[1] ? Number(color[1]) / 100 : 1
    const theme = tokens(selector)
    const background = luminance(theme.get("--popover")!)
    const foreground = luminance(theme.get("--muted-foreground")!)
    const paintedText = linear(srgb(foreground) * opacity + srgb(background) * (1 - opacity))
    expect(contrast(paintedText, background)).toBeGreaterThanOrEqual(4.5)
  })
})

describe("destructive text contrast", () => {
  it.each([":root", ".dark"] as const)("meets WCAG AA for error text and destructive button states in %s", (selector) => {
    const theme = tokens(selector)
    const text = linearRgb(theme.get("--destructive")!)
    const classes = buttonVariants({ variant: "destructive" }).split(" ")
    const dark = selector === ".dark"
    const opacity = (hover: boolean) => {
      const prefix = `${dark ? "dark:" : ""}${hover ? "hover:" : ""}bg-destructive/`
      const value = classes.find(value => value.startsWith(prefix)) ?? classes.find(value => value.startsWith(`${hover ? "hover:" : ""}bg-destructive/`))!
      return Number(value.split("/").at(-1)) / 100
    }
    for (const surface of surfaces) {
      const background = linearRgb(theme.get(surface)!)
      for (const alpha of [0, opacity(false), opacity(true)]) {
        const tinted = background.map((channel, index) => linear(srgb(text[index]) * alpha + srgb(channel) * (1 - alpha)))
        const ratio = contrast(rgbLuminance(text), rgbLuminance(tinted))
        expect(ratio, `${selector} destructive on ${surface} at ${alpha}: ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
      }
    }
  })
})

describe.each(["--success", "--warning"] as const)("%s text contrast", (token) => {
  it.each([":root", ".dark"] as const)("meets WCAG AA on every neutral surface in %s", (selector) => {
    const theme = tokens(selector)
    const text = rgbLuminance(linearRgb(theme.get(token)!))
    for (const surface of surfaces) {
      const ratio = contrast(text, luminance(theme.get(surface)!))
      expect(ratio, `${selector} ${token} on ${surface}: ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
    }
  })

  it.each([":root", ".dark"] as const)("keeps its foreground readable on a solid fill in %s", (selector) => {
    const theme = tokens(selector)
    const fill = rgbLuminance(linearRgb(theme.get(token)!))
    const foreground = rgbLuminance(linearRgb(theme.get(`${token}-foreground`)!))
    expect(contrast(fill, foreground)).toBeGreaterThanOrEqual(3)
  })
})

describe("focus ring contrast", () => {
  it.each([":root", ".dark"] as const)("meets the 3:1 non-text contrast on the page background in %s", (selector) => {
    const theme = tokens(selector)
    const ratio = contrast(luminance(theme.get("--ring")!), luminance(theme.get("--background")!))
    expect(ratio).toBeGreaterThanOrEqual(3)
  })
})

describe("secret restart notice contrast", () => {
  it("styles the notice with the warning token", () => {
    const source = structuredClone(applicationSourceForScenario("running"))
    source.secrets[0] = { ...source.secrets[0], state: "restart-required", pendingComputers: ["dev"] }
    render(createElement(SecretsPage, { source, onSaveSecret: () => {}, onRemoveSecret: () => {} }))
    expect(screen.getByText("Restart to apply: dev").className).toMatch(/\btext-warning\b/)
  })
})
