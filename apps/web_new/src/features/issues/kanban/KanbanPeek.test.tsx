// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board peek (parity/specs/issues.md §5, ISS-034): the issue key above the
// editable title, the rename write and its read-only gate, the copy-link
// action, the full work-item link and Escape.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  patchIssue: vi.fn(),
  copy: vi.fn(),
}));

vi.mock("../list/mutations.js", () => ({
  useListIssuePatch: () => ({ isSaving: false, patchIssue: mocks.patchIssue }),
}));
vi.mock("../../../shared/shell/shareLink.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/shareLink.js")>()),
  copyWorkItemLink: mocks.copy,
}));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  Link: (props: { to: string; params: Record<string, string>; children: React.ReactNode }) => (
    <a data-testid="router-link" data-to={props.to} data-params={JSON.stringify(props.params)}>
      {props.children}
    </a>
  ),
}));

import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { copyCardLink, CARD_COPY_COPIED, CARD_COPY_FAILED, KanbanPeek, peekKeyOf } from "./KanbanPeek.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function setup(overrides: Partial<React.ComponentProps<typeof KanbanPeek>> = {}) {
  const onClose = vi.fn();
  const props: React.ComponentProps<typeof KanbanPeek> = {
    workspaceSlug: "acme",
    projectId: PROJECT,
    projectIdentifier: "WEB",
    issue: { id: ISSUE, name: "Fix login", sequence_id: 42 },
    canRename: true,
    onClose,
    ...overrides,
  };
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  render(<KanbanPeek {...props} />);
  return { onClose };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.copy.mockResolvedValue(true);
});

describe("peekKeyOf", () => {
  it("joins the project code with the sequence", () => {
    expect(peekKeyOf("WEB", { sequence_id: 7 })).toBe("WEB-7");
  });
});

describe("copyCardLink", () => {
  it("copies the item path and passes both notices", async () => {
    await copyCardLink("acme", "WEB-7");
    expect(mocks.copy).toHaveBeenCalledWith(
      "/acme/browse/WEB-7/",
      expect.objectContaining({ copied: CARD_COPY_COPIED, failed: CARD_COPY_FAILED })
    );
  });
});

describe("KanbanPeek", () => {
  it("shows the issue key, the title and the full-item link", () => {
    setup();
    expect(screen.getByTestId("kanban-peek-key")).toHaveTextContent("WEB-42");
    expect(screen.getByLabelText("Work item title", { exact: true })).toHaveValue("Fix login");
    const link = screen.getByTestId("router-link");
    expect(link).toHaveAttribute("data-to", "/$ws/browse/$issueKey");
    expect(JSON.parse(link.getAttribute("data-params") ?? "{}")).toEqual({ ws: "acme", issueKey: "WEB-42" });
  });

  it("renames through the shared issue patch when the title changes", async () => {
    const user = userEvent.setup();
    setup();
    const field = screen.getByLabelText("Work item title", { exact: true });
    await user.clear(field);
    await user.type(field, "Fix logout{enter}");
    await waitFor(() => expect(mocks.patchIssue).toHaveBeenCalledWith({ name: "Fix logout" }));
  });

  it("offers no rename for read-only rows", async () => {
    const user = userEvent.setup();
    setup({ canRename: false });
    const field = screen.getByLabelText("Work item title", { exact: true });
    await user.type(field, " changed{enter}");
    expect(mocks.patchIssue).not.toHaveBeenCalled();
  });

  it("copies the card link from the peek header", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Copy work item link" }));
    expect(mocks.copy).toHaveBeenCalledWith("/acme/browse/WEB-42/", expect.any(Object));
  });

  it("closes on Escape and on the close control", async () => {
    const user = userEvent.setup();
    const { onClose } = setup();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
