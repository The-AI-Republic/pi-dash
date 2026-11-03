// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job modal pure helpers: create defaults, edit pre-fill, the
// save gate, payload builders, the builtin slug lock, and the
// save-failure message resolver.
import type { LoopJob } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  buildLoopJobCreateInput,
  buildLoopJobPatchInput,
  isLoopJobSaveEnabled,
  isLoopJobSlugLocked,
  LOOP_JOB_DEFAULT_MIN_ROLE,
  LOOP_JOB_DEFAULT_RRULE,
  LOOP_JOB_DEFAULT_TZID,
  LOOP_JOB_SAVE_FALLBACK_MESSAGE,
  LOOP_JOB_FIELD_LABELS,
  LOOP_JOB_FORM_DEFAULTS,
  LOOP_JOB_MODAL_CREATE_TITLE,
  LOOP_JOB_MODAL_EDIT_TITLE,
  LOOP_JOB_SAVE_FAILED_TITLE,
  loopJobFormFromJob,
  resolveLoopJobSaveError,
  type LoopJobFormFields,
} from "./loopJobModal.js";

function storedJob(overrides: Partial<LoopJob> = {}): LoopJob {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    slug: "probe-job",
    name: "Probe job",
    public_name: "Probe public",
    public_description: "Probe description",
    prompt: "Probe prompt",
    min_role: 20,
    enabled: true,
    is_builtin: false,
    dtstart: null,
    rrule: "FREQ=WEEKLY;BYDAY=MO",
    tzid: "Europe/Paris",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function fields(overrides: Partial<LoopJobFormFields> = {}): LoopJobFormFields {
  return {
    slug: "probe-job",
    name: "Probe job",
    publicName: "Probe public",
    publicDescription: "Probe description",
    prompt: "Probe prompt",
    minRole: 20,
    tzid: "UTC",
    rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
    ...overrides,
  };
}

describe("loop job modal copy", () => {
  it("pins the modal titles and the save-failure title", () => {
    expect(LOOP_JOB_MODAL_CREATE_TITLE).toBe("New job");
    expect(LOOP_JOB_MODAL_EDIT_TITLE).toBe("Edit job");
    expect(LOOP_JOB_SAVE_FAILED_TITLE).toBe("Save failed");
  });

  it("pins the generic save-failure fallback", () => {
    expect(LOOP_JOB_SAVE_FALLBACK_MESSAGE).toBe("Check the fields and try again.");
  });

  it("labels every field in modal order", () => {
    expect(LOOP_JOB_FIELD_LABELS).toEqual({
      slug: "Slug",
      name: "Admin name",
      publicName: "User-facing name",
      publicDescription: "User-facing description",
      prompt: "Prompt",
      minRole: "Min role",
      tzid: "Timezone",
      rrule: "Recurrence rule",
    });
  });
});

describe("loop job create defaults", () => {
  it("defaults the role to member, the timezone to UTC, and the recurrence to daily", () => {
    expect(LOOP_JOB_DEFAULT_MIN_ROLE).toBe(15);
    expect(LOOP_JOB_DEFAULT_TZID).toBe("UTC");
    expect(LOOP_JOB_DEFAULT_RRULE).toBe("FREQ=DAILY;BYHOUR=3;BYMINUTE=0");
  });

  it("starts identity fields blank under those defaults", () => {
    expect(LOOP_JOB_FORM_DEFAULTS).toEqual({
      slug: "",
      name: "",
      publicName: "",
      publicDescription: "",
      prompt: "",
      minRole: 15,
      tzid: "UTC",
      rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
    });
  });
});

describe("loopJobFormFromJob", () => {
  it("pre-fills every field from the stored job", () => {
    expect(loopJobFormFromJob(storedJob())).toEqual({
      slug: "probe-job",
      name: "Probe job",
      publicName: "Probe public",
      publicDescription: "Probe description",
      prompt: "Probe prompt",
      minRole: 20,
      tzid: "Europe/Paris",
      rrule: "FREQ=WEEKLY;BYDAY=MO",
    });
  });
});

describe("isLoopJobSaveEnabled", () => {
  it("enables save when slug, both names, and prompt are non-blank", () => {
    expect(isLoopJobSaveEnabled(fields())).toBe(true);
  });

  it("gates nothing on description, role, timezone, or recurrence", () => {
    expect(isLoopJobSaveEnabled(fields({ publicDescription: "", minRole: 5, tzid: "", rrule: "" }))).toBe(true);
  });

  it.each([
    ["slug", { slug: "" }],
    ["admin name", { name: "" }],
    ["user-facing name", { publicName: "" }],
    ["prompt", { prompt: "" }],
  ])("disables save when %s is blank", (_label, override) => {
    expect(isLoopJobSaveEnabled(fields(override))).toBe(false);
  });

  it.each([
    ["slug", { slug: "   " }],
    ["admin name", { name: "  " }],
    ["user-facing name", { publicName: "\t" }],
    ["prompt", { prompt: "   " }],
  ])("disables save when %s is whitespace-only", (_label, override) => {
    expect(isLoopJobSaveEnabled(fields(override))).toBe(false);
  });
});

describe("loop job payload builders", () => {
  it("builds the eight-key create body under the wire names", () => {
    expect(buildLoopJobCreateInput(fields())).toEqual({
      slug: "probe-job",
      name: "Probe job",
      public_name: "Probe public",
      public_description: "Probe description",
      prompt: "Probe prompt",
      min_role: 20,
      rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
      tzid: "UTC",
    });
  });

  it("builds the edit patch from the same eight fields", () => {
    expect(buildLoopJobPatchInput(fields({ name: "Renamed" }))).toEqual({
      slug: "probe-job",
      name: "Renamed",
      public_name: "Probe public",
      public_description: "Probe description",
      prompt: "Probe prompt",
      min_role: 20,
      rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
      tzid: "UTC",
    });
  });
});

describe("isLoopJobSlugLocked", () => {
  it("locks the slug for builtin jobs", () => {
    expect(isLoopJobSlugLocked(storedJob({ is_builtin: true }))).toBe(true);
  });

  it("leaves the slug editable for custom jobs", () => {
    expect(isLoopJobSlugLocked(storedJob({ is_builtin: false }))).toBe(false);
  });

  it("leaves the slug editable when creating", () => {
    expect(isLoopJobSlugLocked(undefined)).toBe(false);
  });
});

describe("resolveLoopJobSaveError", () => {
  it("prefers the server-provided text", () => {
    expect(resolveLoopJobSaveError(new Error("parity-probe-failure"))).toBe("parity-probe-failure");
  });

  it("falls back when the failure carries no text", () => {
    expect(resolveLoopJobSaveError(new Error("   "))).toBe("Check the fields and try again.");
  });

  it("falls back for non-errors", () => {
    expect(resolveLoopJobSaveError(null)).toBe("Check the fields and try again.");
    expect(resolveLoopJobSaveError("boom")).toBe("Check the fields and try again.");
  });
});
