// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildCreateLogoProps,
  buildCreatePayload,
  buildCreateReposPayload,
  CREATE_DEFAULT_ICON,
  CREATE_ICON_CHOICES,
  CREATE_BASE_BRANCH_MAX_LENGTH,
  CREATE_IDENTIFIER_MAX_LENGTH,
  CREATE_NAME_MAX_LENGTH,
  CREATE_REPO_BRANCH_TOO_LONG_MESSAGE,
  CREATE_REPO_DUPLICATE_MESSAGE,
  CREATE_REPO_PROVIDER_LABELS,
  CREATE_REPO_URL_INVALID_MESSAGE,
  CREATE_REPO_URL_MAX_LENGTH,
  CREATE_REPO_URL_REQUIRED_MESSAGE,
  CREATE_REPO_URL_TOO_LONG_MESSAGE,
  createRepoIdentityKey,
  deriveProjectIdentifier,
  IDENTIFIER_TAKEN_CODE,
  IDENTIFIER_TAKEN_MESSAGE,
  inferCreateRepoProvider,
  NAME_TAKEN_CODE,
  NAME_TAKEN_MESSAGE,
  pickCreateIcon,
  resolveBrowserTimezone,
  sanitizeProjectIdentifier,
  selectCreateFieldErrors,
  validateCreateFields,
  validateCreateRepoRows,
  type CreateRepoDraft,
} from "./project-create.js";

function draft(overrides: Partial<CreateRepoDraft> = {}): CreateRepoDraft {
  return { url: "https://github.com/acme/nebula", provider: "github", baseBranch: "develop", ...overrides };
}

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

  it("accepts zero repositories", () => {
    expect(validateCreateFields({ name: "Nebula", identifier: "NEB", repos: [] })).toEqual({});
    expect(validateCreateFields({ name: "Nebula", identifier: "NEB" })).toEqual({});
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

  it("maps a refused repos entry onto its row", () => {
    const error = new ApiError({
      status: 400,
      code: "http",
      message: "invalid",
      fields: { settings: ["invalid value for repos: entry 1 is a duplicate of another repository in the list"] },
    });
    expect(selectCreateFieldErrors(error)).toEqual({
      repos: [undefined, { row: "is a duplicate of another repository in the list" }],
    });
  });

  it("surfaces unindexed settings refusals on the list", () => {
    const error = new ApiError({
      status: 400,
      code: "http",
      message: "invalid",
      fields: { settings: ["settings must be a JSON object"] },
    });
    expect(selectCreateFieldErrors(error)).toEqual({ reposError: "settings must be a JSON object" });
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

describe("validateCreateRepoRows", () => {
  it("accepts clean rows and infers providers by host", () => {
    expect(validateCreateRepoRows([draft()])).toEqual([{}]);
    expect(inferCreateRepoProvider("https://github.com/acme/nebula")).toBe("github");
    expect(inferCreateRepoProvider("https://gitlab.com/acme/nebula")).toBe("gitlab");
    expect(inferCreateRepoProvider("ssh://git@git.test/acme/nebula.git")).toBe("other");
    expect(CREATE_REPO_PROVIDER_LABELS).toEqual({ github: "GitHub", gitlab: "GitLab", other: "Other" });
  });

  it("requires a shaped URL within its cap", () => {
    expect(validateCreateRepoRows([draft({ url: "  " })])[0]?.url).toBe(CREATE_REPO_URL_REQUIRED_MESSAGE);
    const long = `https://git.test/${"x".repeat(CREATE_REPO_URL_MAX_LENGTH)}`;
    expect(validateCreateRepoRows([draft({ url: long })])[0]?.url).toBe(CREATE_REPO_URL_TOO_LONG_MESSAGE);
    expect(validateCreateRepoRows([draft({ url: "not a repo" })])[0]?.url).toBe(CREATE_REPO_URL_INVALID_MESSAGE);
  });

  it("caps the branch and rejects duplicates in another URL form", () => {
    const long = "x".repeat(CREATE_BASE_BRANCH_MAX_LENGTH + 1);
    expect(validateCreateRepoRows([draft({ baseBranch: long })])[0]?.baseBranch).toBe(
      CREATE_REPO_BRANCH_TOO_LONG_MESSAGE
    );
    expect(validateCreateRepoRows([draft({ baseBranch: "feat branch" })])[0]?.baseBranch).toContain("may only contain");
    const errors = validateCreateRepoRows([draft(), draft({ url: "git@github.com:acme/nebula.git" })]);
    expect(errors[0]).toEqual({});
    expect(errors[1]?.url).toBe(CREATE_REPO_DUPLICATE_MESSAGE);
  });

  it("maps every URL form of one repo to one key", () => {
    const key = createRepoIdentityKey("https://github.com/acme/nebula");
    expect(key).toBe("github.com/acme/nebula");
    for (const form of ["git@github.com:acme/nebula.git", "https://GITHUB.COM/ACME/NEBULA/"]) {
      expect(createRepoIdentityKey(form)).toBe(key);
    }
    expect(createRepoIdentityKey("not a repo")).toBeNull();
  });
});

describe("buildCreatePayload", () => {
  it("assembles the single-POST body with work-items-only flags", () => {
    const payload = buildCreatePayload({
      name: "  Nebula Craft ",
      identifier: "neb",
      description: "Team space",
      repos: [draft()],
      network: 2,
      projectLead: null,
      icon: "🌊",
      timezone: "Asia/Singapore",
    });
    expect(payload).toEqual({
      name: "Nebula Craft",
      identifier: "NEB",
      description: "Team space",
      settings: {
        repos: [
          {
            id: expect.any(String),
            url: "https://github.com/acme/nebula",
            provider: "github",
            base_branch: "develop",
          },
        ],
      },
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
    expect("repo_url" in payload).toBe(false);
    expect("base_branch" in payload).toBe(false);
  });

  it("writes blank branches as null and fresh ids per entry", () => {
    const entries = buildCreateReposPayload([
      draft({ baseBranch: "" }),
      draft({ url: "git@gitlab.com:acme/other.git", provider: "gitlab", baseBranch: "trunk" }),
    ]);
    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({ url: "https://github.com/acme/nebula", provider: "github", base_branch: null });
    expect(entries[1]).toMatchObject({
      url: "git@gitlab.com:acme/other.git",
      provider: "gitlab",
      base_branch: "trunk",
    });
    expect(entries[0]?.id).not.toBe(entries[1]?.id);
  });

  it("carries no cover keys", () => {
    const payload = buildCreatePayload({
      name: "Nebula",
      identifier: "NEB",
      description: "",
      repos: [],
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

  it("sends no settings bag when no repository is listed", () => {
    const payload = buildCreatePayload({
      name: "Nebula",
      identifier: "NEB",
      description: "",
      network: 0,
      projectLead: null,
      icon: "🌊",
      timezone: "UTC",
    });
    expect("settings" in payload).toBe(false);
    expect("repo_url" in payload).toBe(false);
    expect("base_branch" in payload).toBe(false);
  });
});
