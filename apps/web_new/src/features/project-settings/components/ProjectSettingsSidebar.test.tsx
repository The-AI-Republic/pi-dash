// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings rail composition (PSET-001/PSET-004). The router Link is
// mocked: what this suite owns is grouping, the active marker on the
// canonical slash form and the role caption.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  useLinkProps: (props: { to?: string } & Record<string, unknown>) => ({ href: props.to }),
  Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => (
    <a
      href={props.to}
      className={props["className"] as string | undefined}
      {...({
        "aria-current": props["aria-current"],
        "aria-label": props["aria-label"],
        "data-testid": props["data-testid"],
        "data-active": props["data-active"],
      } as React.AnchorHTMLAttributes<HTMLAnchorElement>)}
    >
      {props.children as React.ReactNode}
    </a>
  ),
}));

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ProjectSettingsSidebar, roleLabel } from "./ProjectSettingsSidebar.js";

const WS = "acme";
const PID = "223e4567-e89b-12d3-a456-426614174001";
const ROOT = `/${WS}/settings/projects/${PID}`;

function renderSidebar(role: number | null, pathname: string) {
  return render(
    <ProjectSettingsSidebar
      workspaceSlug={WS}
      projectId={PID}
      projectName="Website relaunch"
      logoProps={{ in_use: "emoji", emoji: { value: "🚀" } }}
      identifier="WEB"
      role={role}
      pathname={pathname}
    />
  );
}

describe("roleLabel", () => {
  it("names each standing and leaves outsiders blank", () => {
    expect(roleLabel(ROLE_GUEST)).toBe("Guest");
    expect(roleLabel(ROLE_MEMBER)).toBe("Member");
    expect(roleLabel(ROLE_ADMIN)).toBe("Admin");
    expect(roleLabel(7)).toBe("Member");
    expect(roleLabel(null)).toBe("");
  });
});

describe("ProjectSettingsSidebar", () => {
  beforeEach(() => createAppClient("https://api.test"));

  afterEach(() => resetAppClient());

  it("renders nothing for an outsider", () => {
    const { container } = renderSidebar(null, `${ROOT}/`);
    expect(container).toBeEmptyDOMElement();
  });

  it("groups the admin entries with the project header", () => {
    renderSidebar(ROLE_ADMIN, `${ROOT}/`);
    expect(screen.getByRole("complementary", { name: "Project settings navigation" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to project" })).toHaveAttribute(
      "href",
      `/${WS}/projects/${PID}/issues/`
    );
    expect(screen.getByText("Website relaunch")).toBeInTheDocument();
    expect(screen.getByText("Admin")).toBeInTheDocument();
    expect(screen.getByText("🚀")).toBeInTheDocument();
    for (const [key, label] of [
      ["general", "General"],
      ["features", "Features"],
      ["work-structure", "Work structure"],
      ["execution", "Execution"],
    ] as const) {
      expect(screen.getByTestId(`project-settings-group-label-${key}`)).toHaveTextContent(label);
    }
    expect(screen.getByTestId("project-settings-group-work-structure")).toHaveTextContent("States");
    expect(screen.getAllByRole("link")).toHaveLength(15);
    expect(screen.getByTestId("project-settings-tab-github")).toHaveTextContent("Repository");
    expect(screen.getByTestId("project-settings-tab-github")).toHaveAttribute("href", `${ROOT}/github/`);
    expect(screen.getByTestId("project-settings-tab-ai_workers")).toHaveTextContent("AI Workers");
    expect(screen.getByTestId("project-settings-tab-ai_workers")).toHaveAttribute("href", `${ROOT}/ai-workers/`);
  });

  it("filters groups for members and guests", () => {
    const member = renderSidebar(ROLE_MEMBER, `${ROOT}/states/`);
    expect(screen.getByTestId("project-settings-tab-states")).toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-tab-cycles")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-group-features")).not.toBeInTheDocument();
    expect(screen.getByText("Member")).toBeInTheDocument();
    member.unmount();

    renderSidebar(ROLE_GUEST, `${ROOT}/`);
    expect(screen.getByTestId("project-settings-tab-members")).toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-tab-states")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-group-work-structure")).not.toBeInTheDocument();
    expect(screen.getByText("Guest")).toBeInTheDocument();
  });

  it("renders an uploaded emblem as an image resolved against the API origin", () => {
    render(
      <ProjectSettingsSidebar
        workspaceSlug={WS}
        projectId={PID}
        projectName="Website relaunch"
        logoProps={{ in_use: "upload", url: "/api/assets/v2/static/emblem/" }}
        identifier="WEB"
        role={ROLE_ADMIN}
        pathname={`${ROOT}/`}
      />
    );
    expect(screen.getByTestId("project-logo")).toHaveAttribute("data-mark", "image");
    expect(screen.getByTestId("project-logo-image")).toHaveAttribute(
      "src",
      "https://api.test/api/assets/v2/static/emblem/"
    );
  });

  it("renders an emblem picked from the wider emoji set in the rail", () => {
    render(
      <ProjectSettingsSidebar
        workspaceSlug={WS}
        projectId={PID}
        projectName="Website relaunch"
        logoProps={{ in_use: "emoji", emoji: { value: "128640" } }}
        identifier="WEB"
        role={ROLE_ADMIN}
        pathname={`${ROOT}/`}
      />
    );
    expect(screen.getByTestId("project-logo")).toHaveTextContent("🚀");
  });

  it("renders a chosen symbol in the rail", async () => {
    render(
      <ProjectSettingsSidebar
        workspaceSlug={WS}
        projectId={PID}
        projectName="Website relaunch"
        logoProps={{ in_use: "icon", icon: { name: "rocket_launch", color: "#f97316" } }}
        identifier="WEB"
        role={ROLE_ADMIN}
        pathname={`${ROOT}/`}
      />
    );
    expect(screen.getByTestId("project-logo")).toHaveAttribute("data-mark", "icon");
    expect(await screen.findByTestId("project-logo-icon")).toBeInTheDocument();
  });

  it("marks the canonical slash form and nothing else", () => {
    const active = renderSidebar(ROLE_ADMIN, `${ROOT}/members/`);
    expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("project-settings-tab-members")).toHaveClass("bg-layer-transparent-selected");
    expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("data-active", "true");
    expect(screen.getByTestId("project-settings-tab-general")).toHaveAttribute("data-active", "false");
    expect(screen.getByTestId("project-settings-tab-general")).not.toHaveClass("bg-layer-transparent-selected");
    active.unmount();

    renderSidebar(ROLE_ADMIN, `${ROOT}/members`);
    expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("data-active", "false");
    expect(screen.getByTestId("project-settings-tab-members")).not.toHaveAttribute("aria-current");
  });
});
