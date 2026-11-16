// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ResolvedPromptSection } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  CUSTOMIZE_PERSONAL_LABEL,
  CUSTOMIZE_WORKSPACE_LABEL,
  EDIT_PERSONAL_LABEL,
  EDIT_WORKSPACE_LABEL,
  editLabelsFor,
  editorScopeLabel,
  editorSeedFor,
  hasPersonalOverride,
  hasWorkspaceOverride,
  mergePromptSections,
  PROMPT_KIND_LABELS,
  revertArmedFor,
  sourceBadgeFor,
  tierMarkFor,
  type MergedPromptSection,
} from "./sections.js";

function row(overrides: Partial<ResolvedPromptSection> = {}): ResolvedPromptSection {
  return {
    key: "autonomy",
    title: "Autonomy",
    customizable: "overridable",
    body: "effective body",
    default_body: "pristine default",
    source: "default",
    version: 1,
    needs_attention: false,
    editable_at_workspace: true,
    editable_at_personal: true,
    ...overrides,
  };
}

function merged(overrides: Partial<MergedPromptSection> = {}): MergedPromptSection {
  return {
    key: "autonomy",
    title: "Autonomy",
    kinds: ["coding-task"],
    user: row(),
    workspace: null,
    ...overrides,
  };
}

describe("PROMPT_KIND_LABELS", () => {
  it("labels the three browsed kinds", () => {
    expect(PROMPT_KIND_LABELS).toEqual({ "coding-task": "Coding task", review: "Review", scheduler: "Scheduler" });
  });
});

describe("mergePromptSections", () => {
  it("unions keys across kinds in first-seen order", () => {
    const sections = mergePromptSections(
      [
        { kind: "coding-task", sections: [row({ key: "aaa", title: "Aaa" }), row({ key: "bbb", title: "Bbb" })] },
        { kind: "review", sections: [row({ key: "bbb", title: "Bbb" }), row({ key: "ccc", title: "Ccc" })] },
        { kind: "scheduler", sections: [] },
      ],
      null
    );
    expect(sections.map((section) => section.key)).toEqual(["aaa", "bbb", "ccc"]);
    expect(sections[1]?.kinds).toEqual(["coding-task", "review"]);
    expect(sections[1]?.title).toBe("Bbb");
  });

  it("keeps the first row seen per scope", () => {
    const sections = mergePromptSections(
      [{ kind: "coding-task", sections: [row({ key: "aaa", body: "first" })] }],
      [{ kind: "coding-task", sections: [row({ key: "aaa", body: "baseline" })] }]
    );
    expect(sections[0]?.user?.body).toBe("first");
    expect(sections[0]?.workspace?.body).toBe("baseline");
  });

  it("adds baseline-only keys after the member keys", () => {
    const sections = mergePromptSections(
      [{ kind: "coding-task", sections: [row({ key: "aaa" })] }],
      [{ kind: "coding-task", sections: [row({ key: "zzz", title: "Zzz" })] }]
    );
    expect(sections.map((section) => section.key)).toEqual(["aaa", "zzz"]);
    expect(sections[1]?.user).toBeNull();
  });

  it("sorts each card's kinds into page order", () => {
    const sections = mergePromptSections(
      [
        { kind: "scheduler", sections: [row({ key: "aaa" })] },
        { kind: "coding-task", sections: [row({ key: "aaa" })] },
      ],
      null
    );
    expect(sections[0]?.kinds).toEqual(["coding-task", "scheduler"]);
  });

  it("merges an empty registry into no cards", () => {
    expect(mergePromptSections([], null)).toEqual([]);
  });
});

describe("sourceBadgeFor", () => {
  it("badges the default rung", () => {
    expect(sourceBadgeFor("default")).toBe("Pi Dash default");
  });

  it("badges the workspace rung", () => {
    expect(sourceBadgeFor("workspace")).toBe("Workspace override");
  });

  it("badges the personal rung", () => {
    expect(sourceBadgeFor("user:123e4567-e89b-12d3-a456-426614174000")).toBe("Your override");
  });

  it("falls back to the default badge for unknown rungs", () => {
    expect(sourceBadgeFor("archived")).toBe("Pi Dash default");
  });
});

describe("tierMarkFor", () => {
  it("marks locked sections", () => {
    expect(tierMarkFor("locked")).toBe("Locked");
  });

  it("marks admin-managed sections", () => {
    expect(tierMarkFor("workspace")).toBe("Admin-managed");
  });

  it("marks no overridable section", () => {
    expect(tierMarkFor("overridable")).toBeNull();
  });
});

