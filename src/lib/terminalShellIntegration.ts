export interface ShellCompletionScan {
  remainder: string;
  exitCodes: Array<number | null>;
}

const markerStartPattern = /(?:\u001b\]|\u009d)(?:133|633);D/g;
const markerPrefixes = ["\u001b]133;D", "\u001b]633;D", "\u009d133;D", "\u009d633;D"];

function trailingMarkerPrefix(value: string): string {
  const maximum = Math.min(
    value.length,
    Math.max(...markerPrefixes.map((prefix) => prefix.length)) - 1
  );
  for (let length = maximum; length > 0; length -= 1) {
    const suffix = value.slice(-length);
    if (markerPrefixes.some((prefix) => prefix.startsWith(suffix))) return suffix;
  }
  return "";
}

function markerTerminator(value: string, from: number): { index: number; length: number } | null {
  for (let index = from; index < value.length; index += 1) {
    if (value[index] === "\u0007" || value[index] === "\u009c") {
      return { index, length: 1 };
    }
    if (value[index] === "\u001b" && value[index + 1] === "\\") {
      return { index, length: 2 };
    }
  }
  return null;
}

export function consumeShellCompletionMarkers(
  remainder: string,
  chunk: string
): ShellCompletionScan {
  const value = `${remainder}${chunk}`;
  const exitCodes: Array<number | null> = [];
  let cursor = 0;

  while (cursor < value.length) {
    markerStartPattern.lastIndex = cursor;
    const start = markerStartPattern.exec(value);
    if (!start) {
      return {
        remainder: trailingMarkerPrefix(value.slice(cursor)),
        exitCodes
      };
    }

    const bodyStart = start.index + start[0].length;
    const terminator = markerTerminator(value, bodyStart);
    if (!terminator) {
      return {
        remainder: value.slice(start.index),
        exitCodes
      };
    }

    const body = value.slice(bodyStart, terminator.index);
    const status = body.match(/^;(-?\d+)$/);
    exitCodes.push(status ? Number(status[1]) : null);
    cursor = terminator.index + terminator.length;
  }

  return { remainder: "", exitCodes };
}
