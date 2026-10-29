import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(import.meta.dirname, "..");

/** Every stylesheet and component under src/, as [path, text]. */
function sources(dir = SRC): [string, string][] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return sources(path);
    return /\.(css|tsx)$/.test(e.name) ? [[path.slice(SRC.length + 1), readFileSync(path, "utf8")] as [string, string]] : [];
  });
}

describe("design tokens", () => {
  it("are defined wherever they're used", () => {
    const tokens = readFileSync(join(SRC, "styles", "tokens.css"), "utf8");
    const defined = new Set([...tokens.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
    expect(defined.size).toBeGreaterThan(20);
    const missing = sources().flatMap(([file, text]) =>
      [...text.matchAll(/var\((--[a-z0-9-]+)/g)].filter(([, name]) => !defined.has(name)).map(([, name]) => `${file}: ${name}`),
    );
    expect(missing).toEqual([]);
  });
});
