export interface PromptDialogOptions {
  title: string;
  message?: string;
  label?: string;
  initialValue?: string;
  placeholder?: string;
  confirmLabel?: string;
}

export interface ConfirmDialogOptions {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
}

export interface AlertDialogOptions {
  title: string;
  message: string;
  confirmLabel?: string;
}

export type AppDialogRequest =
  | (PromptDialogOptions & {
      kind: "prompt";
      resolve: (value: string | null) => void;
    })
  | (ConfirmDialogOptions & {
      kind: "confirm";
      resolve: (value: boolean) => void;
    })
  | (AlertDialogOptions & {
      kind: "alert";
      resolve: () => void;
    });

let listener: ((request: AppDialogRequest) => void) | null = null;

export function registerDialogListener(
  next: (request: AppDialogRequest) => void
): () => void {
  listener = next;
  return () => {
    if (listener === next) listener = null;
  };
}

export function promptDialog(options: PromptDialogOptions): Promise<string | null> {
  return new Promise((resolve) => {
    if (!listener) {
      resolve(null);
      return;
    }
    listener({ ...options, kind: "prompt", resolve });
  });
}

export function confirmDialog(options: ConfirmDialogOptions): Promise<boolean> {
  return new Promise((resolve) => {
    if (!listener) {
      resolve(false);
      return;
    }
    listener({ ...options, kind: "confirm", resolve });
  });
}

export function alertDialog(options: AlertDialogOptions): Promise<void> {
  return new Promise((resolve) => {
    if (!listener) {
      resolve();
      return;
    }
    listener({ ...options, kind: "alert", resolve });
  });
}
