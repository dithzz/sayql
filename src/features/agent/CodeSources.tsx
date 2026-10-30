import { useEffect, useState } from "react";
import { AlertTriangle, FolderOpen, FolderPlus, GitBranch, Loader2, Plus, RefreshCw, Trash2 } from "lucide-react";
import { commands, type CodeSource } from "../../api";
import { Alert, Button, Field, IconButton, Spinner, TextInput } from "../../components/ui";
import { errorText } from "../../lib/errors";
import styles from "./CodeSources.module.css";

/** Hosts whose token user name SayQL knows (src-tauri/src/repos.rs `token_user`). */
export function knownTokenHost(url: string): boolean {
  const m = /^(?:https:\/\/|ssh:\/\/|git@)(?:[^@/]*@)?([^/:]+)/i.exec(url.trim());
  const host = m?.[1].toLowerCase() ?? "";
  return host === "github.com" || host.endsWith(".github.com") || host === "gitlab.com" || host.startsWith("gitlab.") || host === "bitbucket.org";
}

/** "1a2b3c4", "fetched 2 hours ago": what a source's status line says. */
export function sourceStatus(s: CodeSource, now = Date.now()): string {
  const git = s.kind.type === "git";
  if (!s.lastFetch) return git ? "Fetched when the audit runs" : "Read where it is";
  const mins = Math.floor(Math.max(0, now - s.lastFetch) / 60_000);
  const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
  const ago =
    mins < 1 ? "just now" : mins < 60 ? rtf.format(-mins, "minute") : mins < 48 * 60 ? rtf.format(-Math.floor(mins / 60), "hour") : rtf.format(-Math.floor(mins / 1440), "day");
  const commit = s.lastCommit ? ` · ${s.lastCommit.slice(0, 7)}` : "";
  return `${git ? "Fetched" : "Read"} ${ago}${commit}`;
}

function where(s: CodeSource): string {
  if (s.kind.type === "local") return s.kind.path;
  const branch = s.kind.branch || "default branch";
  return `${s.kind.url} · ${branch}${s.kind.subdir ? ` · ${s.kind.subdir}` : ""}`;
}

export interface CodeSourcesProps {
  connectionId: string;
  /** Told the list each time it loads or changes. */
  onChange?: (sources: CodeSource[]) => void;
}

