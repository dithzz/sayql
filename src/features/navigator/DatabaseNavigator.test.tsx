// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConnectionConfig, Schema, Table } from "../../api";
import { DatabaseNavigator } from "./DatabaseNavigator";

const conn: ConnectionConfig = {
  id: "c1", name: "Shop", kind: "sqlite", host: "", port: null, database: "", user: "", file: "/shop.sqlite", ssl: false, trustCert: false,
};

const table = (name: string, kind: Table["kind"]): Table => ({
  schema: null, name, kind, columns: [], foreign_keys: [], row_estimate: null,
});

describe("DatabaseNavigator", () => {
  afterEach(cleanup);

  it("lists materialized views with the views", () => {
    const schema: Schema = { dialect: "SQLite", tables: [table("events", "table"), table("daily_totals", "materialized view")] };
    render(
      <DatabaseNavigator
        connections={[conn]} activeId="c1" databases={{ c1: null }} databaseName={null} schema={schema} loading={false}
        onOpen={vi.fn()} onOpenDatabase={vi.fn()} onAdd={vi.fn()} onEdit={vi.fn()} onDeleteConnection={vi.fn()} onRefresh={vi.fn()}
        onPreviewTable={vi.fn()} onNewTab={vi.fn()} onOpenSql={vi.fn()} selectedTables={[]} onSelectTables={vi.fn()}
        scripts={[]} onOpenScript={vi.fn()} onDeleteScript={vi.fn()}
        notes={[{ id: "n1", connectionId: "c1", database: null, text: "status 3 = cancelled", updatedAt: 0 }]}
        onEditNote={vi.fn()} onDeleteNote={vi.fn()} onAgentSettings={vi.fn()}
      />,
    );
    expect(screen.getByText("Notes")).toBeTruthy();
    expect(screen.getByText("status 3 = cancelled")).toBeTruthy();
    expect(screen.getByText("Views")).toBeTruthy();
    expect(screen.getByText("daily_totals")).toBeTruthy();
    expect(screen.getByText("events")).toBeTruthy();
  });
});
