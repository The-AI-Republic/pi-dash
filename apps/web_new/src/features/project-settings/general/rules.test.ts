// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General-tab rules (parity/specs/project-settings-general.md §PSET-005–009):
// prefill, the short-code and budget shapers, client validation, the write
// payload, conflict parsing, emblem values and the executor/bound states.

import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import type { ProjectSettingsDetail } from "@pidash/api-client";
import {
  BASE_BRANCH_CHARS_MESSAGE,
  BASE_BRANCH_TOO_LONG_MESSAGE,
  budgetOnSave,
  buildGeneralPayload,
  cadenceSeconds,
  canonicalRepositoryUrl,
  cadenceLabel,
  CADENCE_OPTIONS,
  cappedBudgetOf,
  CUSTOM_CADENCE_LABEL,
  CUSTOM_CADENCE_VALUE,
  DEFAULT_MAX_TICKS,
  EMBLEM_MAX_BYTES,
  EMBLEM_SIZE_MESSAGE,
  EMBLEM_TYPE_MESSAGE,
  emblemFileError,
  executorChoices,
  generalConflictError,
  generalFormFrom,
  IDENTIFIER_REQUIRED_MESSAGE,
  IDENTIFIER_TAKEN_CODE,
  IDENTIFIER_TAKEN_MESSAGE,
  IDENTIFIER_TOO_LONG_MESSAGE,
  IDENTIFIER_UNAVAILABLE_MESSAGE,
  identifierUnavailable,
  INFINITE_MAX_TICKS,
  markEmblem,
  unifiedEmoji,
  NAME_REQUIRED_MESSAGE,
  NAME_TAKEN_CODE,
  NAME_TAKEN_MESSAGE,
  NAME_TOO_LONG_MESSAGE,
  repositoryBoundState,
  saveFailureMessage,
  selectCadence,
  shapeBudget,
  shapeIdentifier,
  uploadedEmblem,
  validateGeneralFields,
  type GeneralFormValues,
} from "./rules.js";

const STORED: ProjectSettingsDetail = {
  id: "223e4567-e89b-12d3-a456-426614174001",
  created_at: "2026-01-02T03:04:05Z",
  updated_at: "2026-02-03T04:05:06Z",
  created_by: null,
  updated_by: null,
  deleted_at: null,
  workspace: "123e4567-e89b-12d3-a456-426614174000",
  name: "Website relaunch",
  description: "Public site work",
  description_text: null,
  description_html: null,
  network: 2,
  identifier: "WEB",
  default_assignee: null,
  project_lead: null,
  emoji: null,
  icon_prop: null,
  module_view: true,
  cycle_view: false,
  issue_views_view: true,
  page_view: false,
  intake_view: true,
  inbox_view: false,
  is_time_tracking_enabled: false,
  is_issue_type_enabled: false,
  is_default: false,
  guest_view_all_features: false,
  members_can_edit_states: false,
  cover_image: null,
  cover_image_asset: null,
  cover_image_url: null,
  estimate: null,
  archive_in: 0,
  close_in: 0,
  logo_props: { in_use: "emoji", emoji: { value: "🌱" } },
  default_state: null,
  archived_at: null,
  timezone: "Europe/Lisbon",
  external_source: null,
  external_id: null,
  repo_url: "https://github.com/acme/site",
  base_branch: "develop",
  is_favorite: false,
  sort_order: null,
  member_role: null,
  anchor: null,
  members: [],
  next_work_item_sequence: 1,
  agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
  default_agent_executor: "cloud_agent",
  agent_ticking_enabled: true,
  agent_default_max_ticks: 7,
  agent_default_interval_seconds: 21_600,
  agent_review_default_interval_seconds: 21_600,
  agent_test_default_interval_seconds: 21_600,
};

function values(overrides: Partial<GeneralFormValues> = {}): GeneralFormValues {
  return { ...generalFormFrom(STORED), ...overrides };
}

describe("generalFormFrom", () => {
  it("prefills every editable field from the stored row", () => {
    const form = generalFormFrom(STORED);
    expect(form).toMatchObject({
      name: "Website relaunch",
      identifier: "WEB",
      description: "Public site work",
      network: 2,
      timezone: "Europe/Lisbon",
      baseBranch: "develop",
      repoUrl: "https://github.com/acme/site",
      executor: "cloud_agent",
      ticking: true,
      budget: "7",
      noCap: false,
      lastCappedBudget: "7",
      cadence: "21600",
    });
    expect(form.logoProps).toEqual(STORED.logo_props);
  });

  it("reads an uncapped row as blank with no remembered cap", () => {
    const form = generalFormFrom({ ...STORED, agent_default_max_ticks: -1, agent_default_interval_seconds: 999 });
    expect(form.budget).toBe("");
    expect(form.noCap).toBe(true);
    expect(form.lastCappedBudget).toBe(String(DEFAULT_MAX_TICKS));
    expect(form.cadence).toBe(CUSTOM_CADENCE_VALUE);
  });

  it("normalizes visibility, an empty executor and a custom cadence", () => {
    const form = generalFormFrom({
      ...STORED,
      network: 5,
      default_agent_executor: "",
      agent_default_interval_seconds: 14_400,
    });
    expect(form.network).toBe(0);
    expect(form.executor).toBe("local_runner");
    expect(form.cadence).toBe(CUSTOM_CADENCE_VALUE);
  });
});

