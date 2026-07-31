export interface QuickConnectTarget {
  address: string;
  username: string;
  port: number;
}

function looksLikeHost(value: string): boolean {
  const normalized = value.trim().toLowerCase();
  return (
    normalized === "localhost" ||
    normalized.startsWith("[") ||
    normalized.includes(".") ||
    normalized.includes(":")
  );
}

function parseAddressAndPort(value: string): { address: string; port: number } {
  const target = value.trim();
  if (target.startsWith("[")) {
    const closingBracket = target.indexOf("]");
    if (closingBracket < 0) throw new Error("Invalid bracketed IPv6 address");
    const address = target.slice(1, closingBracket);
    const remainder = target.slice(closingBracket + 1);
    const port = remainder.startsWith(":") ? Number(remainder.slice(1)) : 22;
    if (remainder && !remainder.startsWith(":")) throw new Error("Invalid SSH address");
    if (!address || !Number.isInteger(port) || port < 1 || port > 65535) {
      throw new Error("SSH port must be between 1 and 65535");
    }
    return { address, port };
  }

  const colonCount = [...target].filter((character) => character === ":").length;
  if (colonCount === 1) {
    const separator = target.lastIndexOf(":");
    const portText = target.slice(separator + 1);
    if (/^\d+$/.test(portText)) {
      const port = Number(portText);
      if (!Number.isInteger(port) || port < 1 || port > 65535) {
        throw new Error("SSH port must be between 1 and 65535");
      }
      return { address: target.slice(0, separator), port };
    }
  }
  return { address: target, port: 22 };
}

export function parseQuickConnectInput(input: string): QuickConnectTarget {
  let value = input.trim();
  if (!value) throw new Error("Enter a hostname or IP address");
  value = value.replace(/^ssh:\/\//i, "");

  let username = "ubuntu";
  let target = value;
  const separator = value.lastIndexOf("@");
  if (separator >= 0) {
    const left = value.slice(0, separator).trim();
    const right = value.slice(separator + 1).trim();
    if (!left || !right) throw new Error("Use username@hostname");
    if (looksLikeHost(left) && !looksLikeHost(right)) {
      target = left;
      username = right;
    } else {
      username = left;
      target = right;
    }
  }

  const { address, port } = parseAddressAndPort(target);
  if (!address || /\s/.test(address) || /\s/.test(username)) {
    throw new Error("Enter a valid SSH hostname and username");
  }
  return { address, username, port };
}
