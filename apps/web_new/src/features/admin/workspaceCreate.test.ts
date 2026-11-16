// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  buildWorkspaceCreateInput,
  deriveWorkspaceSlug,
  formatWorkspaceSlugPrefix,
  isRestrictedWorkspaceSlug,
  isWorkspaceCreateSubmittable,
  ORGANIZATION_SIZE_BANDS,
  shouldShowInvalidSlugHint,
  validateWorkspaceCreateName,
  validateWorkspaceCreateSlug,
  WORKSPACE_CREATE_BUSY_LABEL,
  WORKSPACE_CREATE_FAILURE_TITLE,
  WORKSPACE_CREATE_INVALID_SLUG_HINT,
  WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SIZE_REQUIRED_MESSAGE,
  WORKSPACE_CREATE_SUBMIT_LABEL,
  WORKSPACE_CREATE_SUCCESS_MESSAGE,
  WORKSPACE_CREATE_SUCCESS_TITLE,
  WORKSPACE_CREATE_TAKEN_MESSAGE,
  WORKSPACE_NAME_MAX_LENGTH,
  WORKSPACE_SLUG_MAX_LENGTH,
  WorkspaceSlugProbeError,
  WorkspaceSlugTakenError,
} from "./workspaceCreate.js";

describe("create-workspace helpers", () => {
  it("pins the oracle submit, busy, and notice copy", () => {
    expect(WORKSPACE_CREATE_SUBMIT_LABEL).toBe("Create workspace");
    expect(WORKSPACE_CREATE_BUSY_LABEL).toBe("Creating workspace");
    expect(WORKSPACE_CREATE_SUCCESS_TITLE).toBe("Success!");
    expect(WORKSPACE_CREATE_SUCCESS_MESSAGE).toContain("Workspace created successfully");
    expect(WORKSPACE_CREATE_FAILURE_TITLE).toBe("Error!");
    expect(WORKSPACE_CREATE_TAKEN_MESSAGE).not.toBe("");
    expect(WORKSPACE_CREATE_INVALID_SLUG_HINT).not.toBe("");
    expect(WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE).not.toBe("");
    expect(WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE).not.toBe("");
    expect(WORKSPACE_CREATE_SIZE_REQUIRED_MESSAGE).not.toBe("");
  });

  it("fixes the size bands and length caps", () => {
    expect(ORGANIZATION_SIZE_BANDS).toEqual(["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"]);
    expect(WORKSPACE_NAME_MAX_LENGTH).toBe(80);
    expect(WORKSPACE_SLUG_MAX_LENGTH).toBe(48);
  });

  it("derives the slug lowercased, trimmed, spaces to hyphens", () => {
    expect(deriveWorkspaceSlug("My Cool Workspace")).toBe("my-cool-workspace");
    expect(deriveWorkspaceSlug("  Padded  Name  ")).toBe("padded--name");
    expect(deriveWorkspaceSlug("UPPER")).toBe("upper");
    expect(deriveWorkspaceSlug("")).toBe("");
  });

  it("flags restricted slugs exactly", () => {
    expect(isRestrictedWorkspaceSlug("god-mode")).toBe(true);
    expect(isRestrictedWorkspaceSlug("api")).toBe(true);
    expect(isRestrictedWorkspaceSlug("admin")).toBe(true);
    expect(isRestrictedWorkspaceSlug("my-cool-workspace")).toBe(false);
    expect(isRestrictedWorkspaceSlug("")).toBe(false);
  });

  it("shows the charset hint only for non-empty slugs outside the keystroke set", () => {
    expect(shouldShowInvalidSlugHint("")).toBe(false);
    expect(shouldShowInvalidSlugHint("good-slug-1")).toBe(false);
    expect(shouldShowInvalidSlugHint("UPPER-SLUG")).toBe(false);
    expect(shouldShowInvalidSlugHint("bad slug!")).toBe(true);
    expect(shouldShowInvalidSlugHint("caf\u00e9")).toBe(true);
  });

  it("validates the name: required, capped, charset", () => {
    expect(validateWorkspaceCreateName("")).not.toBe(null);
    expect(validateWorkspaceCreateName("   ")).not.toBe(null);
    expect(validateWorkspaceCreateName("a".repeat(81))).toContain("80");
    expect(validateWorkspaceCreateName("a".repeat(80))).toBe(null);
    const injection = validateWorkspaceCreateName("Bad{Name}");
    expect(injection).toContain("Workspace name");
    const charset = validateWorkspaceCreateName("Bad@Name");
    expect(charset).toContain("Workspace name");
    expect(validateWorkspaceCreateName("Good Name")).toBe(null);
    expect(validateWorkspaceCreateName("caf\u00e9_Naam-1")).toBe(null);
  });

  it("validates the slug: required, capped, charset", () => {
    expect(validateWorkspaceCreateSlug("")).not.toBe(null);
    expect(validateWorkspaceCreateSlug("   ")).not.toBe(null);
    expect(validateWorkspaceCreateSlug("a".repeat(49))).toContain("48");
    expect(validateWorkspaceCreateSlug("a".repeat(48))).toBe(null);
    expect(validateWorkspaceCreateSlug("bad{slug}")).toContain("Slug");
    expect(validateWorkspaceCreateSlug("bad slug!")).toContain("Slug");
    expect(validateWorkspaceCreateSlug("bad@slug")).toBe("Slug allows letters, numbers, hyphens and underscores.");
    expect(validateWorkspaceCreateSlug("good-slug-1")).toBe(null);
  });

  it("gates submit on a valid name, slug, and picked band", () => {
    expect(isWorkspaceCreateSubmittable("Good Name", "good-slug-1", "2-10")).toBe(true);
    expect(isWorkspaceCreateSubmittable("Bad{Name}", "good-slug-1", "2-10")).toBe(false);
    expect(isWorkspaceCreateSubmittable("Good Name", "bad slug!", "2-10")).toBe(false);
    expect(isWorkspaceCreateSubmittable("Good Name", "good-slug-1", "")).toBe(false);
    expect(isWorkspaceCreateSubmittable("Good Name", "good-slug-1", "7-11")).toBe(false);
    expect(isWorkspaceCreateSubmittable("", "", "")).toBe(false);
  });

  it("builds the create body with no owner field", () => {
    const body = buildWorkspaceCreateInput("Good Name", "good-slug-1", "11-50");
    expect(body).toEqual({ name: "Good Name", slug: "good-slug-1", organization_size: "11-50" });
    expect("owner" in body).toBe(false);
  });

  it("formats the slug prefix as the origin plus one slash", () => {
    expect(formatWorkspaceSlugPrefix("https://app.example")).toBe("https://app.example/");
    expect(formatWorkspaceSlugPrefix("https://app.example///")).toBe("https://app.example/");
    expect(formatWorkspaceSlugPrefix("http://localhost")).toBe("http://localhost/");
  });

  it("names the taken and probe errors after the slug", () => {
    const taken = new WorkspaceSlugTakenError("taken-slug");
    expect(taken).toBeInstanceOf(Error);
    expect(taken.name).toBe("WorkspaceSlugTakenError");
    expect(taken.message).toContain("taken-slug");
    const probe = new WorkspaceSlugProbeError("probe-slug");
    expect(probe).toBeInstanceOf(Error);
    expect(probe.name).toBe("WorkspaceSlugProbeError");
    expect(probe.message).toContain("probe-slug");
  });
});
