import { useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { Cell, QueryResult } from "../api";

const ROW_H = 26;
const NUM_W = 52;

function display(v: Cell): string {
  if (v === null) return "NULL";
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

function toCsv(r: QueryResult): string {
  const esc = (s: string) => (/[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);
  const lines = [r.columns.map(esc).join(",")];
  for (const row of r.rows) lines.push(row.map((v) => (v === null ? "" : esc(display(v)))).join(","));
  return lines.join("\n");
}

export function ResultsGrid({ result }: { result: QueryResult }) {
  const parent = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);

  const widths = useMemo(() => {
    const sample = result.rows.slice(0, 200);
    return result.columns.map((c, i) => {
      const longest = Math.max(c.length + 2, ...sample.map((r) => display(r[i]).length));
      return Math.min(360, Math.max(80, longest * 7.4 + 20));
    });
  }, [result]);
  const template = `${NUM_W}px ${widths.map((w) => `${w}px`).join(" ")}`;
  const totalWidth = NUM_W + widths.reduce((a, b) => a + b, 0);

  const virt = useVirtualizer({
    count: result.rows.length,
    getScrollElement: () => parent.current,
    estimateSize: () => ROW_H,
    overscan: 20,
  });

  const copy = async () => {
    await navigator.clipboard.writeText(toCsv(result));
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  if (result.columns.length === 0) {
    return (
      <div className="results-empty">
        Done. {result.rowsAffected ?? 0} row{result.rowsAffected === 1 ? "" : "s"} affected · {result.elapsedMs} ms
      </div>
    );
  }

  return (
    <div className="results">
      <div className="results-bar">
        <span>
          {result.rows.length.toLocaleString()} row{result.rows.length === 1 ? "" : "s"}
          {result.truncated && " (first 5,000 shown)"} · {result.elapsedMs} ms
        </span>
        <button className="ghost small" onClick={copy}>
          {copied ? "Copied" : "Copy as CSV"}
        </button>
      </div>
      <div className="grid-scroll" ref={parent}>
        <div style={{ width: totalWidth, minWidth: "100%" }}>
          <div className="grid-row grid-head" style={{ gridTemplateColumns: template }}>
            <div className="cell num">#</div>
            {result.columns.map((c, i) => (
              <div className="cell" key={i} title={c}>
                {c}
              </div>
            ))}
          </div>
          <div style={{ height: virt.getTotalSize(), position: "relative" }}>
            {virt.getVirtualItems().map((v) => {
              const row = result.rows[v.index];
              return (
                <div
                  key={v.key}
                  className={`grid-row${v.index % 2 ? " odd" : ""}`}
                  style={{ gridTemplateColumns: template, transform: `translateY(${v.start}px)` }}
                >
                  <div className="cell num">{v.index + 1}</div>
                  {row.map((cell, i) => (
                    <div
                      key={i}
                      className={`cell${cell === null ? " null" : typeof cell === "number" ? " number" : ""}`}
                      title={display(cell)}
                    >
                      {display(cell)}
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
