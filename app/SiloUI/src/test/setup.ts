import "@testing-library/jest-dom/vitest"

import { afterAll, afterEach, beforeEach, vi } from "vitest"
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks"
import { toast } from "sonner"
import { collectUnexpectedConsoleErrors, installConsoleErrorGuard } from "./console-error-guard"
import { gateSonnerStyleSheet, purgeDetachedStyleSheets } from "./stylesheets"

// Sonner keeps its toast store at module scope; clear it so notifications never leak between tests.
afterEach(() => { toast.dismiss() })

// Sonner removes a dismissed toast 200 ms later, even after its Toaster unmounted, and
// updating state once the file's jsdom window is gone is an unhandled error. Let those
// timers run before the environment is torn down.
const SONNER_REMOVAL_DELAY_MS = 250
afterAll(async () => {
  if (toast.getHistory().length > 0 && !vi.isFakeTimers()) await new Promise((resolve) => setTimeout(resolve, SONNER_REMOVAL_DELAY_MS))
})

// Keep stylesheets no element can match out of jsdom's style computation: Sonner's
// sheet while no toast is shown, and sheets jsdom leaks from removed components.
gateSonnerStyleSheet(window)
beforeEach(() => { purgeDetachedStyleSheets(document) })

// Without a Tauri webview, the real `invoke`/`listen` throw inside
// transformCallback and surface only as caught console errors. Install
// Tauri's own IPC mock so modules that are not vi.mock'ed get a working
// bridge: event listeners register, and unknown commands resolve undefined.
// Suites that need specific command results still vi.mock the modules or call
// mockIPC themselves.
beforeEach(() => {
  mockIPC(() => undefined, { shouldMockEvents: true })
})
afterEach(() => { clearMocks() })

// Fail on every unexpected console.error, including React act warnings. The check
// runs in onTestFinished, after every afterEach hook, so throwing here cannot
// skip DOM cleanup or timer and mock restoration.
installConsoleErrorGuard()
beforeEach(({ onTestFinished }) => {
  onTestFinished(() => {
    const unexpected = collectUnexpectedConsoleErrors()
    if (unexpected.length > 0) {
      throw new Error(`Unexpected console.error during test:\n${unexpected.join("\n---\n")}`)
    }
  })
})

Object.defineProperty(navigator, "clipboard", {
  configurable: true,
  value: { writeText: vi.fn().mockResolvedValue(undefined) },
})

// jsdom omits matchMedia; the Sonner toaster and a few hooks call it. Individual
// suites still override this with vi.stubGlobal when they assert on media state.
if (typeof window.matchMedia !== "function") {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }),
  })
}

class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}

Object.defineProperty(window, "ResizeObserver", { configurable: true, value: ResizeObserverStub })
Object.defineProperty(Element.prototype, "hasPointerCapture", { configurable: true, value: () => false })
Object.defineProperty(Element.prototype, "setPointerCapture", { configurable: true, value: () => undefined })
Object.defineProperty(Element.prototype, "releasePointerCapture", { configurable: true, value: () => undefined })
Object.defineProperty(Element.prototype, "scrollIntoView", { configurable: true, value: () => undefined })
