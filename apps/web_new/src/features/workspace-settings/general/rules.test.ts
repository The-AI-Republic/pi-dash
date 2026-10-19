// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rules for the general page (WSET-003–006): the shared name rule, size
// bands, merged timezone choices, logo file limits, the delete gate and
// the landing target.
import { describe, expect, it } from "vitest";

import {
  assetKeyFromUrl,
  canConfirmWorkspaceDelete,
  DELETE_CONFIRM_PHRASE,
  isLegacyLogoValue,
  LOGO_ACCEPTED_TYPES,
  LOGO_ACCEPT_ATTRIBUTE,
  LOGO_MAX_BYTES,
  LOGO_TOO_LARGE_MESSAGE,
  LOGO_WRONG_TYPE_MESSAGE,
  mergeTimezoneOptions,
  ORGANIZATION_SIZE_BANDS,
  ORGANIZATION_SIZE_PLACEHOLDER,
  postDeletePath,
  timezoneChoiceLabel,
  validateLogoFile,
  validateWorkspaceDetailsName,
  WORKSPACE_NAME_MAX_LENGTH,
  workspaceAddress,
  workspaceInitial,
} from "./rules.js";

describe("validateWorkspaceDetailsName", () => {
  it("requires a non-blank name with the pinned message", () => {
    expect(validateWorkspaceDetailsName("")).toBe("Workspace name is required");
    expect(validateWorkspaceDetailsName("   ")).toBe("Workspace name is required");
  });

  it("caps the length at eighty characters", () => {
    expect(validateWorkspaceDetailsName("a".repeat(WORKSPACE_NAME_MAX_LENGTH))).toBeNull();
    expect(validateWorkspaceDetailsName("a".repeat(WORKSPACE_NAME_MAX_LENGTH + 1))).toBe(
      "Workspace name must be 80 characters or less"
    );
  });

  it("refuses injection-risk characters naming special characters", () => {
    for (const name of ["Bad{Name}", "<x>", "a'b", 'a"b', "a[b]", "a*b", "a^b", "a!b", "a#b", "a%b"]) {
      expect(validateWorkspaceDetailsName(name)).toContain("special characters");
    }
  });

  it("refuses characters outside letters, numbers, spaces, hyphens and underscores", () => {
    expect(validateWorkspaceDetailsName("Acme Corp_item-2")).toBeNull();
    expect(validateWorkspaceDetailsName("Acme/Corp")).toBe(
      "Workspace name can only contain letters, numbers, spaces, hyphens and underscores"
    );
  });
});

describe("organization size", () => {
  it("offers exactly the six presets in order", () => {
    expect(ORGANIZATION_SIZE_BANDS).toEqual(["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"]);
    expect(ORGANIZATION_SIZE_PLACEHOLDER).toBe("Select organization size");
  });
});

describe("mergeTimezoneOptions", () => {
  const rows = [
    { value: "Pacific/Auckland", label: "Auckland", utc_offset: "UTC+13:00", gmt_offset: "GMT+13:00" },
    {
      value: "America/New_York",
      label: "Eastern Time (US and Canada)",
      utc_offset: "UTC-05:00",
      gmt_offset: "GMT-05:00",
    },
    { value: "Pacific/Auckland", label: "Wellington", utc_offset: "UTC+13:00", gmt_offset: "GMT+13:00" },
  ];

  it("leads with UTC and merges cities sharing a zone id", () => {
    const choices = mergeTimezoneOptions(rows);
    expect(choices.map((choice) => choice.id)).toEqual(["UTC", "Pacific/Auckland", "America/New_York"]);
    expect(choices[1]?.label).toBe("Auckland, Wellington");
    expect(choices[1]?.searchText).toContain("Pacific/Auckland");
    expect(choices[1]?.searchText).toContain("GMT+13:00");
    expect(choices[1]?.searchText).toContain("UTC+13:00");
  });

  it("walks a later duplicate into the merged label and search text", () => {
    const choices = mergeTimezoneOptions([
      { value: "X/y", label: "One", utc_offset: "UTC+1", gmt_offset: "GMT+1" },
      { value: "X/y", label: "Two", utc_offset: "UTC+1", gmt_offset: "GMT+1" },
      { value: "X/y", label: "Three", utc_offset: "UTC+1", gmt_offset: "GMT+1" },
    ]);
    expect(choices[1]?.label).toBe("One, Two, Three");
    expect(choices[1]?.searchText).toContain("One, Two, Three");
  });

  it("returns only UTC for an empty list", () => {
    expect(mergeTimezoneOptions([])).toEqual([
      { id: "UTC", label: "UTC", searchText: "utc, coordinated universal time" },
    ]);
  });
});

