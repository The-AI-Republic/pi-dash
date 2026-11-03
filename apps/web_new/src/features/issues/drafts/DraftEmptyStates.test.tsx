// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Drafts first-run screens (parity/specs/issues.md §12, ISS-141).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  DraftsEmptyState,
  DraftsNoProjectState,
  NO_DRAFTS_ACTION,
  NO_DRAFTS_TITLE,
  NO_PROJECT_ACTION,
  NO_PROJECT_TITLE,
} from "./DraftEmptyStates.js";

describe("DraftsEmptyState", () => {
  it("invites the first draft", () => {
    const onCreate = vi.fn();
    render(<DraftsEmptyState canCreate onCreate={onCreate} />);
    expect(screen.getByText(NO_DRAFTS_TITLE)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: NO_DRAFTS_ACTION }));
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("keeps the action visible but disabled for a guest", () => {
    render(<DraftsEmptyState canCreate={false} onCreate={vi.fn()} />);
    expect(screen.getByRole("button", { name: NO_DRAFTS_ACTION })).toBeDisabled();
  });
});

describe("DraftsNoProjectState", () => {
  it("sends the viewer to create a project", () => {
    const onCreateProject = vi.fn();
    render(<DraftsNoProjectState canCreate onCreateProject={onCreateProject} />);
    expect(screen.getByText(NO_PROJECT_TITLE)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: NO_PROJECT_ACTION }));
    expect(onCreateProject).toHaveBeenCalledTimes(1);
  });

  it("disables the project action for a guest", () => {
    render(<DraftsNoProjectState canCreate={false} onCreateProject={vi.fn()} />);
    expect(screen.getByRole("button", { name: NO_PROJECT_ACTION })).toBeDisabled();
  });
});
