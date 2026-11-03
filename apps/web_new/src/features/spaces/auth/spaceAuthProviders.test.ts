// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { SpaceAuthProviderFlags } from "./spaceAuthProviders.js";
import { resolveSpaceAuthProviders } from "./spaceAuthProviders.js";

const NONE: SpaceAuthProviderFlags = {
  googleEnabled: false,
  githubEnabled: false,
  gitlabEnabled: false,
  giteaEnabled: false,
};

describe("resolveSpaceAuthProviders", () => {
  it("resolves nothing when no provider is enabled", () => {
    expect(resolveSpaceAuthProviders(NONE, "sign-up")).toEqual([]);
  });

  it("resolves the enabled providers in display order", () => {
    const providers = resolveSpaceAuthProviders({ ...NONE, githubEnabled: true, googleEnabled: true }, "sign-in");
    expect(providers.map((provider) => provider.id)).toEqual(["google", "github"]);
    expect(providers[0]).toEqual({
      id: "google",
      name: "Google",
      monogram: "GO",
      label: "Continue with Google",
      href: "/auth/google/",
    });
  });

  it("follows the mode in the invitation", () => {
    const providers = resolveSpaceAuthProviders({ ...NONE, gitlabEnabled: true }, "sign-up");
    expect(providers[0]?.label).toBe("Create your account with GitLab");
  });

  it("carries the encoded return address and omits it when empty", () => {
    const flags: SpaceAuthProviderFlags = { ...NONE, giteaEnabled: true };
    expect(resolveSpaceAuthProviders(flags, "sign-in", "/issues/a?board=x")[0]?.href).toBe(
      "/auth/gitea/?next_path=%2Fissues%2Fa%3Fboard%3Dx"
    );
    expect(resolveSpaceAuthProviders(flags, "sign-in", "")[0]?.href).toBe("/auth/gitea/");
    expect(resolveSpaceAuthProviders(flags, "sign-in")[0]?.href).toBe("/auth/gitea/");
  });
});