describe("timezoneChoiceLabel", () => {
  const choices = mergeTimezoneOptions([
    { value: "Pacific/Auckland", label: "Auckland", utc_offset: "UTC+13:00", gmt_offset: "GMT+13:00" },
  ]);

  it("shows the placeholder for no zone", () => {
    expect(timezoneChoiceLabel(choices, null)).toBe("Select a timezone");
    expect(timezoneChoiceLabel(choices, "", "Pick one")).toBe("Pick one");
  });

  it("shows the merged label, or the stored id when the list lacks it", () => {
    expect(timezoneChoiceLabel(choices, "Pacific/Auckland")).toBe("Auckland");
    expect(timezoneChoiceLabel(choices, "Mars/Olympus")).toBe("Mars/Olympus");
  });
});

describe("validateLogoFile", () => {
  it("accepts the three image types at the size limit", () => {
    for (const type of LOGO_ACCEPTED_TYPES) {
      expect(validateLogoFile({ type, size: LOGO_MAX_BYTES })).toBeNull();
    }
    expect(LOGO_ACCEPT_ATTRIBUTE).toBe("image/jpeg,image/png,image/webp");
  });

  it("refuses other types and oversize files", () => {
    expect(validateLogoFile({ type: "image/gif", size: 10 })).toBe(LOGO_WRONG_TYPE_MESSAGE);
    expect(validateLogoFile({ type: "image/png", size: LOGO_MAX_BYTES + 1 })).toBe(LOGO_TOO_LARGE_MESSAGE);
  });
});

describe("workspaceInitial", () => {
  it("shows the uppercased first character, falling back to W", () => {
    expect(workspaceInitial("acme")).toBe("A");
    expect(workspaceInitial("  zeta")).toBe("Z");
    expect(workspaceInitial("")).toBe("W");
    expect(workspaceInitial("   ")).toBe("W");
  });
});

describe("workspaceAddress", () => {
  it("joins the origin and slug without a doubled slash", () => {
    expect(workspaceAddress("https://app.test", "acme")).toBe("https://app.test/acme");
    expect(workspaceAddress("https://app.test/", "acme")).toBe("https://app.test/acme");
  });
});

describe("logo value shapes", () => {
  it("treats http(s) URLs as legacy and asset paths as current", () => {
    expect(isLegacyLogoValue("https://cdn.test/logo.png")).toBe(true);
    expect(isLegacyLogoValue("HTTP://cdn.test/logo.png")).toBe(true);
    expect(isLegacyLogoValue("/api/assets/v2/static/a1/")).toBe(false);
    expect(isLegacyLogoValue(null)).toBe(false);
    expect(isLegacyLogoValue(undefined)).toBe(false);
  });

  it("reads the trailing asset key ignoring a trailing slash", () => {
    expect(assetKeyFromUrl("/api/assets/v2/static/a1/")).toBe("a1");
    expect(assetKeyFromUrl("/api/assets/v2/static/a1")).toBe("a1");
    expect(assetKeyFromUrl("a1")).toBe("a1");
  });
});

describe("canConfirmWorkspaceDelete", () => {
  const base = { workspaceName: "Acme", typedName: "Acme", typedPhrase: DELETE_CONFIRM_PHRASE };

  it("accepts only an exact name and the exact phrase", () => {
    expect(DELETE_CONFIRM_PHRASE).toBe("delete my workspace");
    expect(canConfirmWorkspaceDelete(base)).toBe(true);
    expect(canConfirmWorkspaceDelete({ ...base, typedName: "acme" })).toBe(false);
    expect(canConfirmWorkspaceDelete({ ...base, typedPhrase: "delete my workspace " })).toBe(false);
    expect(canConfirmWorkspaceDelete({ ...base, typedPhrase: "wrong phrase" })).toBe(false);
  });
});

describe("postDeletePath", () => {
  it("prefers the last workspace, then the fallback, when it survives", () => {
    expect(
      postDeletePath({
        remainingSlugs: ["beta", "gamma"],
        lastWorkspaceSlug: "beta",
        fallbackWorkspaceSlug: "gamma",
        creationPath: "/create-workspace",
      })
    ).toBe("/beta/");
    expect(
      postDeletePath({
        remainingSlugs: ["gamma"],
        lastWorkspaceSlug: "gone",
        fallbackWorkspaceSlug: "gamma",
        creationPath: "/create-workspace",
      })
    ).toBe("/gamma/");
  });

  it("falls back to workspace creation when neither survives", () => {
    expect(
      postDeletePath({
        remainingSlugs: [],
        lastWorkspaceSlug: "gone",
        fallbackWorkspaceSlug: null,
        creationPath: "/create-workspace",
      })
    ).toBe("/create-workspace");
  });
});
