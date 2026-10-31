// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildCreateLogoProps,
  buildCreatePayload,
  CREATE_DEFAULT_ICON,
  CREATE_ICON_CHOICES,
  CREATE_BASE_BRANCH_MAX_LENGTH,
  CREATE_IDENTIFIER_MAX_LENGTH,
  CREATE_NAME_MAX_LENGTH,
  CREATE_REPO_URL_MAX_LENGTH,
  deriveProjectIdentifier,
  IDENTIFIER_TAKEN_CODE,
  IDENTIFIER_TAKEN_MESSAGE,
  NAME_TAKEN_CODE,
  NAME_TAKEN_MESSAGE,
  pickCreateIcon,
  resolveBrowserTimezone,
  sanitizeProjectIdentifier,
  selectCreateFieldErrors,
  validateCreateFields,
} from "./project-create.js";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("sanitizeProjectIdentifier", () => {
  it("keeps letters and digits across scripts", () => {
    expect(sanitizeProjectIdentifier("NebulaCraft42ÇŞĞ")).toBe("NebulaCraft42ÇŞĞ");
  });

  it("drops separators, marks and symbols", () => {
    expect(sanitizeProjectIdentifier("my project!@#-1")).toBe("myproject1");
  });

  it("keeps the empty string empty", () => {
    expect(sanitizeProjectIdentifier("")).toBe("");
  });
});

describe("deriveProjectIdentifier", () => {
  it("derives the code from the name", () => {
    expect(deriveProjectIdentifier("Nebula Craft")).toBe("NebulaCraf");
  });

  it("caps the code at the form length", () => {
    expect(deriveProjectIdentifier("abcdefghijklmnop").length).toBe(CREATE_IDENTIFIER_MAX_LENGTH);
  });

  it("derives nothing from a blank name", () => {
    expect(deriveProjectIdentifier("   ")).toBe("");
  });
});

describe("resolveBrowserTimezone", () => {
  it("reads the browser zone", () => {
    expect(resolveBrowserTimezone()).not.toBe("");
  });

  it("falls back to UTC when detection throws", () => {
    vi.spyOn(Intl, "DateTimeFormat").mockImplementationOnce((() => {
      throw new Error("tz down");
    }) as unknown as typeof Intl.DateTimeFormat);
    expect(resolveBrowserTimezone()).toBe("UTC");
  });

  it("falls back to UTC on an empty zone", () => {
    vi.spyOn(Intl, "DateTimeFormat").mockImplementationOnce((() => ({
      resolvedOptions: () => ({ timeZone: "" }),
    })) as unknown as typeof Intl.DateTimeFormat);
    expect(resolveBrowserTimezone()).toBe("UTC");
  });

  it("falls back to UTC on a missing zone", () => {
    vi.spyOn(Intl, "DateTimeFormat").mockImplementationOnce((() => ({
      resolvedOptions: () => ({}),
    })) as unknown as typeof Intl.DateTimeFormat);
    expect(resolveBrowserTimezone()).toBe("UTC");
  });
});

describe("pickCreateIcon", () => {
  it("picks the first icon at zero", () => {
    expect(pickCreateIcon(() => 0)).toBe(CREATE_ICON_CHOICES[0]);
  });

  it("picks the last icon near one", () => {
    expect(pickCreateIcon(() => 0.9999)).toBe(CREATE_ICON_CHOICES[CREATE_ICON_CHOICES.length - 1]);
  });

  it("falls back when the source lands out of range", () => {
    expect(pickCreateIcon(() => 2)).toBe(CREATE_DEFAULT_ICON);
  });

  it("draws from the set by default", () => {
    expect(CREATE_ICON_CHOICES).toContain(pickCreateIcon());
  });
});

describe("buildCreateLogoProps", () => {
  it("wraps the glyph as the in-use emoji", () => {
    expect(buildCreateLogoProps("🌊")).toEqual({ in_use: "emoji", emoji: { value: "🌊" } });
  });
});

