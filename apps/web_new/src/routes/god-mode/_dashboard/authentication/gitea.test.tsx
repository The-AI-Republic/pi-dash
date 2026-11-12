// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the Gitea address renders the
// shared provider page over the Gitea definition with the Gitea
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
  AUTH_PROVIDER_BY_SLUG: { gitea: { slug: "gitea" } },
}));
vi.mock("../../../../features/admin/providerCredentials.js", () => ({
  PROVIDER_CREDENTIALS: {
    gitea: {
      fields: "gitea-fields",
      prefill: "gitea-prefill",
      copyDescriptions: "gitea-descriptions",
      copyHelp: "gitea-help",
    },
  },
}));

import { Route } from "./gitea.js";

describe("gitea authentication route", () => {
  it("mounts the provider page over the Gitea definition", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("gitea authentication route has no component");
    render(<Component />);
    expect(screen.getByTestId("admin-auth-provider-page-gitea")).toBeInTheDocument();
    expect(seen.props).toMatchObject({
      provider: { slug: "gitea" },
      fields: "gitea-fields",
      prefill: "gitea-prefill",
      copyDescriptions: "gitea-descriptions",
      copyHelp: "gitea-help",
    });
  });
});
