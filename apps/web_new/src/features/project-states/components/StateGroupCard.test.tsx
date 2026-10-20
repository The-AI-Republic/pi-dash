// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { stateList } from "../testFixtures.js";
import { StateGroupCard, type StateGroupCardProps } from "./StateGroupCard.js";

afterEach(() => {
  cleanup();
});

const TWO_ROWS = stateList([
  { name: "Alpha", group: "backlog", sequence: 10 },
  { name: "Beta", group: "backlog", sequence: 20 },
]);

function box(top: number): DOMRect {
  return {
    top,
    bottom: top + 40,
    height: 40,
    left: 0,
    right: 100,
    width: 100,
    x: 0,
    y: top,
    toJSON: () => ({}),
  } as DOMRect;
}

function renderCard(props: Partial<StateGroupCardProps> = {}) {
  const handlers = {
    onToggle: vi.fn(),
    onOpenCreate: vi.fn(),
    onCancelCreate: vi.fn(),
    onCreate: vi.fn(),
    onEdit: vi.fn(),
    onCancelEdit: vi.fn(),
    onSaveEdit: vi.fn(),
    onDelete: vi.fn(),
    onPromote: vi.fn(),
    onMove: vi.fn(),
  };
  render(
    <StateGroupCard
      group="backlog"
      label="Backlog"
      rows={TWO_ROWS}
      expanded
      editable
      creating={false}
      createPending={false}
      editingId={null}
      editPending={false}
      deleteBlockedReason={() => null}
      promotingId={null}
      {...handlers}
      {...props}
    />
  );
  return handlers;
}

describe("StateGroupCard header", () => {
  it("names the group, counts its rows and toggles open state", async () => {
    const user = userEvent.setup();
    const { onToggle } = renderCard();
    const toggle = screen.getByTestId("state-group-toggle");
    expect(toggle).toHaveTextContent("Backlog");
    expect(toggle).toHaveTextContent("2");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("hides the body when collapsed", () => {
    renderCard({ expanded: false });
    expect(screen.getByTestId("state-group-toggle")).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("state-group-body")).not.toBeInTheDocument();
    expect(screen.queryByTestId("state-row")).not.toBeInTheDocument();
  });

  it("opens creation from the header and disables the control while the form shows", async () => {
    const user = userEvent.setup();
    const { onOpenCreate } = renderCard();
    await user.click(screen.getByRole("button", { name: "Add a state to Backlog" }));
    expect(onOpenCreate).toHaveBeenCalledTimes(1);

    cleanup();
    renderCard({ creating: true });
    expect(screen.getByRole("button", { name: "Add a state to Backlog" })).toBeDisabled();
  });

  it("disables creation for readers", () => {
    renderCard({ editable: false });
    expect(screen.getByRole("button", { name: "Add a state to Backlog" })).toBeDisabled();
  });
});

describe("state rows", () => {
  it("offers a grip on a multi-row group and withholds it on a lone row", () => {
    renderCard();
    expect(screen.getAllByTestId("state-row-grip")).toHaveLength(2);

    cleanup();
    renderCard({ rows: [TWO_ROWS[0]!] });
    expect(screen.queryByTestId("state-row-grip")).not.toBeInTheDocument();
  });

  it("swaps the edited row for the prefilled form", () => {
    renderCard({ editingId: TWO_ROWS[0]!.id });
    const form = screen.getByRole("form", { name: "Edit state Alpha" });
    expect(within(form).getByLabelText("State name")).toHaveValue("Alpha");
    expect(screen.getAllByTestId("state-row")).toHaveLength(1);
  });

  it("keeps the edit form's save and cancel wired to the card", async () => {
    const user = userEvent.setup();
    const { onSaveEdit, onCancelEdit } = renderCard({ editingId: TWO_ROWS[0]!.id });
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    expect(onSaveEdit).toHaveBeenCalledWith(TWO_ROWS[0], {
      name: "Alpha",
      description: "",
      color: "#000000",
    });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancelEdit).toHaveBeenCalledTimes(1);
  });

  it("docks the create form with the group's swatch and closes through the card", async () => {
    const user = userEvent.setup();
    const { onCreate, onCancelCreate } = renderCard({ creating: true });
    const form = screen.getByRole("form", { name: "Create state in Backlog" });
    expect(within(form).getByRole("button", { name: "Pick a state color" })).toHaveStyle({
      backgroundColor: "#9CA3AF",
    });
    await user.type(within(form).getByLabelText("State name"), "New");
    await user.click(within(form).getByRole("button", { name: "Create state" }));
    expect(onCreate).toHaveBeenCalledWith({ name: "New", description: "", color: "#9CA3AF" });
    await user.click(within(form).getByRole("button", { name: "Cancel" }));
    expect(onCancelCreate).toHaveBeenCalledTimes(1);
  });
});

