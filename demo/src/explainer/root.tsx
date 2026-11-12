import React from "react";
import { Composition } from "remotion";
import { Explainer } from "./film.tsx";
import { DURATION, FPS, HEIGHT, WIDTH } from "./timeline.ts";

export const Root: React.FC = () => (
  <Composition
    id="SiloExplainer"
    component={Explainer}
    durationInFrames={DURATION}
    fps={FPS}
    width={WIDTH}
    height={HEIGHT}
    defaultProps={{ narration: true }}
  />
);
