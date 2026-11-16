// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link (CrossAppLink.test.tsx precedent): card
// actions targeting migrated prefixes render as router links, whose
// navigation is TanStack's own tested behavior.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => {
      const { to, children, ...rest } = props;
      return (
        <a data-router-link="" data-to={to} href={to} {...(rest as React.AnchorHTMLAttributes<HTMLAnchorElement>)}>
          {children as React.ReactNode}
        </a>
      );
    },
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN } from "../../../core/session/index.js";
import { sessionKeys, setSessionClient } from "../../../core/session/queries.js";
import { assistantKeys } from "../../assistant/index.js";
import { HomeDashboard } from "./HomeDashboard.js";

// Migrated-route links (F-09, NEWFRONT-528) render through the workspace
// routes this component ships in. The suite has no router; the router
// link's navigation is TanStack's own tested behavior, so it stands in as
// an anchor and the assertions still read the href.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => {
    const { to, children, ...rest } = props;
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  },
}));

function renderDashboard(options: {
  workspaceSlug?: string;
  seedWorkspace?: boolean;
  widgets?: React.ReactNode;
  onCreateProject?: () => void;
}): { rerenderAt: (workspaceSlug: string) => void } {
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  createAppClient("https://api.test", (async (url: string) => ({
    status: 500,
    url,
    headers: { get: () => null },
    text: async () => "{}",
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seedWorkspace !== false) {
    queryClient.setQueryData(sessionKeys.workspaces, [
      {
        id: "123e4567-e89b-12d3-a456-426614174000",
        name: "Acme",
        slug: "acme",
        logo_url: null,
        total_members: 1,
        role: ROLE_ADMIN,
      },
      {
        id: "123e4567-e89b-12d3-a456-426614174009",
        name: "Beta",
        slug: "beta",
        logo_url: null,
        total_members: 1,
        role: ROLE_ADMIN,
      },
    ]);
    queryClient.setQueryData(sessionKeys.me, {
      id: "123e4567-e89b-12d3-a456-426614174000",
      first_name: "Ada",
      user_timezone: "UTC",
    });
    queryClient.setQueryData(sessionKeys.profile, {
      id: "223e4567-e89b-12d3-a456-426614174001",
      language: "en",
      is_tour_completed: true,
    });
    for (const slug of ["acme", "beta"]) {
      queryClient.setQueryData(assistantKeys.threads(slug), []);
      queryClient.setQueryData(assistantKeys.config(slug), {
        provider_kind: "openai",
        base_url: "https://api.test",
        model_name: "",
        has_api_key: false,
        last_verified_at: null,
      });
    }
  }
  const renderAt = (slug: string): React.ReactElement => (
    <QueryClientProvider client={queryClient}>
      <HomeDashboard
        workspaceSlug={slug}
        projects={[]}
        {...(options.onCreateProject === undefined ? {} : { onCreateProject: options.onCreateProject })}
        {...(options.widgets === undefined ? {} : { widgets: options.widgets })}
      />
    </QueryClientProvider>
  );
  const rendered = render(renderAt(options.workspaceSlug ?? "acme"));
  return { rerenderAt: (slug: string) => rendered.rerender(renderAt(slug)) };
}

beforeEach(() => {
  window.localStorage.clear();
  resetAppClient();
});

describe("HomeDashboard", () => {
  it("composes the greeting, assistant card and quickstart with the workspace title", async () => {
    renderDashboard({ widgets: <div data-testid="widget-stack" /> });
    expect(await screen.findByRole("heading", { name: /good (morning|afternoon|evening), ada/i })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Assistant" })).toHaveAttribute("data-assistant-state", "setup");
    expect(screen.getByRole("region", { name: "Quickstart checklist" })).toBeInTheDocument();
    expect(screen.getByTestId("widget-stack")).toBeInTheDocument();
    expect(document.title).toContain("Acme");
  });

  it("renders the create action as a list link until the dialog lands", () => {
    renderDashboard({});
    expect(screen.getByRole("link", { name: "Get started" })).toHaveAttribute("href", "/acme/projects");
  });

  it("forwards the create-project opener to the quickstart card", async () => {
    const user = userEvent.setup();
    const onCreateProject = vi.fn();
    renderDashboard({ onCreateProject });
    await user.click(screen.getByRole("button", { name: "Get started" }));
    expect(onCreateProject).toHaveBeenCalledTimes(1);
  });

  it("renders nothing before workspace context exists", () => {
    renderDashboard({ seedWorkspace: false });
    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
    expect(document.title).toContain("acme");
  });

  it("re-reads per-workspace quickstart flags when the workspace changes", async () => {
    const user = userEvent.setup();
    const { rerenderAt } = renderDashboard({});
    expect(screen.getByRole("region", { name: "Quickstart checklist" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("region", { name: "Quickstart checklist" })).not.toBeInTheDocument();
    rerenderAt("beta");
    expect(document.title).toContain("Beta");
    expect(screen.getByRole("region", { name: "Quickstart checklist" })).toBeInTheDocument();
  });
});