describe("cappedBudgetOf", () => {
  it("keeps a real cap and treats a zero or sentinel as uncapped", () => {
    expect(cappedBudgetOf({ agent_default_max_ticks: 3 })).toBe(3);
    expect(cappedBudgetOf({ agent_default_max_ticks: 0 })).toBeNull();
    expect(cappedBudgetOf({ agent_default_max_ticks: INFINITE_MAX_TICKS })).toBeNull();
  });
});

describe("cadence", () => {
  it("selects a preset only for its exact stored seconds", () => {
    for (const option of CADENCE_OPTIONS) expect(selectCadence(option.seconds)).toBe(String(option.seconds));
    expect(selectCadence(5_400)).toBe(CUSTOM_CADENCE_VALUE);
  });

  it("labels a preset and marks anything else custom", () => {
    expect(cadenceLabel("21600")).toBe("6 hours");
    expect(cadenceLabel(CUSTOM_CADENCE_VALUE)).toBe(CUSTOM_CADENCE_LABEL);
    expect(cadenceLabel("5400")).toBe(CUSTOM_CADENCE_LABEL);
  });
});

describe("shapeIdentifier", () => {
  it("drops separators, uppercases and caps the length", () => {
    expect(shapeIdentifier("ab-12_cd")).toBe("AB12CD");
    expect(shapeIdentifier("abcdefghijkl")).toBe("ABCDEFGHIJ");
  });

  it("keeps non-Latin letters and digits", () => {
    expect(shapeIdentifier("проект-1")).toBe("ПРОЕКТ1");
  });
});

describe("shapeBudget", () => {
  it("keeps digits only and leaves an empty field empty", () => {
    expect(shapeBudget("12a3.5")).toBe("1235");
    expect(shapeBudget("")).toBe("");
  });
});

describe("validateGeneralFields", () => {
  it("accepts the prefilled form", () => {
    expect(validateGeneralFields(values())).toEqual({});
  });

  it("requires a name and an identifier", () => {
    expect(validateGeneralFields(values({ name: "   ", identifier: " " }))).toEqual({
      name: NAME_REQUIRED_MESSAGE,
      identifier: IDENTIFIER_REQUIRED_MESSAGE,
    });
  });

  it("caps the name and the short code", () => {
    expect(validateGeneralFields(values({ name: "n".repeat(256) }))).toEqual({ name: NAME_TOO_LONG_MESSAGE });
    expect(validateGeneralFields(values({ identifier: "ABCDEFGHIJK" }))).toEqual({
      identifier: IDENTIFIER_TOO_LONG_MESSAGE,
    });
  });

  it("allows a blank base branch, caps it and restricts its characters", () => {
    expect(validateGeneralFields(values({ baseBranch: "" }))).toEqual({});
    expect(validateGeneralFields(values({ baseBranch: "a".repeat(129) }))).toEqual({
      baseBranch: BASE_BRANCH_TOO_LONG_MESSAGE,
    });
    expect(validateGeneralFields(values({ baseBranch: "feature branch" }))).toEqual({
      baseBranch: BASE_BRANCH_CHARS_MESSAGE,
    });
    expect(validateGeneralFields(values({ baseBranch: "release/2.0_hotfix-1" }))).toEqual({});
  });

  it("requires a whole budget of at least one while ticking", () => {
    expect(validateGeneralFields(values({ budget: "" }))).toEqual({ budget: expect.any(String) });
    expect(validateGeneralFields(values({ budget: "0" }))).toEqual({ budget: expect.any(String) });
    expect(validateGeneralFields(values({ budget: "1" }))).toEqual({});
  });

  it("validates nothing about the policy while automated runs are off", () => {
    expect(validateGeneralFields(values({ ticking: false, budget: "", noCap: false }))).toEqual({});
    expect(validateGeneralFields(values({ noCap: true, budget: "" }))).toEqual({});
  });
});

describe("budgetOnSave", () => {
  it("writes the sentinel for no cap, the typed cap, or the default", () => {
    expect(budgetOnSave(values({ noCap: true }))).toBe(INFINITE_MAX_TICKS);
    expect(budgetOnSave(values({ budget: "12" }))).toBe(12);
    expect(budgetOnSave(values({ budget: "" }))).toBe(DEFAULT_MAX_TICKS);
    expect(budgetOnSave(values({ budget: "0" }))).toBe(1);
  });
});

