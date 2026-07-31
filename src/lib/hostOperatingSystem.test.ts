import { describe, expect, it } from "vitest";
import { detectHostOperatingSystem } from "./hostOperatingSystem";

describe("host operating system detection", () => {
  it("detects Ubuntu from its login banner", () => {
    expect(detectHostOperatingSystem("Welcome to Ubuntu 24.04.2 LTS (GNU/Linux 6.8.0)")).toBe("ubuntu");
  });

  it("recognizes common Linux distributions", () => {
    expect(detectHostOperatingSystem("Alpine Linux 3.21")).toBe("alpine");
    expect(detectHostOperatingSystem("Rocky Linux release 9.5")).toBe("redhat");
    expect(detectHostOperatingSystem("Arch Linux")).toBe("arch");
  });
});
