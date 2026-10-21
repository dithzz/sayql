import { useRef } from "react";

/** A ref that always holds the latest value — for long-lived callbacks (event listeners, editor commands). */
export function useLatest<T>(value: T) {
  const ref = useRef(value);
  ref.current = value;
  return ref;
}
