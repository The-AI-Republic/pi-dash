// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

import { ListPeek } from "./ListPeek.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-42661417400a";

describe("ListPeek", () => {
  it("shows the key and title and closes from the control", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<ListPeek issueKey="WEB-42" title="Fix login" onClose={onClose} />);
    expect(screen.getByLabelText("Work item preview")).toBeInTheDocument();
    expect(screen.getByText("WEB-42")).toBeInTheDocument();
    expect(screen.getByLabelText("Work item title")).toHaveValue("Fix login");
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape and ignores other keys", () => {
    const onClose = vi.fn();
    render(<ListPeek issueKey="WEB-42" title="Fix login" onClose={onClose} />);
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps its own copy-link button in the header", () => {
    render(<ListPeek issueKey="WEB-42" title="Fix login" onClose={vi.fn()} workspaceSlug="acme" />);
    expect(screen.getByRole("button", { name: "Copy work item link" })).toBeInTheDocument();
    // Without a resolved row there is no menu target, so no header menu.
    expect(screen.queryByRole("button", { name: "Actions for WEB-42" })).not.toBeInTheDocument();
  });

  it("mounts the detail-style menu once the row resolves (ISS-066)", async () => {
    createAppClient("https://api.test", (() => Promise.reject(new Error("unexpected network"))) as never);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <ListPeek
          issueKey="WEB-42"
          title="Fix login"
          onClose={vi.fn()}
          workspaceSlug="acme"
          projectId={PROJECT}
          projectIdentifier="WEB"
          canWrite
          issue={{
            id: ISSUE,
            name: "Fix login",
            state_id: null,
            sort_order: 1,
            completed_at: null,
            priority: "none",
            sequence_id: 42,
            project_id: PROJECT,
            parent_id: null,
            cycle_id: null,
            module_ids: [],
            label_ids: [],
            assignee_ids: [],
            sub_issues_count: null,
            attachment_count: null,
            link_count: null,
            created_at: "2026-03-01T00:00:00Z",
            updated_at: "2026-03-02T00:00:00Z",
            created_by: null,
            updated_by: null,
            is_draft: false,
            archived_at: null,
          }}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Actions for WEB-42" }));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.querySelector("[data-menu-title]")?.textContent ?? item.textContent)).toEqual([
      "Make a copy",
      "Open in new tab",
      "Move to project",
      "Archive",
      "Delete",
    ]);
    resetAppClient();
  });

  it("removes the key listener on unmount", () => {
    const onClose = vi.fn();
    const { unmount } = render(<ListPeek issueKey="WEB-42" title="Fix login" onClose={onClose} />);
    unmount();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });
});
