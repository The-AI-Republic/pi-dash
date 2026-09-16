import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { desktopWebUrl } from "../../core/utils/desktop-web-url";

describe("desktopWebUrl", () => {
  it("resolves an issue path against the configured hosted frontend", () => {
    expect(desktopWebUrl("/ai-republic/projects/project-id/issues/issue-id", "https://pidash.airepublic.com")).toBe(
      "https://pidash.airepublic.com/ai-republic/projects/project-id/issues/issue-id"
    );
  });

  it("preserves query parameters and fragments", () => {
    expect(desktopWebUrl("/workspace/issues/issue-id?peek=activity#comment", "https://pidash.example/")).toBe(
      "https://pidash.example/workspace/issues/issue-id?peek=activity#comment"
    );
  });

  it("rejects a missing hosted frontend origin instead of copying a Tauri URL", () => {
    expect(() => desktopWebUrl("/workspace/issues/issue-id", "  ")).toThrow("VITE_WEB_BASE_URL is required");
  });

  it("rejects a Tauri origin even when one is configured accidentally", () => {
    expect(() => desktopWebUrl("/workspace/issues/issue-id", "tauri://localhost")).toThrow("must use http or https");
  });

  it.each([
    "../../core/components/issues/issue-detail/issue-detail-quick-actions.tsx",
    "../../core/components/issues/peek-overview/header.tsx",
    "../../core/components/issues/issue-layouts/quick-action-dropdowns/helper.tsx",
  ])("routes the desktop copy-link call site in %s through the hosted origin", (relativePath) => {
    const source = readFileSync(new URL(relativePath, import.meta.url), "utf8");
    expect(source).toContain("desktopWebUrl(workItemLink)");
  });
});
