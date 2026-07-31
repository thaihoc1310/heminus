import { confirmDialog } from "./dialog";

export function draftSnapshot(value: unknown): string {
  return JSON.stringify(value);
}

export function draftChanged(baseline: string | null, value: unknown): boolean {
  return baseline !== null && baseline !== draftSnapshot(value);
}

export async function confirmDiscardChanges(changed: boolean): Promise<boolean> {
  if (!changed) return true;
  return confirmDialog({
    title: "Discard changes?",
    message: "You have unsaved changes. Discard them and close the editor?",
    confirmLabel: "Discard",
    danger: true
  });
}
