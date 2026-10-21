import { useEffect, useState } from "react";
import { storage } from "../lib/storage";

/** useState that remembers its value in localStorage under `key`. */
export function usePersistentState<T>(key: string, initial: T) {
  const [value, setValue] = useState<T>(() => storage.get(key, initial));
  useEffect(() => storage.set(key, value), [key, value]);
  return [value, setValue] as const;
}
