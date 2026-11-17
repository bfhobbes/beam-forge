/**
 * Toolpaths for tests, timed the simple way: cuts at their feed and rapids at
 * `rapidRate`, with no acceleration, so expected times are easy to work out
 * by hand. The app's toolpaths are timed by the desktop app's estimator.
 */
import type { Toolpath } from "#lib/bindings/gcode/Toolpath.js";
import type { Plan } from "#lib/bindings/plan/Plan.js";

export function atFeedRate(plan: Plan, rapidRate = 6000): Toolpath {
  const moveSeconds: number[] = [];
  let at = plan.start;
  const layers = plan.layers.map((layer) => {
    let seconds = 0;
    for (const step of layer.moves) {
      const length = Math.hypot(step.to.x - at.x, step.to.y - at.y);
      const rate = step.kind === "cut" ? step.speed : rapidRate;
      const time = (length / rate) * 60;
      moveSeconds.push(time);
      seconds += time;
      at = step.to;
    }
    return { layerId: layer.layerId, seconds };
  });
  return {
    plan,
    moveSeconds,
    estimate: { seconds: layers.reduce((sum, layer) => sum + layer.seconds, 0), layers },
  };
}
