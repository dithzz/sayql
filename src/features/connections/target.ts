import type { Target } from "../../api";

/** A stable string for a target, for effect dependencies and cache keys. */
export function targetKey(t: Target | null): string {
  return t ? `${t.connectionId}/${t.database ?? ""}` : "";
}

export function sameTarget(a: Target | null, b: Target | null): boolean {
  return targetKey(a) === targetKey(b);
}
