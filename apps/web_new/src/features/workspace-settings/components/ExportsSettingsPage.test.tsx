// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { registerWorkspaceExportsGuide, resetWorkspaceExportsGuide } from "../lib/exportsGuide.js";
import {
  EXPORTS_GUIDE_PENDING_TITLE,
  EXPORTS_PAGE_DESCRIPTION,
  EXPORTS_PAGE_TITLE,
  ExportsSettingsPage,
} from "./ExportsSettingsPage.js";

afterEach(() => {
  cleanup();
  resetWorkspaceExportsGuide();
});

describe("ExportsSettingsPage", () => {
  it("explains the exports purpose above its guide", () => {
    render(<ExportsSettingsPage workspaceSlug="acme" />);
    expect(screen.getByTestId("workspace-exports-settings")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: EXPORTS_PAGE_TITLE })).toBeInTheDocument();
    expect(screen.getByText(EXPORTS_PAGE_DESCRIPTION)).toBeInTheDocument();
  });

  it("shows the quiet guide-pending state before the exports area registers", () => {
    render(<ExportsSettingsPage workspaceSlug="acme" />);
    expect(screen.getByTestId("workspace-exports-guide-pending")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: EXPORTS_GUIDE_PENDING_TITLE })).toBeInTheDocument();
    expect(screen.queryAllByRole("button")).toHaveLength(0);
    expect(screen.queryAllByRole("textbox")).toHaveLength(0);
  });

  it("mounts a registered guide through the seam with the workspace slug", () => {
    render(<ExportsSettingsPage workspaceSlug="acme" />);
    act(() => {
      registerWorkspaceExportsGuide(({ workspaceSlug }) => <p>{`export guide for ${workspaceSlug}`}</p>);
    });
    expect(screen.getByText("export guide for acme")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-exports-guide-pending")).not.toBeInTheDocument();
  });

  it("returns to the pending state when the guide unmounts", () => {
    render(<ExportsSettingsPage workspaceSlug="acme" />);
    let unregister = (): void => undefined;
    act(() => {
      unregister = registerWorkspaceExportsGuide(() => <p>live guide</p>);
    });
    expect(screen.getByText("live guide")).toBeInTheDocument();
    act(() => {
      unregister();
    });
    expect(screen.getByTestId("workspace-exports-guide-pending")).toBeInTheDocument();
  });
});
