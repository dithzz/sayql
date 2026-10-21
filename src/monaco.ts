// Load Monaco from the local package (not a CDN) so the editor works offline.
import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
import "monaco-editor/esm/vs/basic-languages/sql/sql.contribution";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import { loader } from "@monaco-editor/react";
import { qualifiedName, type Schema } from "./api";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

monaco.editor.defineTheme("askdb", {
  base: "vs-dark",
  inherit: true,
  rules: [
    { token: "keyword", foreground: "7aa2f7", fontStyle: "bold" },
    { token: "string", foreground: "9ece6a" },
    { token: "number", foreground: "ff9e64" },
    { token: "comment", foreground: "6b7394", fontStyle: "italic" },
  ],
  colors: {
    "editor.background": "#151821",
    "editor.lineHighlightBackground": "#1c2030",
    "editorLineNumber.foreground": "#3b4261",
    "editorCursor.foreground": "#c0caf5",
  },
});

const KEYWORDS = [
  "SELECT", "FROM", "WHERE", "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "JOIN", "LEFT JOIN", "INNER JOIN",
  "ON", "AS", "AND", "OR", "NOT", "IN", "IS NULL", "IS NOT NULL", "COUNT", "SUM", "AVG", "MIN", "MAX",
  "DISTINCT", "CASE", "WHEN", "THEN", "ELSE", "END", "WITH", "UNION", "DESC", "ASC", "BETWEEN", "LIKE",
];

let currentSchema: Schema | null = null;
export function setCompletionSchema(s: Schema | null) {
  currentSchema = s;
}

monaco.languages.registerCompletionItemProvider("sql", {
  provideCompletionItems(model, position) {
    const word = model.getWordUntilPosition(position);
    const range = new monaco.Range(position.lineNumber, word.startColumn, position.lineNumber, word.endColumn);
    const K = monaco.languages.CompletionItemKind;
    const items: monaco.languages.CompletionItem[] = KEYWORDS.map((k) => ({
      label: k,
      kind: K.Keyword,
      insertText: k,
      range,
    }));
    for (const t of currentSchema?.tables ?? []) {
      items.push({ label: qualifiedName(t), kind: K.Struct, detail: t.kind, insertText: qualifiedName(t), range });
      for (const c of t.columns) {
        items.push({ label: c.name, kind: K.Field, detail: `${t.name} · ${c.data_type}`, insertText: c.name, range });
      }
    }
    return { suggestions: items };
  },
});

export { monaco };
