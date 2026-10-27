import { useState } from "react";
import { commands, type TeamAccount } from "../../api";
import { Alert, Button, Dialog, Field, TextInput } from "../../components/ui";
import { errorText } from "../../lib/errors";
import { serverHost } from "./teamText";
import styles from "./Team.module.css";

export interface TeamSignInDialogProps {
  server?: string;
  code?: string;
  /** Opened from a sayql://team-join link: anyone can make one, so the person checks the server. */
  fromLink?: boolean;
  onSignedIn: (account: TeamAccount) => void;
  onClose: () => void;
}

/** "Sign in to your team": a server address and an invite code. Nothing is sent until Sign in. */
export function TeamSignInDialog({ server: initialServer = "", code: initialCode = "", fromLink, onSignedIn, onClose }: TeamSignInDialogProps) {
  const [server, setServer] = useState(initialServer);
  const [code, setCode] = useState(initialCode);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const signIn = () => {
    setBusy(true);
    setError(null);
    commands.teamSignIn(server, code).then(onSignedIn, (e) => {
      setBusy(false);
      setError(errorText(e));
    });
  };

  return (
    <Dialog
      title="Sign in to your team"
      size="sm"
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" loading={busy} disabled={!server.trim() || !code.trim()} onClick={signIn}>
            Sign in
          </Button>
        </>
      }
    >
      <div className={styles.body}>
        {fromLink && (
          <Alert tone="warning">
            An invite link asks you to sign in to <strong>{serverHost(server)}</strong>. Continue only if your team admin sent it to you.
          </Alert>
        )}
        <p className={styles.lead}>
          Your company's SayQL server shares its database connections with you. Queries on them, and the questions they answer, go
          to that server, which keeps them in its audit log.
        </p>
        <Field label="Server" hint="From your team admin, e.g. https://sayql.example.com">
          {(id) => (
            <TextInput
              id={id}
              mono
              value={server}
              placeholder="https://sayql.example.com"
              onChange={(e) => setServer(e.target.value)}
              autoFocus={!fromLink}
            />
          )}
        </Field>
        <Field label="Invite code" hint="It works once and expires after 7 days.">
          {(id) => <TextInput id={id} mono value={code} onChange={(e) => setCode(e.target.value)} />}
        </Field>
        {error && <Alert tone="danger">{error}</Alert>}
      </div>
    </Dialog>
  );
}
