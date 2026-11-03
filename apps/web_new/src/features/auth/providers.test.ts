// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { InstanceConfig } from "@pidash/api-client";
import { describe, expect, it } from "vitest";
import { CLOUD_PROVIDER_ID, resolveSignInProviders } from "./providers.js";

function configWith(flags: Partial<InstanceConfig>): InstanceConfig {
  return {
    enable_signup: true,
    is_workspace_creation_disabled: false,
    is_google_enabled: false,
    is_github_enabled: false,
    is_gitlab_enabled: false,
    is_gitea_enabled: false,
    is_magic_login_enabled: true,
    is_email_password_enabled: true,
    is_smtp_configured: true,
    is_self_managed: false,
    instance_changelog_url: "",
    ...flags,
  };
}

describe("sign-in providers", () => {
  it("offers only the providers enabled in both the edition and the instance", () => {
    const resolved = resolveSignInProviders(
      [
        { id: "google", label: "Google" },
        { id: "github", label: "GitHub" },
      ],
      configWith({ is_google_enabled: true }),
      { mode: "sign-in", apiOrigin: "https://api.test" }
    );
    expect(resolved).toEqual([{ id: "google", label: "Sign in with Google", href: "https://api.test/auth/google/" }]);
  });

  it("resolves every known provider with mode-aware labels", () => {
    const resolved = resolveSignInProviders(
      [
        { id: "google", label: "Google" },
        { id: "github", label: "GitHub" },
        { id: "gitlab", label: "GitLab" },
        { id: "gitea", label: "Gitea" },
      ],
      configWith({
        is_google_enabled: true,
        is_github_enabled: true,
        is_gitlab_enabled: true,
        is_gitea_enabled: true,
      }),
      { mode: "sign-up", apiOrigin: "https://api.test/" }
    );
    expect(resolved.map((provider) => provider.label)).toEqual([
      "Sign up with Google",
      "Sign up with GitHub",
      "Sign up with GitLab",
      "Sign up with Gitea",
    ]);
    expect(resolved.map((provider) => provider.href)).toEqual([
      "https://api.test/auth/google/",
      "https://api.test/auth/github/",
      "https://api.test/auth/gitlab/",
      "https://api.test/auth/gitea/",
    ]);
  });

  it("forwards the return path to the provider URL", () => {
    const resolved = resolveSignInProviders(
      [{ id: "google", label: "Google" }],
      configWith({ is_google_enabled: true }),
      {
        mode: "sign-in",
        apiOrigin: "https://api.test",
        nextPath: "/acme/issues?layout=compact",
      }
    );
    expect(resolved).toEqual([
      {
        id: "google",
        label: "Sign in with Google",
        href: "https://api.test/auth/google/?next_path=%2Facme%2Fissues%3Flayout%3Dcompact",
      },
    ]);
  });

  it("ignores unknown provider ids and an empty edition", () => {
    const config = configWith({ is_google_enabled: true });
    expect(
      resolveSignInProviders([{ id: "saml", label: "SAML" }], config, {
        mode: "sign-in",
        apiOrigin: "https://api.test",
      })
    ).toEqual([]);
    expect(resolveSignInProviders([], config, { mode: "sign-in", apiOrigin: "https://api.test" })).toEqual([]);
    expect(resolveSignInProviders(undefined, config, { mode: "sign-in", apiOrigin: "https://api.test" })).toEqual([]);
  });

  it("resolves the organizational provider with the page-preserving handoff", () => {
    const config = configWith({});
    const resolved = resolveSignInProviders([{ id: CLOUD_PROVIDER_ID, label: "AI Republic" }], config, {
      mode: "sign-up",
      apiOrigin: "https://api.test",
      returnToPage: "/sign-up?next_path=%2Fdocs",
      browserOrigin: "https://app.test",
    });
    expect(resolved).toEqual([
      {
        id: CLOUD_PROVIDER_ID,
        label: "Sign up with AI Republic",
        href: "https://api.test/api/auth/sign-in/?return_to=%2Fsign-up%3Fnext_path%3D%252Fdocs&return_origin=https%3A%2F%2Fapp.test",
        cloudHandoff: { returnTo: "/sign-up?next_path=%2Fdocs" },
      },
    ]);
  });

  it("needs no instance flag and defaults the return target to the root", () => {
    const config = configWith({});
    const resolved = resolveSignInProviders([{ id: CLOUD_PROVIDER_ID, label: "AI Republic" }], config, {
      mode: "sign-in",
      apiOrigin: "https://api.test",
      browserOrigin: "https://api.test",
    });
    expect(resolved).toEqual([
      {
        id: CLOUD_PROVIDER_ID,
        label: "Sign in with AI Republic",
        href: "https://api.test/api/auth/sign-in/?return_to=%2F",
        cloudHandoff: { returnTo: "/" },
      },
    ]);
    const unoriginated = resolveSignInProviders([{ id: CLOUD_PROVIDER_ID, label: "AI Republic" }], config, {
      mode: "sign-in",
      apiOrigin: "https://api.test",
    });
    expect(unoriginated[0]?.href).toBe("https://api.test/api/auth/sign-in/?return_to=%2F");
  });
});