describe("hasWorkspaceOverride / hasPersonalOverride", () => {
  it("reads the workspace rung off the baseline", () => {
    expect(hasWorkspaceOverride(merged({ workspace: row({ source: "workspace" }) }))).toBe(true);
    expect(hasWorkspaceOverride(merged({ workspace: row({ source: "default" }) }))).toBe(false);
    expect(hasWorkspaceOverride(merged({ workspace: null }))).toBe(false);
  });

  it("reads the personal rung off the effective row", () => {
    expect(hasPersonalOverride(merged({ user: row({ source: "user:abc" }) }))).toBe(true);
    expect(hasPersonalOverride(merged({ user: row({ source: "workspace" }) }))).toBe(false);
    expect(hasPersonalOverride(merged({ user: row({ source: "default" }) }))).toBe(false);
    expect(hasPersonalOverride(merged({ user: null }))).toBe(false);
  });
});

describe("editLabelsFor", () => {
  const admin = { isAdmin: true, canEdit: true, baselineLoaded: true };

  it("offers both scopes to the admin on an overridable section", () => {
    expect(editLabelsFor(merged(), admin)).toEqual({
      workspace: CUSTOMIZE_WORKSPACE_LABEL,
      personal: CUSTOMIZE_PERSONAL_LABEL,
    });
  });

  it("renames the controls once overrides exist", () => {
    const section = merged({
      user: row({ source: "user:abc" }),
      workspace: row({ source: "workspace" }),
    });
    expect(editLabelsFor(section, admin)).toEqual({
      workspace: EDIT_WORKSPACE_LABEL,
      personal: EDIT_PERSONAL_LABEL,
    });
  });

  it("hides workspace editing from non-admins", () => {
    expect(editLabelsFor(merged(), { isAdmin: false, canEdit: true, baselineLoaded: true })).toEqual({
      workspace: null,
      personal: CUSTOMIZE_PERSONAL_LABEL,
    });
  });

  it("hides workspace editing while the baseline is unknown", () => {
    expect(editLabelsFor(merged(), { ...admin, baselineLoaded: false })).toEqual({
      workspace: null,
      personal: CUSTOMIZE_PERSONAL_LABEL,
    });
  });

  it("hides personal editing from guests", () => {
    expect(editLabelsFor(merged(), { isAdmin: false, canEdit: false, baselineLoaded: false })).toEqual({
      workspace: null,
      personal: null,
    });
  });

  it("hides every control on locked sections", () => {
    const section = merged({
      user: row({ customizable: "locked", editable_at_workspace: false, editable_at_personal: false }),
    });
    expect(editLabelsFor(section, admin)).toEqual({ workspace: null, personal: null });
  });

  it("hides personal editing on admin-managed sections", () => {
    const section = merged({
      user: row({ customizable: "workspace", editable_at_workspace: true, editable_at_personal: false }),
    });
    expect(editLabelsFor(section, admin)).toEqual({ workspace: CUSTOMIZE_WORKSPACE_LABEL, personal: null });
  });

  it("offers nothing without an effective row", () => {
    expect(editLabelsFor(merged({ user: null, workspace: row() }), admin)).toEqual({
      workspace: null,
      personal: null,
    });
  });
});

describe("editorScopeLabel", () => {
  it("captions the workspace editor", () => {
    expect(editorScopeLabel("workspace")).toContain("Workspace default");
  });

  it("captions the personal editor", () => {
    expect(editorScopeLabel("user")).toContain("personal override");
  });
});

describe("editorSeedFor", () => {
  it("seeds the workspace draft from the baseline", () => {
    expect(editorSeedFor(merged({ workspace: row({ body: "baseline" }) }), "workspace")).toBe("baseline");
  });

  it("seeds an empty workspace draft without a baseline row", () => {
    expect(editorSeedFor(merged({ workspace: null }), "workspace")).toBe("");
  });

  it("seeds the personal draft from the effective body", () => {
    expect(editorSeedFor(merged({ user: row({ body: "effective" }) }), "user")).toBe("effective");
  });

  it("seeds an empty personal draft without an effective row", () => {
    expect(editorSeedFor(merged({ user: null }), "user")).toBe("");
  });
});

describe("revertArmedFor", () => {
  it("arms the workspace revert with a baseline override", () => {
    expect(revertArmedFor(merged({ workspace: row({ source: "workspace" }) }), "workspace")).toBe(true);
    expect(revertArmedFor(merged({ workspace: row({ source: "default" }) }), "workspace")).toBe(false);
  });

  it("arms the personal revert with a personal override only", () => {
    expect(revertArmedFor(merged({ user: row({ source: "user:abc" }) }), "user")).toBe(true);
    // A workspace override never arms the personal editor's revert.
    expect(revertArmedFor(merged({ user: row({ source: "workspace" }) }), "user")).toBe(false);
  });
});
