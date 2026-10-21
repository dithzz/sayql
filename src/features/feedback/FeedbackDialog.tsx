import { useEffect, useState } from "react";
import { Github, Lock } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands, type AppInfo } from "../../api";
import { Button, Dialog, Field, SegmentedControl } from "../../components/ui";
import { FEEDBACK_FORM_URL, githubIssueUrl, osLabel, privateFormUrl, type FeedbackKind } from "./links";
import styles from "./FeedbackDialog.module.css";

const KINDS: { value: FeedbackKind; label: string }[] = [
  { value: "feedback", label: "Feedback" },
  { value: "bug", label: "Something's wrong" },
  { value: "idea", label: "Idea" },
];

/** Opens a pre-filled GitHub issue (public) or the private form, in the browser. */
export function FeedbackDialog({ onClose }: { onClose: () => void }) {
  const [kind, setKind] = useState<FeedbackKind>("feedback");
  const [text, setText] = useState("");
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    commands.appInfo().then(setInfo, () => setInfo({ version: "unknown", os: "unknown", arch: "unknown" }));
  }, []);

  const open = (url: string) => {
    void openUrl(url);
    onClose();
  };

  return (
    <Dialog
      title="Send feedback"
      onClose={onClose}
      secondaryActions={<Button onClick={onClose}>Cancel</Button>}
      actions={
        <>
          {FEEDBACK_FORM_URL && (
            <Button icon={Lock} disabled={!info} onClick={() => info && open(privateFormUrl(FEEDBACK_FORM_URL, kind, info))}>
              Send privately
            </Button>
          )}
          <Button variant="primary" icon={Github} disabled={!info} onClick={() => info && open(githubIssueUrl(kind, text, info))}>
            Post on GitHub
          </Button>
        </>
      }
    >
      <p className={styles.intro}>
        Every report is read. What you asked, and whether SayQL got the SQL right, helps most.
      </p>
      <SegmentedControl ariaLabel="Kind of feedback" value={kind} options={KINDS} onChange={setKind} />
      <Field label="What's on your mind?" optional hint="You can add more on the next page.">
        {(id) => (
          <textarea
            id={id}
            className={styles.text}
            rows={5}
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={kind === "bug" ? "What happened, and what did you expect?" : kind === "idea" ? "What are you trying to do?" : "What worked, what didn't?"}
          />
        )}
      </Field>
      <p className={styles.privacy}>
        {FEEDBACK_FORM_URL ? "Post on GitHub is public and needs a GitHub account; Send privately needs no account. " : "This opens a public GitHub issue (you'll need a GitHub account). "}
        Only your SayQL version{info ? ` (${info.version})` : ""} and operating system{info ? ` (${osLabel(info)})` : ""} are added. Never include
        passwords, keys or private data.
      </p>
    </Dialog>
  );
}