describe("buildGeneralPayload", () => {
  it("carries the editable fields but never the repository URL", () => {
    const payload = buildGeneralPayload({ values: values({ name: "New name" }), cadenceDirty: false });
    expect(payload).toEqual({
      name: "New name",
      identifier: "WEB",
      description: "Public site work",
      network: 2,
      timezone: "Europe/Lisbon",
      base_branch: "develop",
      logo_props: STORED.logo_props,
      default_agent_executor: "cloud_agent",
      agent_ticking_enabled: true,
      agent_default_max_ticks: 7,
    });
    expect("repo_url" in payload).toBe(false);
  });

  it("leaves per-stage cadence out unless the picker moved", () => {
    const clean = buildGeneralPayload({ values: values(), cadenceDirty: false });
    expect(clean.agent_default_interval_seconds).toBeUndefined();
    expect(clean.agent_review_default_interval_seconds).toBeUndefined();
    expect(clean.agent_test_default_interval_seconds).toBeUndefined();
  });

  it("writes a picked cadence to every ticking stage", () => {
    const payload = buildGeneralPayload({ values: values({ cadence: "1800" }), cadenceDirty: true });
    expect(payload.agent_default_interval_seconds).toBe(1_800);
    expect(payload.agent_review_default_interval_seconds).toBe(1_800);
    expect(payload.agent_test_default_interval_seconds).toBe(1_800);
  });

  it("writes no stage columns for the custom marker, and reads presets as seconds", () => {
    const payload = buildGeneralPayload({ values: values({ cadence: CUSTOM_CADENCE_VALUE }), cadenceDirty: true });
    expect("agent_default_interval_seconds" in payload).toBe(false);
    expect("agent_review_default_interval_seconds" in payload).toBe(false);
    expect("agent_test_default_interval_seconds" in payload).toBe(false);
    expect(cadenceSeconds("21600")).toBe(21_600);
    expect(cadenceSeconds(CUSTOM_CADENCE_VALUE)).toBeNull();
    expect(cadenceSeconds("not-a-number")).toBeNull();
    expect(cadenceSeconds("0")).toBeNull();
  });

  it("writes the no-cap sentinel and the switched-off ticking flag", () => {
    const payload = buildGeneralPayload({
      values: values({ noCap: true, budget: "", ticking: false }),
      cadenceDirty: false,
    });
    expect(payload.agent_default_max_ticks).toBe(INFINITE_MAX_TICKS);
    expect(payload.agent_ticking_enabled).toBe(false);
  });
});

describe("identifierUnavailable", () => {
  it("turns a hit into the field error and a free name into none", () => {
    expect(identifierUnavailable({ exists: 2 })).toBe(IDENTIFIER_UNAVAILABLE_MESSAGE);
    expect(identifierUnavailable({ exists: 0 })).toBeNull();
  });
});

describe("saveFailureMessage", () => {
  it("reads an Error's message and stringifies anything else", () => {
    expect(saveFailureMessage(new Error("Request failed"))).toBe("Request failed");
    expect(saveFailureMessage("offline")).toBe("offline");
    expect(saveFailureMessage(undefined)).toBe("undefined");
  });
});

describe("generalConflictError", () => {
  function conflict(fields: Record<string, string[]>): ApiError {
    return new ApiError({ status: 400, code: "validation", message: "invalid", fields });
  }

  it("maps each distinct taken code onto its own field", () => {
    expect(generalConflictError(conflict({ name: [NAME_TAKEN_CODE] }))).toEqual({
      field: "name",
      message: NAME_TAKEN_MESSAGE,
    });
    expect(generalConflictError(conflict({ identifier: [IDENTIFIER_TAKEN_CODE] }))).toEqual({
      field: "identifier",
      message: IDENTIFIER_TAKEN_MESSAGE,
    });
  });

  it("ignores other codes, foreign fields and non-API errors", () => {
    expect(generalConflictError(conflict({ name: ["REQUIRED"] }))).toBeNull();
    expect(generalConflictError(conflict({ identifier: ["REQUIRED"] }))).toBeNull();
    expect(generalConflictError(conflict({}))).toBeNull();
    expect(generalConflictError(new Error("boom"))).toBeNull();
    expect(generalConflictError(undefined)).toBeNull();
  });
});