/** The code an audit reads for one connection: folders on this computer and git repositories. */
export function CodeSources({ connectionId, onChange }: CodeSourcesProps) {
  const [sources, setSources] = useState<CodeSource[] | null>(null);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const show = (list: CodeSource[]) => {
    setSources(list);
    onChange?.(list);
  };

  useEffect(() => {
    let live = true;
    setSources(null);
    setAdding(false);
    setError(null);
    if (!connectionId) return;
    Promise.resolve()
      .then(() => commands.codeSources(connectionId))
      .then(
        (list) => {
          if (!live) return;
          setSources(list);
          onChange?.(list);
        },
        () => live && setSources([]),
      );
    return () => {
      live = false;
    };
    // onChange is a callback from the parent: only the connection reloads the list.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectionId]);

  const list = sources ?? [];
  const replace = (s: CodeSource) => show(list.map((x) => (x.id === s.id ? s : x)));

  const addFolder = () => {
    setError(null);
    commands.addCodeFolder(connectionId).then(
      (s) => s && show([...list, s]),
      (e) => setError(errorText(e)),
    );
  };

  const remove = (s: CodeSource) => {
    setError(null);
    commands.removeCodeSource(connectionId, s.id).then(
      () => show(list.filter((x) => x.id !== s.id)),
      (e) => setError(errorText(e)),
    );
  };

  const check = (s: CodeSource) => {
    setError(null);
    setBusy(s.id);
    commands
      .checkCodeSource(connectionId, s.id)
      .then(replace, (e) => setError(errorText(e)))
      .finally(() => setBusy(null));
  };

  return (
    <div className={styles.sources}>
      {sources === null ? (
        <p className={styles.muted}>
          <Spinner /> Loading…
        </p>
      ) : list.length === 0 ? (
        <p className={styles.muted}>No code yet: add the folder or repository with this database&apos;s code.</p>
      ) : (
        <ul className={styles.list} aria-label="Code sources">
          {list.map((s) => (
            <li key={s.id} className={styles.item}>
              {s.kind.type === "git" ? <GitBranch size={14} /> : <FolderOpen size={14} />}
              <div className={styles.text}>
                <span className={styles.name}>
                  <strong>{s.label}</strong>
                  <code className={styles.where} title={where(s)}>
                    {where(s)}
                  </code>
                </span>
                {s.error ? (
                  <span className={styles.error}>
                    <AlertTriangle size={12} /> {s.error}
                  </span>
                ) : (
                  <span className={styles.muted}>
                    {sourceStatus(s)}
                    {s.kind.type === "git" && s.kind.hasToken && " · token saved"}
                  </span>
                )}
              </div>
              <IconButton
                icon={busy === s.id ? Loader2 : RefreshCw}
                size="sm"
                label={`Check access to ${s.label}`}
                title={s.kind.type === "git" ? "Check git can reach this repository" : "Check this folder is still there"}
                disabled={busy === s.id}
                onClick={() => check(s)}
              />
              <IconButton icon={Trash2} size="sm" label={`Remove ${s.label}`} title="Remove this code source" onClick={() => remove(s)} />
            </li>
          ))}
        </ul>
      )}
      {adding ? (
        <RepoForm
          connectionId={connectionId}
          onAdded={(s) => {
            show([...list, s]);
            setAdding(false);
          }}
          onCancel={() => setAdding(false)}
        />
      ) : (
        <div className={styles.add}>
          <Button size="sm" icon={FolderPlus} disabled={!connectionId} onClick={addFolder}>
            Add folder…
          </Button>
          <Button size="sm" icon={Plus} disabled={!connectionId} onClick={() => setAdding(true)}>
            Add repository…
          </Button>
        </div>
      )}
      {error && <Alert tone="danger">{error}</Alert>}
    </div>
  );
}

/** A git repository: its address, branch, a folder in it, and a token if git can't read it as you. */
function RepoForm({ connectionId, onAdded, onCancel }: { connectionId: string; onAdded: (s: CodeSource) => void; onCancel: () => void }) {
  const [url, setUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [subdir, setSubdir] = useState("");
  const [token, setToken] = useState("");
  const [tokenUser, setTokenUser] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const askUser = token.trim() !== "" && url.trim() !== "" && !knownTokenHost(url);

  const add = () => {
    setBusy(true);
    setError(null);
    commands
      .addCodeRepo(connectionId, {
        url: url.trim(),
        branch: branch.trim(),
        subdir: subdir.trim(),
        token: token.trim() || null,
        tokenUser: askUser ? tokenUser.trim() || null : null,
      })
      .then(onAdded, (e) => setError(errorText(e)))
      .finally(() => setBusy(false));
  };

  return (
    <form
      className={styles.form}
      aria-label="Add a repository"
      onSubmit={(e) => {
        e.preventDefault();
        if (url.trim() && !busy) add();
      }}
    >
      <Field label="Repository address" hint="Its https:// or SSH address, as its Clone button shows it. GitHub, GitLab, Bitbucket or any git server.">
        {(id) => <TextInput id={id} mono autoFocus value={url} placeholder="https://github.com/acme/shop.git" onChange={(e) => setUrl(e.target.value)} />}
      </Field>
      <div className={styles.pair}>
        <Field label="Branch" optional>
          {(id) => <TextInput id={id} mono value={branch} placeholder="default branch" onChange={(e) => setBranch(e.target.value)} />}
        </Field>
        <Field label="Folder in the repository" optional>
          {(id) => <TextInput id={id} mono value={subdir} placeholder="services/api" onChange={(e) => setSubdir(e.target.value)} />}
        </Field>
      </div>
      <Field
        label="Read-only token"
        optional
        hint="Only if git can't read it with your own SSH key or credential helper. Kept in your keychain and sent only to this repository's server."
      >
        {(id) => <TextInput id={id} type="password" autoComplete="off" value={token} onChange={(e) => setToken(e.target.value)} />}
      </Field>
      {askUser && (
        <Field label="User name for the token" hint="The user name this server expects with a token.">
          {(id) => <TextInput id={id} value={tokenUser} onChange={(e) => setTokenUser(e.target.value)} />}
        </Field>
      )}
      {error && <Alert tone="danger">{error}</Alert>}
      <div className={styles.add}>
        <Button size="sm" type="submit" variant="primary" disabled={!url.trim() || busy || (askUser && !tokenUser.trim())}>
          {busy ? "Checking access…" : "Add repository"}
        </Button>
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      </div>
      <p className={styles.muted}>SayQL copies it with your own git, on this computer only, and fetches it each time the audit runs.</p>
    </form>
  );
}
