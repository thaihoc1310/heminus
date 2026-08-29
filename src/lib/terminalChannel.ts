import type { TerminalChannelMessage, TerminalControlEvent } from "./types";

export type DecodedTerminalMessage =
  | { kind: "output"; data: Uint8Array }
  | TerminalControlEvent;

/**
 * Sorts one terminal channel message into output bytes or a control event.
 *
 * Output used to travel as a JSON array of byte values, which inflated a 16 KiB
 * read to roughly 75 KB of text and made the webview parse it back into a boxed
 * number array. It now arrives as an `ArrayBuffer`, so the two cases are told
 * apart by type rather than by a discriminant field.
 */
export function decodeTerminalMessage(
  message: TerminalChannelMessage
): DecodedTerminalMessage {
  return message instanceof ArrayBuffer
    ? { kind: "output", data: new Uint8Array(message) }
    : message;
}
