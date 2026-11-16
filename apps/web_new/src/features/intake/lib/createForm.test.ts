// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake create-form rules (parity/specs/intake.md INT-013–015): the
// create defaults, the over-long-only title rule, and the payload each
// save posts.

import { describe, expect, it } from "vitest";

import {
  buildIntakeCreatePayload,
  emptyIntakeCreateForm,
  INTAKE_TITLE_MAX_LENGTH,
  intakeTitleError,
  isIntakeTitleSubmittable,
  normalizeIntakeTitle,
  resetIntakeCreateForm,
  visibleIntakeDescriptionHtml,
  type IntakeCreateForm,
} from "./createForm.js";

function filled(): IntakeCreateForm {
  return {
    name: "  Triage me  ",
    descriptionHtml: "<p>Details</p>",
    stateId: "state-1",
    priority: "high",
    assigneeIds: ["user-1", "user-2"],
    labelIds: ["label-1"],
    targetDate: "2026-11-01",
  };
}

describe("intake create defaults (INT-013)", () => {
  it("starts blank with no committed state, no priority and no due date", () => {
    expect(emptyIntakeCreateForm()).toEqual({
      name: "",
      descriptionHtml: "",
      stateId: null,
      priority: "none",
      assigneeIds: [],
      labelIds: [],
      targetDate: null,
    });
  });

  it("seeds the editor's default content when given one", () => {
    expect(emptyIntakeCreateForm("<p>seed</p>").descriptionHtml).toBe("<p>seed</p>");
  });

  it("resets every field to the create defaults for the next request", () => {
    expect(resetIntakeCreateForm()).toEqual(emptyIntakeCreateForm());
    expect(resetIntakeCreateForm("<p>seed</p>")).toEqual(emptyIntakeCreateForm("<p>seed</p>"));
  });
});

describe("intake title rules (INT-014)", () => {
  it("trims the title and folds whitespace-only to empty", () => {
    expect(normalizeIntakeTitle("  Triage me  ")).toBe("Triage me");
    expect(normalizeIntakeTitle("   ")).toBe("");
  });

  it("caps the title at the server's length", () => {
    expect(INTAKE_TITLE_MAX_LENGTH).toBe(255);
  });

  it("flags only an over-long title and stays quiet otherwise", () => {
    expect(intakeTitleError("x".repeat(256))).toBe("Titles stop at 255 characters.");
    expect(intakeTitleError("x".repeat(255))).toBeNull();
    expect(intakeTitleError("Triage me")).toBeNull();
    expect(intakeTitleError("")).toBeNull();
    expect(intakeTitleError("   ")).toBeNull();
  });

  it("lets every title through except an over-long one", () => {
    expect(isIntakeTitleSubmittable("x".repeat(256))).toBe(false);
    expect(isIntakeTitleSubmittable("x".repeat(255))).toBe(true);
    expect(isIntakeTitleSubmittable("Triage me")).toBe(true);
    // Empty titles reach the server, which refuses them (INT-014).
    expect(isIntakeTitleSubmittable("")).toBe(true);
    expect(isIntakeTitleSubmittable("   ")).toBe(true);
  });
});

describe("visibleIntakeDescriptionHtml (INT-013)", () => {
  it("keeps stored content and drops the mount-time empty paragraph", () => {
    expect(visibleIntakeDescriptionHtml("<p>Details</p>")).toBe("<p>Details</p>");
    expect(visibleIntakeDescriptionHtml("<p></p>")).toBe("");
    expect(visibleIntakeDescriptionHtml("")).toBe("");
  });
});

describe("buildIntakeCreatePayload (INT-013)", () => {
  it("sends the trimmed title, the shown state and every set property", () => {
    expect(buildIntakeCreatePayload(filled())).toEqual({
      source: "IN_APP",
      issue: {
        name: "Triage me",
        description_html: "<p>Details</p>",
        state_id: "state-1",
        priority: "high",
        assignee_ids: ["user-1", "user-2"],
        label_ids: ["label-1"],
        target_date: "2026-11-01",
      },
    });
  });

  it("sends an empty state while unset and omits every untouched property", () => {
    expect(buildIntakeCreatePayload(emptyIntakeCreateForm())).toEqual({
      source: "IN_APP",
      issue: { name: "", state_id: "" },
    });
  });

  it("still posts an empty title for the server to refuse", () => {
    const payload = buildIntakeCreatePayload({ ...filled(), name: "   " });
    expect(payload.issue.name).toBe("");
  });

  it("omits an empty description rather than sending the mount paragraph", () => {
    const payload = buildIntakeCreatePayload({ ...filled(), descriptionHtml: "<p></p>" });
    expect(payload.issue.description_html).toBeUndefined();
  });

  it("copies the id lists so later form edits cannot rewrite the payload", () => {
    const form = filled();
    const payload = buildIntakeCreatePayload(form);
    form.assigneeIds.push("user-3");
    form.labelIds.push("label-2");
    expect(payload.issue.assignee_ids).toEqual(["user-1", "user-2"]);
    expect(payload.issue.label_ids).toEqual(["label-1"]);
  });
});