describe("validateCreateFields", () => {
  it("accepts a filled form", () => {
    expect(validateCreateFields({ name: "Nebula", identifier: "NEB" })).toEqual({});
  });

  it("requires a name", () => {
    expect(validateCreateFields({ name: "   ", identifier: "NEB" }).name).not.toBeUndefined();
  });

  it("caps the name length", () => {
    const long = "n".repeat(CREATE_NAME_MAX_LENGTH + 1);
    expect(validateCreateFields({ name: long, identifier: "NEB" }).name).toContain(String(CREATE_NAME_MAX_LENGTH));
  });

  it("requires a short code", () => {
    expect(validateCreateFields({ name: "Nebula", identifier: "" }).identifier).not.toBeUndefined();
  });

  it("caps the short code length", () => {
    const long = "A".repeat(CREATE_IDENTIFIER_MAX_LENGTH + 1);
    expect(validateCreateFields({ name: "Nebula", identifier: long }).identifier).toContain(
      String(CREATE_IDENTIFIER_MAX_LENGTH)
    );
  });

  it("rejects identifier punctuation", () => {
    expect(validateCreateFields({ name: "Nebula", identifier: "NEB!" }).identifier).toContain("letters and numbers");
  });

  it("caps the repository URL", () => {
    const long = "x".repeat(CREATE_REPO_URL_MAX_LENGTH + 1);
    expect(validateCreateFields({ name: "Nebula", identifier: "NEB", repoUrl: long }).repoUrl).toContain(
      String(CREATE_REPO_URL_MAX_LENGTH)
    );
  });

  it("caps and shapes the base branch", () => {
    const long = "x".repeat(CREATE_BASE_BRANCH_MAX_LENGTH + 1);
    expect(validateCreateFields({ name: "Nebula", identifier: "NEB", baseBranch: long }).baseBranch).toContain(
      String(CREATE_BASE_BRANCH_MAX_LENGTH)
    );
    expect(validateCreateFields({ name: "Nebula", identifier: "NEB", baseBranch: "feat branch" }).baseBranch).toContain(
      "may only contain"
    );
  });
});

describe("selectCreateFieldErrors", () => {
  it("maps a reused name onto the name field", () => {
    const error = new ApiError({ status: 400, code: "http", message: "taken", fields: { name: [NAME_TAKEN_CODE] } });
    expect(selectCreateFieldErrors(error)).toEqual({ name: NAME_TAKEN_MESSAGE });
  });

  it("maps a reused code onto the identifier field", () => {
    const error = new ApiError({
      status: 400,
      code: "http",
      message: "taken",
      fields: { identifier: [IDENTIFIER_TAKEN_CODE] },
    });
    expect(selectCreateFieldErrors(error)).toEqual({ identifier: IDENTIFIER_TAKEN_MESSAGE });
  });

  it("maps both fields at once", () => {
    const error = new ApiError({
      status: 400,
      code: "http",
      message: "taken",
      fields: { name: [NAME_TAKEN_CODE], identifier: [IDENTIFIER_TAKEN_CODE] },
    });
    expect(selectCreateFieldErrors(error)).toEqual({ name: NAME_TAKEN_MESSAGE, identifier: IDENTIFIER_TAKEN_MESSAGE });
  });

  it("leaves fields clear for other failures", () => {
    expect(selectCreateFieldErrors(new Error("boom"))).toEqual({});
    expect(selectCreateFieldErrors(new ApiError({ status: 500, code: "http", message: "broke" }))).toEqual({});
    const other = new ApiError({
      status: 400,
      code: "http",
      message: "other",
      fields: { timezone: ["UNKNOWN_ZONE"] },
    });
    expect(selectCreateFieldErrors(other)).toEqual({});
    const unrelated = new ApiError({
      status: 400,
      code: "http",
      message: "unrelated",
      fields: { name: ["TOO_LONG"], identifier: ["TOO_LONG"] },
    });
    expect(selectCreateFieldErrors(unrelated)).toEqual({});
  });
});

describe("buildCreatePayload", () => {
  it("assembles the single-POST body with work-items-only flags", () => {
    const payload = buildCreatePayload({
      name: "  Nebula Craft ",
      identifier: "neb",
      description: "Team space",
      repoUrl: "git@github.com:acme/nebula.git",
      baseBranch: "develop",
      network: 2,
      projectLead: null,
      icon: "🌊",
      timezone: "Asia/Singapore",
    });
    expect(payload).toEqual({
      name: "Nebula Craft",
      identifier: "NEB",
      description: "Team space",
      repo_url: "git@github.com:acme/nebula.git",
      base_branch: "develop",
      network: 2,
      project_lead: null,
      logo_props: { in_use: "emoji", emoji: { value: "🌊" } },
      timezone: "Asia/Singapore",
      cycle_view: false,
      module_view: false,
      issue_views_view: false,
      page_view: false,
      intake_view: false,
    });
  });

  it("carries no cover keys", () => {
    const payload = buildCreatePayload({
      name: "Nebula",
      identifier: "NEB",
      description: "",
      repoUrl: "",
      baseBranch: "",
      network: 0,
      projectLead: "323e4567-e89b-12d3-a456-426614174002",
      icon: "🌊",
      timezone: "UTC",
    });
    for (const key of Object.keys(payload)) {
      expect(key.startsWith("cover_")).toBe(false);
    }
    expect(payload.project_lead).toBe("323e4567-e89b-12d3-a456-426614174002");
  });

  it("sends empty repository fields when the optional values are omitted", () => {
    const payload = buildCreatePayload({
      name: "Nebula",
      identifier: "NEB",
      description: "",
      network: 0,
      projectLead: null,
      icon: "🌊",
      timezone: "UTC",
    });
    expect(payload.repo_url).toBe("");
    expect(payload.base_branch).toBe("");
  });
});
