// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Receipt presentation model (parity/specs/agents.md §7, AGT-028-031).
import type { CompiledPrompt, ResolvedPromptSection } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  automaticTemplate,
  draftPreviewKinds,
  initialDraftKind,
  previewTargetLabel,
  previewTargetMode,
  previewTargetPayload,
  previewTargetPlaceholder,
  receiptAnchorId,
  receiptSlots,
  sectionCountLabel,
} from "./receipt.js";

function section(overrides: Partial<ResolvedPromptSection> = {}): ResolvedPromptSection {
  return {
    key: "autonomy",
    title: "Autonomy",
    customizable: "overridable",
    body: "body",
    default_body: "default",
    source: "default",
    version: 1,
    needs_attention: false,
    editable_at_workspace: true,
    editable_at_personal: true,
    ...overrides,
  };
}

function compiled(overrides: Partial<CompiledPrompt> = {}): CompiledPrompt {
  return { kind: "coding-task", scope: "user", template_body: "personal", ...overrides };
}

describe("sectionCountLabel", () => {
  it("renders singular, plural and zero counts rather than a template", () => {
    expect(sectionCountLabel(0)).toBe("0 sections");
    expect(sectionCountLabel(1)).toBe("1 section");
    expect(sectionCountLabel(4)).toBe("4 sections");
    expect(sectionCountLabel(4)).not.toContain("{{");
  });
});

describe("receiptSlots", () => {
  it("numbers the composition from one with zero padding and keeps order", () => {
    const slots = receiptSlots([
      section({ key: "a", title: "A", source: "default" }),
      section({ key: "b", title: "B", source: "user:123" }),
      ...Array.from({ length: 9 }, (_, index) => section({ key: `k${index}`, title: `K${index}` })),
    ]);
    expect(slots[0]).toEqual({ num: "01", key: "a", title: "A", source: "default" });
    expect(slots[1]).toEqual({ num: "02", key: "b", title: "B", source: "user:123" });
    expect(slots.at(-1)?.num).toBe("11");
  });

  it("returns an empty composition for an empty list", () => {
    expect(receiptSlots([])).toEqual([]);
  });
});

describe("automaticTemplate", () => {
  it("returns the workspace-only variant when the API sends one", () => {
    expect(automaticTemplate(compiled({ automatic_template_body: "automatic" }))).toBe("automatic");
  });

  it("hides the variant when absent", () => {
    expect(automaticTemplate(compiled())).toBeNull();
  });

  it("hides the variant when it repeats the personal assembly", () => {
    expect(automaticTemplate(compiled({ automatic_template_body: "personal" }))).toBeNull();
  });
});

describe("preview targets", () => {
  it("collects an issue id for the coding and review kinds", () => {
    expect(previewTargetMode("coding-task")).toBe("issue");
    expect(previewTargetMode("review")).toBe("issue");
    expect(previewTargetPayload("review", "issue-1")).toEqual({ issue_id: "issue-1" });
    expect(previewTargetLabel("coding-task")).toBe("Issue id");
    expect(previewTargetPlaceholder("coding-task")).toBe("Issue id (UUID)");
  });

  it("collects a scheduler install id for the scheduler kind", () => {
    expect(previewTargetMode("scheduler")).toBe("install");
    expect(previewTargetPayload("scheduler", "binding-1")).toEqual({ binding_id: "binding-1" });
    expect(previewTargetLabel("scheduler")).toBe("Scheduler install id");
    expect(previewTargetPlaceholder("scheduler")).toBe("Scheduler install id (UUID)");
  });

  it("adds the draft scope, section key and body for unsaved previews", () => {
    expect(
      previewTargetPayload("coding-task", "issue-1", { scope: "user", sectionKey: "autonomy", body: "draft" })
    ).toEqual({ issue_id: "issue-1", scope: "user", section_key: "autonomy", body: "draft" });
    expect(previewTargetPayload("scheduler", "binding-1", { scope: "workspace", sectionKey: "k", body: "d" })).toEqual({
      binding_id: "binding-1",
      scope: "workspace",
      section_key: "k",
      body: "d",
    });
  });
});

describe("draft preview kinds", () => {
  it("offers a switcher only when several kinds carry the section", () => {
    expect(draftPreviewKinds(["coding-task"])).toEqual([]);
    expect(draftPreviewKinds([])).toEqual([]);
    expect(draftPreviewKinds(["coding-task", "review"])).toEqual(["coding-task", "review"]);
  });

  it("opens on the first kind, or coding-task when there is none", () => {
    expect(initialDraftKind(["review", "scheduler"])).toBe("review");
    expect(initialDraftKind([])).toBe("coding-task");
  });
});

describe("receiptAnchorId", () => {
  it("anchors each kind on its own id", () => {
    expect(receiptAnchorId("coding-task")).toBe("prompt-receipt-coding-task");
    expect(receiptAnchorId("scheduler")).toBe("prompt-receipt-scheduler");
  });
});
