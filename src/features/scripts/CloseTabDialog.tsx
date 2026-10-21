import { Button, Dialog } from "../../components/ui";

export interface CloseTabDialogProps {
  tabName: string;
  onCancel: () => void;
  onDiscard: () => void;
  onSave: () => void;
}

/** Shown when closing a tab whose SQL isn't saved. */
export function CloseTabDialog({ tabName, onCancel, onDiscard, onSave }: CloseTabDialogProps) {
  return (
    <Dialog
      title={`Save changes to “${tabName}”?`}
      size="sm"
      onClose={onCancel}
      secondaryActions={
        <Button variant="dangerGhost" onClick={onDiscard}>
          Don’t save
        </Button>
      }
      actions={
        <>
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant="primary" onClick={onSave} autoFocus>
            Save
          </Button>
        </>
      }
    >
      <p style={{ margin: 0 }}>This tab has SQL that isn’t saved. If you close it without saving, it’s gone.</p>
    </Dialog>
  );
}
