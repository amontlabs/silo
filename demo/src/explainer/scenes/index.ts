import type React from "react";
import { beyond } from "./beyond.tsx";
import { computers } from "./computers.tsx";
import { engine } from "./engine.tsx";
import { isolation } from "./isolation.tsx";
import { overview } from "./overview.tsx";
import { project } from "./project.tsx";
import { working } from "./working.tsx";

export type Visual = { render: () => React.ReactNode; bare?: boolean };

/** The picture for each scene of the narration, by scene id. */
export const VISUALS: Record<string, Visual> = {
  ...overview,
  ...engine,
  ...computers,
  ...isolation,
  ...working,
  ...beyond,
  ...project,
};