describe("empty groups", () => {
  it("explains the empty group and invites editors to create", async () => {
    const user = userEvent.setup();
    const { onOpenCreate } = renderCard({ rows: [] });
    expect(screen.getByTestId("state-group-empty")).toHaveTextContent("No states available for the Backlog group.");
    expect(screen.getByTestId("state-group-empty")).toHaveTextContent("Please create a new state.");
    await user.click(screen.getByRole("button", { name: "Create a state" }));
    expect(onOpenCreate).toHaveBeenCalledTimes(1);
  });

  it("drops the create prompt for readers", () => {
    renderCard({ rows: [], editable: false });
    expect(screen.getByTestId("state-group-empty")).toHaveTextContent("No states available for the Backlog group.");
    expect(screen.queryByText("Please create a new state.")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create a state" })).not.toBeInTheDocument();
  });

  it("hides the empty note while the create form is open", () => {
    renderCard({ rows: [], creating: true });
    expect(screen.queryByTestId("state-group-empty")).not.toBeInTheDocument();
  });
});

describe("dragging", () => {
  it("lights the marker for the hovered edge and commits the drop", () => {
    const { onMove } = renderCard();
    const rows = screen.getAllByTestId("state-row");
    const source = screen.getAllByTestId("state-row-grip")[0]!;
    fireEvent.dragStart(source, { dataTransfer: { setData: vi.fn(), effectAllowed: "" } });
    expect(rows[0]).toHaveAttribute("data-dragging", "true");
    fireEvent.dragOver(rows[1]!);
    expect(screen.getAllByTestId("state-drop-marker")).toHaveLength(1);
    fireEvent.drop(rows[1]!);
    expect(onMove).toHaveBeenCalledWith(TWO_ROWS[0]!.id, TWO_ROWS[1]!.id, "below");
    expect(screen.queryAllByTestId("state-drop-marker")).toHaveLength(0);
  });

  it("lights the marker above a hovered row", () => {
    renderCard();
    const rows = screen.getAllByTestId("state-row");
    fireEvent.dragStart(screen.getAllByTestId("state-row-grip")[0]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    vi.spyOn(rows[1]!, "getBoundingClientRect").mockReturnValue(box(100));
    const NativeDrag = globalThis.DragEvent;
    // jsdom's DragEvent drops the MouseEvent coordinates on drag events;
    // a MouseEvent subclass keeps them, so the edge resolves for real.
    class CoordDragEvent extends MouseEvent {}
    globalThis.DragEvent = CoordDragEvent as unknown as typeof DragEvent;
    try {
      fireEvent.dragOver(rows[1]!, { clientY: 105 });
      expect(screen.getAllByTestId("state-drop-marker")[0]).toBe(rows[1]!.previousSibling);
    } finally {
      globalThis.DragEvent = NativeDrag;
    }
  });

  it("does not commit a drop onto the row being dragged", () => {
    const { onMove } = renderCard();
    const rows = screen.getAllByTestId("state-row");
    fireEvent.dragStart(screen.getAllByTestId("state-row-grip")[0]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.drop(rows[0]!);
    expect(onMove).not.toHaveBeenCalled();
  });

  it("clears the drag when it ends without a drop", () => {
    renderCard();
    const grip = screen.getAllByTestId("state-row-grip")[0]!;
    fireEvent.dragStart(grip, { dataTransfer: { setData: vi.fn(), effectAllowed: "" } });
    fireEvent.dragEnd(grip);
    expect(screen.getAllByTestId("state-row")[0]).toHaveAttribute("data-dragging", "false");
  });

  it("clears a hover marker when the pointer leaves that row", () => {
    renderCard();
    const rows = screen.getAllByTestId("state-row");
    fireEvent.dragStart(screen.getAllByTestId("state-row-grip")[0]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.dragOver(rows[1]!);
    expect(screen.getAllByTestId("state-drop-marker")).toHaveLength(1);
    fireEvent.dragLeave(rows[1]!);
    expect(screen.queryAllByTestId("state-drop-marker")).toHaveLength(0);
  });

  it("leaves an unrelated hover marker alone when another row is left", () => {
    renderCard();
    const rows = screen.getAllByTestId("state-row");
    fireEvent.dragStart(screen.getAllByTestId("state-row-grip")[0]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.dragOver(rows[1]!);
    fireEvent.dragLeave(rows[0]!);
    expect(screen.getAllByTestId("state-drop-marker")).toHaveLength(1);
  });
});
