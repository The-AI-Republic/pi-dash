// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings rail marker against the real router (PSET-001/PSET-003). The
// router writes its own `aria-current` for fuzzy matches, so the doubled
// marker on a sub-tab is only visible with the real Link; this suite pins
// exactly one current entry on the canonical slash form, none on the
// equivalent no-slash form (NEWFRONT-396) and one on General.
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import { renderInRouter } from "../testing.js";
import { ProjectSettingsSidebar } from "./ProjectSettingsSidebar.js";

const WS = "acme";
const PID = "223e4567-e89b-12d3-a456-426614174001";
const ROOT = `/${WS}/settings/projects/${PID}`;

function renderAt(address: string): void {
  render(
    renderInRouter(
      address,
      <ProjectSettingsSidebar
        workspaceSlug={WS}
        projectId={PID}
        projectName="Website relaunch"
        logoProps={{ in_use: "emoji", emoji: { value: "🚀" } }}
        identifier="WEB"
        role={ROLE_ADMIN}
        pathname={address}
      />
    )
  );
}

function currentEntries(): string[] {
  return [...document.querySelectorAll('[aria-current="page"]')].map((node) => node.textContent ?? "");
}

describe("ProjectSettingsSidebar current entry", () => {
  it("marks only the current sub-tab on the canonical slash form", async () => {
    renderAt(`${ROOT}/members/`);
    await waitFor(() =>
      expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("data-active", "true")
    );
    await waitFor(() => expect(currentEntries()).toEqual(["Members"]));
  });

  it("marks General alone on its canonical address", async () => {
    renderAt(`${ROOT}/`);
    await waitFor(() => expect(currentEntries()).toEqual(["General"]));
  });

  it("marks nothing on the no-slash deep link", async () => {
    renderAt(`${ROOT}/members`);
    await waitFor(() =>
      expect(screen.getByTestId("project-settings-tab-members")).toHaveAttribute("data-active", "false")
    );
    expect(currentEntries()).toEqual([]);
  });
});
