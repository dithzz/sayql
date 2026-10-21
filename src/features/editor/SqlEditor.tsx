import { useEffect } from "react";
import Editor from "@monaco-editor/react";
import type { Schema } from "../../api";
import { useLatest } from "../../hooks/useLatest";
import { useTheme } from "../theme";
import { monaco, monacoTheme, setCompletionSchema } from "./monacoSetup";

export interface SqlEditorProps {
  value: string;
  onChange: (sql: string) => void;
  /** ⌘↵ / Ctrl+↵ inside the editor. */
  onRun: () => void;
  schema: Schema | null;
  /** Only the visible editor's schema feeds autocomplete. */
  active: boolean;
}

export function SqlEditor({ value, onChange, onRun, schema, active }: SqlEditorProps) {
  const { resolved } = useTheme();
  const runRef = useLatest(onRun);

  useEffect(() => {
    if (active) setCompletionSchema(schema);
  }, [active, schema]);

  return (
    <Editor
      language="sql"
      theme={monacoTheme(resolved)}
      value={value}
      onChange={(v) => onChange(v ?? "")}
      onMount={(editor) => {
        editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => runRef.current());
      }}
      options={{
        minimap: { enabled: false },
        fontSize: 13,
        fontFamily: "'JetBrains Mono', 'SF Mono', Menlo, Consolas, monospace",
        lineNumbersMinChars: 3,
        scrollBeyondLastLine: false,
        wordWrap: "on",
        automaticLayout: true,
        padding: { top: 8 },
        renderLineHighlight: "line",
        overviewRulerBorder: false,
        scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10 },
      }}
    />
  );
}
