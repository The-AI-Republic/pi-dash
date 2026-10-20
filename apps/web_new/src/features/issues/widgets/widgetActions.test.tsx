// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The action-row seam (parity/specs/issues.md §15, ISS-173/180/185): the
// relation menu offers the four kinds, the link button opens the link
// dialog, both stay disabled for a read-only viewer, and neither renders
// outside the provider that owns the dialog state.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { LinkActionButton, RelationActionMenu, WidgetActionsProvider, useWidgetActions } from "./widgetActions.js";

function Probe(): React.ReactElement {
  const actions = useWidgetActions();
  return (
    <button type="button" onClick={() => actions.setRelationKind("blocked_by")}>
      {actions.relationKind ?? "none"}
    </button>
  );
}

describe("widget action row", () => {
  it("offers the four relation kinds and records the chosen one", async () => {
    const user = userEvent.setup();
    render(
      <WidgetActionsProvider>
        <RelationActionMenu canEdit />
        <Probe />
      </WidgetActionsProvider>
    );
    await user.click(screen.getByRole("button", { name: "Add relation" }));
    for (const label of ["Relates to", "Duplicate of", "Blocked by", "Blocking"]) {
      expect(await screen.findByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    await user.click(screen.getByRole("menuitem", { name: "Blocking" }));
    expect(screen.getByRole("button", { name: "blocking" })).toBeInTheDocument();
  });

  it("opens the link dialog through the shared state", async () => {
    const user = userEvent.setup();
    function LinkProbe(): React.ReactElement {
      const actions = useWidgetActions();
      return <span>{actions.linkOpen ? "open" : "closed"}</span>;
    }
    render(
      <WidgetActionsProvider>
        <LinkActionButton canEdit />
        <LinkProbe />
      </WidgetActionsProvider>
    );
    expect(screen.getByText("closed")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(screen.getByText("open")).toBeInTheDocument();
  });

  it("disables both entries for a read-only viewer", () => {
    render(
      <WidgetActionsProvider>
        <RelationActionMenu canEdit={false} />
        <LinkActionButton canEdit={false} />
      </WidgetActionsProvider>
    );
    expect(screen.getByRole("button", { name: "Add relation" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add link" })).toBeDisabled();
  });

  it("refuses to read the shared state outside its provider", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(() => render(<Probe />)).toThrow(/WidgetActionsProvider/);
    consoleError.mockRestore();
  });
});
