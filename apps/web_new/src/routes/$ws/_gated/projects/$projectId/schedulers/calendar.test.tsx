// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers calendar route (parity/specs/agents.md §5,
// AGT-015–017, 062): the route reads the caller's project role and hands
// the calendar the admin flag — admins get the panel's edit entry point,
// members and a failed role read do not — and sets the document head.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { hooks } = vi.hoisted(() => ({
  hooks: {
    roles: { isSuccess: true, data: {} as Record<string, number> },
    workspace: { name: "Acme" } as { name?: string } | undefined,
    head: vi.fn(),
    calendar: vi.fn(),
  },
}));

vi.mock("../../../../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../../../../core/session/index.js")>();
  return { ...original, useWorkspace: () => ({ workspace: hooks.workspace }) };
});
vi.mock("../../../../../../features/auth/index.js", () => ({
  useProjectRoles: () => hooks.roles,
}));
vi.mock("../../../../../../features/schedulers/FiringCalendar.js", () => ({
  FiringCalendar: (props: { workspaceSlug: string; projectId: string; canEdit: boolean }) => {
    hooks.calendar(props);
    return <div data-testid="calendar" />;
  },
}));
vi.mock("../../../../../../shared/shell/index.js", () => ({
  useDocumentHead: (value: { title: string }) => hooks.head(value),
}));

import { ROLE_ADMIN, ROLE_MEMBER } from "../../../../../../core/session/index.js";
import { Route } from "./calendar.js";

const CalendarComponent = Route.options.component as () => React.ReactElement;

async function renderRoute(): Promise<void> {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: "p1" });
  render(<CalendarComponent />);
  // The calendar body is a lazy leaf; wait for it to mount.
  await screen.findByTestId("calendar");
}

afterEach(() => {
  hooks.roles = { isSuccess: true, data: {} };
  hooks.workspace = { name: "Acme" };
  vi.clearAllMocks();
});

describe("ProjectSchedulersCalendarComponent", () => {
  it("lets project admins edit from the panel", async () => {
    hooks.roles = { isSuccess: true, data: { p1: ROLE_ADMIN } };
    await renderRoute();
    expect(hooks.calendar).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: "p1", canEdit: true });
    expect(hooks.head).toHaveBeenCalledWith({ title: "Acme · Scheduler calendar", robots: "noindex, nofollow" });
    expect(screen.getByTestId("calendar")).toBeInTheDocument();
  });

  it("keeps members read-only", async () => {
    hooks.roles = { isSuccess: true, data: { p1: ROLE_MEMBER } };
    await renderRoute();
    expect(hooks.calendar).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: "p1", canEdit: false });
  });

  it("refuses the edit entry point when the role read failed", async () => {
    hooks.roles = { isSuccess: false, data: {} };
    await renderRoute();
    expect(hooks.calendar).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: "p1", canEdit: false });
  });

  it("names the workspace slug when the workspace has not loaded", async () => {
    hooks.roles = { isSuccess: true, data: {} };
    hooks.workspace = undefined;
    await renderRoute();
    expect(hooks.head).toHaveBeenCalledWith({ title: "acme · Scheduler calendar", robots: "noindex, nofollow" });
  });
});
