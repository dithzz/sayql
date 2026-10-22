// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { QUERY_CANCELLED } from "../../api";
import { ResultsPanel } from "./ResultsPanel";

describe("ResultsPanel", () => {
  afterEach(cleanup);

  it("shows a cancelled query as cancelled, not as a database error", () => {
    const outcomes = [{ sql: "SELECT pg_sleep(30)", result: null, error: QUERY_CANCELLED, cancelled: false }];
    render(<ResultsPanel outcomes={outcomes} total={1} running={false} />);
    expect(screen.getByText("Query cancelled")).toBeTruthy();
    expect(screen.queryByText(/returned an error/)).toBeNull();
  });

  it("still shows real database errors as errors", () => {
    const outcomes = [{ sql: "SELECT nme FROM t", result: null, error: "column \"nme\" does not exist", cancelled: false }];
    render(<ResultsPanel outcomes={outcomes} total={1} running={false} />);
    expect(screen.getByText("The database returned an error")).toBeTruthy();
  });
});
