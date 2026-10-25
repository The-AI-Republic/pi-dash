// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the GitLab address renders the
// shared provider page over the GitLab definition with the GitLab
// credential set (shell, form, and notices belong to the admin area).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const seen = vi.hoisted(() => ({ props: null as null | Record<string, unknown> }));

vi.mock("../../../../features/admin/components/AdminAuthProviderPage.js", () => ({
  AdminAuthProviderPage: (props: Record<string, unknown>) => {
    seen.props = props;
    const provider = props.provider as { slug: string };
    return <div data-testid={`admin-auth-provider-page-${provider.slug}`} />;
  },
}));
vi.mock("../../../../features/admin/providerAuth.js", () => ({
  AUTH_PROVIDER_BY_SLUG: { gitlab: { slug: "gitlab" } },
}));
vi.mock("../../../../features/admin/providerCredentials.js", () => ({
  PROVIDER_CREDENTIALS: {
    gitlab: {
      fields: "gitlab-fields",
      prefill: "gitlab-prefill",
      copyDescriptions: "gitlab-descriptions",
      copyHelp: "gitlab-help",
    },
  },
}));

import { Route } from "./gitlab.js";

describe("gitlab authentication route", () => {
  it("mounts the provider page over the GitLab definition", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("gitlab authentication route has no component");
    render(<Component />);
    expect(screen.getByTestId("admin-auth-provider-page-gitlab")).toBeInTheDocument();
    expect(seen.props).toMatchObject({
      provider: { slug: "gitlab" },
      fields: "gitlab-fields",
      prefill: "gitlab-prefill",
      copyDescriptions: "gitlab-descriptions",
      copyHelp: "gitlab-help",
    });
  });
});
