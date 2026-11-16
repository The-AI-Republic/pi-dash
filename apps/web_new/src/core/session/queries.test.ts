// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { profileQueryOptions, sessionKeys, settingsQueryOptions } from "./queries.js";

describe("profile query", () => {
  it("keys the profile under the session", () => {
    expect(sessionKeys.profile).toEqual(["session", "profile"]);
  });

  it("fetches the profile through the client", async () => {
    const profile = { id: "profile-1", is_onboarded: true };
    const getJson = vi.fn(async () => profile);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = profileQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(sessionKeys.profile);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(profile);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/profile/");
  });
});

describe("settings query", () => {
  it("keys the settings under the session", () => {
    expect(sessionKeys.settings).toEqual(["session", "settings"]);
  });

  it("fetches the settings through the client", async () => {
    const settings = { id: "user-1", workspace: { last_workspace_slug: "acme" } };
    const getJson = vi.fn(async () => settings);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = settingsQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(sessionKeys.settings);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(settings);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/settings/");
  });
});
