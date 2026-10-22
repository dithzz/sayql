import { describe, expect, it } from "vitest";
import { splitForRun, statementRanges } from "./sqlStatements";

const texts = (sql: string) => statementRanges(sql).map((r) => sql.slice(r.start, r.end));

describe("statementRanges", () => {
  it("splits at semicolons", () => {
    expect(texts("SELECT 1;\n\nSELECT 2;\nSELECT 3")).toEqual(["SELECT 1;", "SELECT 2;", "SELECT 3"]);
  });

  it("ignores semicolons in strings, quoted names and comments", () => {
    const sql = "SELECT ';' AS \"a;b\", [c;d] -- x;y\nFROM t /* ; */;\nSELECT $$;$$;";
    expect(texts(sql)).toEqual(["SELECT ';' AS \"a;b\", [c;d] -- x;y\nFROM t /* ; */;", "SELECT $$;$$;"]);
  });

  it("returns nothing for blank text", () => {
    expect(texts("  \n ")).toEqual([]);
  });
});

describe("splitForRun", () => {
  it("runs each statement on its own, without comment-only parts", () => {
    expect(splitForRun("-- customers\nSELECT * FROM customers;\nSELECT * FROM orders;\n-- done")).toEqual([
      "SELECT * FROM customers;",
      "SELECT * FROM orders;",
    ]);
    expect(splitForRun("SELECT 1")).toEqual(["SELECT 1"]);
    expect(splitForRun("  -- nothing\n")).toEqual([]);
  });

  it("sends a procedure or trigger definition whole", () => {
    const sql = "CREATE PROCEDURE p AS BEGIN SELECT 1; SELECT 2; END;";
    expect(splitForRun(sql)).toEqual([sql]);
    expect(splitForRun("create or replace function f() returns int as $$ select 1; $$ language sql;")).toHaveLength(1);
  });
});
