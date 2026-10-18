// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ProjectDetails, State } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ARCHIVE_ROW_TITLE, AutomationRows, CLOSE_ROW_TITLE } from "./AutomationRows.js";

function row(): ProjectDetails {
  return { archive_in: 0, close_in: 0, default_state: null } as ProjectDetails;
}

function cancelled(id: string, name: string): State {
  return {
    id,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name,
    color: "#6b7280",
    group: "cancelled",
    default: false,
    description: "",
    sequence: 0,
  };
}

describe("AutomationRows", () => {
  it("paints both headings before the project row resolves", () => {
    render(<AutomationRows project={undefined} states={[]} onChange={vi.fn()} />);
    expect(screen.getByRole("heading", { level: 4, name: ARCHIVE_ROW_TITLE })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 4, name: CLOSE_ROW_TITLE })).toBeInTheDocument();
  });

  it("renders both detail panels once the project row exists", () => {
    render(
      <AutomationRows
        project={row()}
        states={[cancelled("123e4567-e89b-12d3-a456-42661417400a", "Dropped")]}
        onChange={vi.fn()}
      />
    );
    expect(screen.queryByRole("status")).toBeNull();
  });
});
