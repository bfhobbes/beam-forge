/**
 * Following a running design on its toolpath: which moves are done, from the
 * job's acknowledged lines and the head's reported position.
 *
 * An acknowledged line has only reached GRBL's planner, which runs up to
 * ~16 moves behind it, so the line count alone runs ahead of the head. The
 * head's position says where it really is: the done moves are those before
 * the move the head is on, searched forward from the last answer and no
 * further than the acknowledged lines (the head can't be on a move GRBL
 * hasn't taken yet). Searching forward only keeps a later pass over the
 * same outline from matching early.
 */
import type { WorkPlacement } from "#lib/bindings/gcode/WorkPlacement.js";
import type { Point } from "#lib/bindings/project/Point.js";
import { moveStart, type Timeline } from "#lib/toolpath/timeline.js";

/** How far, mm, the reported head may be from a move and still be on it. GRBL reports to a micron. */
export const ON_MOVE_MM = 0.05;

/** A work position (as the machine reports it) on the design, undoing the job's placement. */
export function toDocument(placement: WorkPlacement, work: { x: number; y: number }): Point {
  const x = work.x - placement.offset.x;
  const y = work.y - placement.offset.y;
  return {
    x: placement.mirrorWidth === null ? x : placement.mirrorWidth - x,
    y: placement.mirrorHeight === null ? y : placement.mirrorHeight - y,
  };
}

/** How many moves the controller has taken: those whose line is acknowledged. `moveLines` never decreases. */
export function takenMoves(moveLines: readonly number[], acknowledged: number): number {
  let low = 0;
  let high = moveLines.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (moveLines[mid]! <= acknowledged) low = mid + 1;
    else high = mid;
  }
  return low;
}

/** Where `head` falls along move `index`: 0 at its start, 1 at its end; and how far off it the head is, mm. */
function along(timeline: Timeline, index: number, head: Point): { t: number; off: number } {
  const from = moveStart(timeline, index);
  const dx = timeline.x[index]! - from.x;
  const dy = timeline.y[index]! - from.y;
  const length2 = dx * dx + dy * dy;
  const t =
    length2 > 0
      ? Math.min(1, Math.max(0, ((head.x - from.x) * dx + (head.y - from.y) * dy) / length2))
      : 1;
  const off = Math.hypot(from.x + dx * t - head.x, from.y + dy * t - head.y);
  return { t, off };
}

/**
 * Moves done, given the moves the controller has taken (`taken`), where the
 * head is and the last answer (`previous`). Never goes back. Without a head
 * position, or with the head on none of the moves it could be on, it stays.
 */
export function followHead(
  timeline: Timeline,
  taken: number,
  head: Point | null,
  previous: number,
): number {
  const last = Math.min(taken, timeline.count);
  if (!head || previous >= last) return previous;
  let done = -1;
  for (let index = previous; index < last; index++) {
    const { t, off } = along(timeline, index, head);
    if (off > ON_MOVE_MM) {
      if (done >= 0) break;
      continue;
    }
    // At a move's end the head may already be on the next one.
    const end = Math.hypot(timeline.x[index]! - head.x, timeline.y[index]! - head.y);
    if (end > ON_MOVE_MM && t < 1) return index;
    done = index + 1;
  }
  return done >= 0 ? done : previous;
}

/** Seconds into the job's timeline the head is, `completed` moves done. */
export function timeAtHead(timeline: Timeline, completed: number, head: Point | null): number {
  if (completed >= timeline.count) return timeline.duration;
  const began = completed > 0 ? timeline.ends[completed - 1]! : 0;
  if (!head) return began;
  const { t } = along(timeline, completed, head);
  return began + t * (timeline.ends[completed]! - began);
}
