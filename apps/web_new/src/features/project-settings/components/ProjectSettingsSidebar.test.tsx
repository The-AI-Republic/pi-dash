// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings rail composition (PSET-001/PSET-004). The router harness is real
// so the current-page marker is exercised through TanStack's own matching:
// what this suite owns is grouping, the slash-sensitive active marker (with
// exactly one aria-current entry) and the role caption.
import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { renderInRouter } from "../testing.js";
import { ProjectSettingsSidebar, roleLabel } from "./ProjectSettingsSidebar.js";

const WS = "acme";
const PID = "223e4567-e89b-12d3-a456-426614174001";
const ROOT = `/${WS}/settings/projects/${PID}`;

function renderSidebar(role: number | null, pathname: string) {
  return render(
    renderInRouter(
      pathname,
      <ProjectSettingsSidebar
        workspaceSlug={WS}
        projectId={PID}
        projectName="Website relaunch"
        logoProps={{ in_use: "emoji", emoji: { value: "🚀" } }}
        identifier="WEB"
        role={role}
        pathname={pathname}
      />
    )
  );
}

afterEach(() => {
  cleanup();
});

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
  it("renders nothing for an outsider", () => {
    const { container } = renderSidebar(null, `${ROOT}/`);
    expect(container).toBeEmptyDOMElement();
  });

  it("groups the admin entries with the project header", async () => {
    renderSidebar(ROLE_ADMIN, `${ROOT}/`);
    await screen.findByTestId("project-settings-sidebar");
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
    expect(screen.getAllByRole("link")).toHaveLength(14);
    expect(screen.getByTestId("project-settings-tab-github")).toHaveTextContent("Repository");
    expect(screen.getByTestId("project-settings-tab-github")).toHaveAttribute("href", `${ROOT}/github/`);
  });

  it("filters groups for members and guests", async () => {
    const member = renderSidebar(ROLE_MEMBER, `${ROOT}/states/`);
    await screen.findByTestId("project-settings-sidebar");
    expect(screen.getByTestId("project-settings-tab-states")).toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-tab-cycles")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-group-features")).not.toBeInTheDocument();
    expect(screen.getByText("Member")).toBeInTheDocument();
    member.unmount();

    renderSidebar(ROLE_GUEST, `${ROOT}/`);
    await screen.findByTestId("project-settings-sidebar");
    expect(screen.getByTestId("project-settings-tab-members")).toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-tab-states")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-group-work-structure")).not.toBeInTheDocument();
    expect(screen.getByText("Guest")).toBeInTheDocument();
  });

  it("marks the canonical slash form and nothing else", async () => {
    const active = renderSidebar(ROLE_ADMIN, `${ROOT}/members/`);
    await screen.findByTestId("project-settings-sidebar");
    expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("project-settings-tab-members")).toHaveClass("bg-layer-transparent-selected");
    expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("data-active", "true");
    expect(screen.getByTestId("project-settings-tab-general")).toHaveAttribute("data-active", "false");
    expect(screen.getByTestId("project-settings-tab-general")).not.toHaveClass("bg-layer-transparent-selected");
    active.unmount();

    renderSidebar(ROLE_ADMIN, `${ROOT}/members`);
    await screen.findByTestId("project-settings-sidebar");
    expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("data-active", "false");
    expect(screen.getByTestId("project-settings-tab-members")).not.toHaveAttribute("aria-current");
  });

  it("reports exactly one current entry on the slash form and none without it", async () => {
    const slash = renderSidebar(ROLE_ADMIN, `${ROOT}/members/`);
    await screen.findByTestId("project-settings-sidebar");
    expect(document.querySelectorAll('[aria-current="page"]')).toHaveLength(1);
    expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("project-settings-tab-general")).not.toHaveAttribute("aria-current");
    slash.unmount();

    renderSidebar(ROLE_ADMIN, `${ROOT}/members`);
    await screen.findByTestId("project-settings-sidebar");
    expect(document.querySelectorAll('[aria-current="page"]')).toHaveLength(0);
  });

  it("reports General as the only current entry at the project settings root", async () => {
    renderSidebar(ROLE_ADMIN, `${ROOT}/`);
    await screen.findByTestId("project-settings-sidebar");
    expect(document.querySelectorAll('[aria-current="page"]')).toHaveLength(1);
    expect(screen.getByTestId("project-settings-tab-general")).toHaveAttribute("aria-current", "page");
  });
});
