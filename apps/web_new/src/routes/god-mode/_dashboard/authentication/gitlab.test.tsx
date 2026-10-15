// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the GitLab address renders the
// shared provider page over the GitLab definition (shell, form, and
// notices belong to the admin area).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../features/admin/index.js", () => ({
  AdminAuthProviderPage: ({ provider }: { provider: { slug: string } }) => (
    <div data-testid={`admin-auth-provider-page-${provider.slug}`} />
  ),
  AUTH_PROVIDER_BY_SLUG: { gitlab: { slug: "gitlab" } },
}));

import { Route } from "./gitlab.js";

describe("gitlab authentication route", () => {
  it("mounts the provider page over the GitLab definition", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("gitlab authentication route has no component");
    render(<Component />);
    expect(screen.getByTestId("admin-auth-provider-page-gitlab")).toBeInTheDocument();
  });
});