describe("repositoryBoundState", () => {
  it("reads bound only on a bound status with an exact trimmed match", () => {
    const status = {
      bound: true as const,
      repo_url: "https://github.com/acme/site",
    };
    expect(repositoryBoundState({ ...status, bound: false }, status.repo_url)).toBe("bind");
    expect(repositoryBoundState(status as never, `  ${status.repo_url} `)).toBe("bound");
    expect(repositoryBoundState(status as never, "https://github.com/acme/other")).toBe("bind");
    expect(repositoryBoundState(status as never, "  ")).toBe("bind");
    expect(repositoryBoundState(status as never, "")).toBe("bind");
    expect(repositoryBoundState(undefined, status.repo_url)).toBe("bind");
    expect(repositoryBoundState({ bound: true } as never, "")).toBe("bind");
    expect(repositoryBoundState({ bound: true } as never, "https://github.com/acme/site")).toBe("bind");
    expect(repositoryBoundState({ bound: true, repo_url: "" } as never, "https://github.com/acme/site")).toBe("bind");
  });

  it("compares a status row against the stored clone URL, not the web URL", () => {
    // The real status GET carries no top-level repo_url; the project stores
    // the canonical clone URL, which keeps the `.git` suffix the web URL drops.
    const status = {
      bound: true as const,
      repository: {
        web_url: "https://github.com/acme/site",
        clone_url_http: "https://github.com/acme/site.git",
      },
    };
    expect(canonicalRepositoryUrl(status)).toBe("https://github.com/acme/site.git");
    expect(repositoryBoundState(status, "https://github.com/acme/site.git")).toBe("bound");
    expect(repositoryBoundState(status, " https://github.com/acme/site.git ")).toBe("bound");
    expect(repositoryBoundState(status, "https://github.com/acme/site")).toBe("bind");
    expect(repositoryBoundState(status, "https://github.com/acme/other")).toBe("bind");
    expect(canonicalRepositoryUrl(undefined)).toBeUndefined();
    expect(canonicalRepositoryUrl({ repository: {} })).toBeUndefined();
  });

  it("falls back to the web URL only when neither canonical shape is present", () => {
    expect(canonicalRepositoryUrl({ repository: { web_url: "https://github.com/acme/site" } })).toBe(
      "https://github.com/acme/site"
    );
    expect(
      canonicalRepositoryUrl({ repo_url: "  ", repository: { clone_url_http: "https://git.test/acme/site.git" } })
    ).toBe("https://git.test/acme/site.git");
    expect(
      canonicalRepositoryUrl({
        repo_url: "https://github.com/acme/site.git",
        repository: { clone_url_http: "https://git.test/acme/site.git" },
      })
    ).toBe("https://github.com/acme/site.git");
  });
});

describe("executorChoices", () => {
  it("disables only families the instance reports unavailable", () => {
    expect(executorChoices([{ kind: "cloud_agent", available: true }])).toEqual([
      { kind: "local_runner", label: "Local runner", disabled: false },
      { kind: "cloud_agent", label: "Cloud agent", disabled: false },
      { kind: "managed_runner", label: "Desktop agent", disabled: true },
    ]);
  });

  it("keeps the local runner enabled even when the options disagree", () => {
    expect(executorChoices([{ kind: "local_runner", available: false }])[0]?.disabled).toBe(false);
  });
});

describe("emblem values", () => {
  it("writes an emoji mark as the stored value record of decimal code points", () => {
    expect(markEmblem("emoji", "🌊")).toEqual({ in_use: "emoji", emoji: { value: "127754" } });
    expect(markEmblem("emoji", "🚀")).toEqual({ in_use: "emoji", emoji: { value: "128640" } });
    expect(markEmblem("emoji", "🛰️")).toEqual({ in_use: "emoji", emoji: { value: "128752-65039" } });
  });

  it("shapes an icon mark", () => {
    expect(markEmblem("icon", { name: "star", color: "#fff" })).toEqual({
      in_use: "icon",
      icon: { name: "star", color: "#fff" },
    });
  });

  it("joins a multi-codepoint emoji, and leaves a non-string value as given", () => {
    expect(unifiedEmoji("👩‍🚀")).toBe("128105-8205-128640");
    expect(markEmblem("emoji", 7)).toEqual({ in_use: "emoji", emoji: { value: 7 } });
  });

  it("stores an uploaded emblem as an upload record with its URL", () => {
    expect(uploadedEmblem("/api/assets/emblem.png")).toEqual({
      in_use: "upload",
      url: "/api/assets/emblem.png",
    });
  });

  it("accepts the four image types up to five megabytes", () => {
    for (const type of ["image/png", "image/jpeg", "image/webp", "image/gif"]) {
      expect(emblemFileError({ type, size: EMBLEM_MAX_BYTES })).toBeNull();
    }
  });

  it("refuses another type or an oversized image", () => {
    expect(emblemFileError({ type: "image/svg+xml", size: 10 })).toBe(EMBLEM_TYPE_MESSAGE);
    expect(emblemFileError({ type: "image/png", size: EMBLEM_MAX_BYTES + 1 })).toBe(EMBLEM_SIZE_MESSAGE);
  });
});
