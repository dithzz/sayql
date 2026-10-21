import { useEffect, useState } from "react";
import { Check, Copy } from "lucide-react";
import { commands, type McpSetup } from "../../api";
import { Alert, Button } from "../../components/ui";
import styles from "./ClaudeIntegration.module.css";

function CopyBlock({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className={styles.block}>
      <div className={styles.blockHead}>
        <span>{label}</span>
        <Button size="sm" variant="ghost" icon={copied ? Check : Copy} onClick={copy}>
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <pre className={styles.code}>{text}</pre>
    </div>
  );
}

/** How to let Claude Code / Claude Desktop query the saved connections through MCP. */
export function ClaudeIntegration() {
  const [setup, setSetup] = useState<McpSetup | null>(null);
  useEffect(() => {
    commands.mcpSetup().then(setSetup, () => setSetup(null));
  }, []);
  if (!setup) return null;

  const desktopConfig = JSON.stringify({ mcpServers: { askdb: { command: setup.path } } }, null, 2);
  return (
    <div className={styles.section}>
      <p className={styles.intro}>
        Let Claude list your connections, read schemas and run <strong>read-only</strong> queries. Anything that could
        change data is refused. Query results are sent to Claude.
      </p>
      {!setup.exists && (
        <Alert tone="warning">The MCP server isn&apos;t built yet. Run `cargo build --bin askdb-mcp` in src-tauri.</Alert>
      )}
      <CopyBlock label="Claude Code — run in a terminal" text={`claude mcp add --scope user askdb -- "${setup.path}"`} />
      <CopyBlock label="Claude Desktop — merge into claude_desktop_config.json" text={desktopConfig} />
    </div>
  );
}
