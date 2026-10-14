// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { edition } from "./index.js";
import { edition as ossEdition } from "./oss.js";

describe("edition", () => {
  it("resolves the OSS edition through the index", () => {
    expect(edition).toBe(ossEdition);
    expect(edition.id).toBe("oss");
  });

  it("contributes no cloud surface", () => {
    expect(edition.routes ?? []).toEqual([]);
    expect(edition.sidebar ?? []).toEqual([]);
    expect(edition.settingsSections ?? []).toEqual([]);
    // The four native providers are backend-native, not a cloud extra: the
    // instance flags alone decide which buttons show (AUTH-003).
    expect(edition.auth ?? []).toEqual([
      { id: "google", label: "Google" },
      { id: "github", label: "GitHub" },
      { id: "gitlab", label: "GitLab" },
      { id: "gitea", label: "Gitea" },
    ]);
    expect(edition.api ?? []).toEqual([]);
    expect(edition.flags).toEqual({});
    expect(edition.slots ?? {}).toEqual({});
  });
});
