import { expect, it } from "vitest"

import styles from "@/index.css?raw"
import { STATUS_PANEL_MAX_HEIGHT } from "@/desktop/use-status-panel-size"

it("keeps the panel height used for resizing equal to the stylesheet's", () => {
  expect(styles).toContain(`--status-panel-height: ${STATUS_PANEL_MAX_HEIGHT}px;`)
})
