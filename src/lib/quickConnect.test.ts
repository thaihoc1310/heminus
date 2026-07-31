import { describe, expect, it } from "vitest";
import { parseQuickConnectInput } from "./quickConnect";

describe("parseQuickConnectInput", () => {
  it("parses the standard username at hostname form", () => {
    expect(parseQuickConnectInput("root@server.example.com:2221")).toEqual({
      address: "server.example.com",
      username: "root",
      port: 2221
    });
  });

  it("accepts the address at username form shown by Termius", () => {
    expect(parseQuickConnectInput("172.30.163.72@ubuntu")).toEqual({
      address: "172.30.163.72",
      username: "ubuntu",
      port: 22
    });
  });

  it("supports bracketed IPv6 targets", () => {
    expect(parseQuickConnectInput("deploy@[2001:db8::10]:2200")).toEqual({
      address: "2001:db8::10",
      username: "deploy",
      port: 2200
    });
  });
});
