// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { state as row, testRowId } from "../testFixtures.js";
import { edgeForPointer, StateRow } from "./StateRow.js";

afterEach(() => {
  cleanup();
});

function renderRow(props: Partial<React.ComponentProps<typeof StateRow>> = {}) {
  const handlers = {
    onDragStart: vi.fn(),
    onDragEnd: vi.fn(),
    onDragOverEdge: vi.fn(),
    onDragLeaveRow: vi.fn(),
    onDrop: vi.fn(),
    onEdit: vi.fn(),
    onPromote: vi.fn(),
    onDelete: vi.fn(),
  };
  const tree = render(
    <StateRow
      state={row({ name: "In progress", group: "started", description: "Work under way" })}
      group="started"
      share={0.5}
      editable
      draggable
      dropEnabled
      deleteBlockedReason={null}
      promoting={false}
      dragging={false}
      {...handlers}
      {...props}
    />
  );
  return { ...handlers, ...tree };
}

/** A rect whose middle the event coordinates can be placed against. */
function rect(top: number, height: number): DOMRect {
  return {
    top,
    bottom: top + height,
    height,
    left: 0,
    right: 100,
    width: 100,
    x: 0,
    y: top,
    toJSON: () => ({}),
  } as DOMRect;
}

describe("StateRow", () => {
  it("shows the name, description, share ring and the group glyph", () => {
    renderRow();
    expect(screen.getByRole("heading", { name: "In progress" })).toBeInTheDocument();
    expect(screen.getByText("Work under way")).toBeInTheDocument();
    expect(screen.getByTestId("state-group-glyph")).toHaveAttribute("data-share", "0.50");
  });

  it("omits an empty description line", () => {
    renderRow({ state: row({ name: "Empty", group: "test", description: "" }) });
    expect(screen.queryByText("Work under way")).not.toBeInTheDocument();
  });

  it("clamps a nonsensical share into the ring range", () => {
    const { rerender } = renderRow({ share: 2 });
    expect(screen.getByTestId("state-group-glyph")).toHaveAttribute("data-share", "1.00");
    rerender(
      <StateRow
        state={row({ name: "In progress", group: "started" })}
        group="started"
        share={-1}
        editable
        draggable
        dropEnabled
        deleteBlockedReason={null}
        promoting={false}
        dragging={false}
        onDragStart={vi.fn()}
        onDragEnd={vi.fn()}
        onDragOverEdge={vi.fn()}
        onDragLeaveRow={vi.fn()}
        onDrop={vi.fn()}
        onEdit={vi.fn()}
        onPromote={vi.fn()}
        onDelete={vi.fn()}
      />
    );
    expect(screen.getByTestId("state-group-glyph")).toHaveAttribute("data-share", "0.00");
  });

  it("gives readers no grip, edit, delete or promote affordances", () => {
    renderRow({ editable: false, draggable: false });
    expect(screen.queryByTestId("state-row-grip")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Edit/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Delete/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Make In progress the default/ })).not.toBeInTheDocument();
  });

  it("withholds the grip when the group cannot be dragged", () => {
    renderRow({ draggable: false });
    expect(screen.queryByTestId("state-row-grip")).not.toBeInTheDocument();
  });

  it("starts a drag with the row id and reports the end", async () => {
    const user = userEvent.setup();
    const { onDragStart, onDragEnd } = renderRow();
    const grip = screen.getByTestId("state-row-grip");
    const data = { setData: vi.fn(), effectAllowed: "" } as unknown as DataTransfer;
    fireEvent.dragStart(grip, { dataTransfer: data });
    expect(data.setData).toHaveBeenCalledWith("text/plain", testRowId("In progress"));
    expect(onDragStart).toHaveBeenCalledTimes(1);
    fireEvent.dragEnd(grip);
    expect(onDragEnd).toHaveBeenCalledTimes(1);
    await user.tab();
  });

  it("names the edge above or below the row's middle", () => {
    expect(edgeForPointer(100, 40, 105)).toBe("above");
    expect(edgeForPointer(100, 40, 119)).toBe("above");
    expect(edgeForPointer(100, 40, 120)).toBe("below");
    expect(edgeForPointer(100, 40, 135)).toBe("below");
  });

  it("wires drag-over, leave and drop to its handlers", () => {
    const { onDragOverEdge, onDrop, onDragLeaveRow } = renderRow();
    const target = screen.getByTestId("state-row");
    vi.spyOn(target, "getBoundingClientRect").mockReturnValue(rect(100, 40));
    // jsdom drops init coordinates on drag events, so the handlers see a
    // zero pointer and resolve the lower half; the edge math itself is
    // pinned by edgeForPointer above.
    fireEvent.dragOver(target);
    expect(onDragOverEdge).toHaveBeenLastCalledWith("below");
    fireEvent.dragLeave(target);
    expect(onDragLeaveRow).toHaveBeenCalledTimes(1);
    fireEvent.drop(target);
    expect(onDrop).toHaveBeenCalledWith("below");
  });

  it("dims while it is the drag source", () => {
    renderRow({ dragging: true });
    expect(screen.getByTestId("state-row")).toHaveAttribute("data-dragging", "true");
    expect(screen.getByTestId("state-row").className).toContain("opacity-50");
  });

  it("offers edit, promote and delete to editors", async () => {
    const user = userEvent.setup();
    const { onEdit, onPromote, onDelete } = renderRow();
    await user.click(screen.getByRole("button", { name: "Edit In progress" }));
    await user.click(screen.getByRole("button", { name: "Make In progress the default state" }));
    await user.click(screen.getByRole("button", { name: "Delete In progress" }));
    expect(onEdit).toHaveBeenCalledTimes(1);
    expect(onPromote).toHaveBeenCalledTimes(1);
    expect(onDelete).toHaveBeenCalledTimes(1);
  });

  it("disables promote while a promotion is flying", () => {
    renderRow({ promoting: true });
    expect(screen.getByRole("button", { name: "Make In progress the default state" })).toBeDisabled();
  });

  it("shows a static default marker instead of the promote action", () => {
    renderRow({ state: row({ name: "In progress", group: "started", default: true }) });
    expect(screen.getByTestId("state-default-marker")).toHaveTextContent("Default");
    expect(screen.queryByRole("button", { name: /the default state/ })).not.toBeInTheDocument();
  });

  it("disables delete with the explanation when the row is protected", async () => {
    const user = userEvent.setup();
    const { onDelete } = renderRow({ deleteBlockedReason: "The default state can't be deleted" });
    expect(screen.getByText("The default state can't be deleted")).toBeInTheDocument();
    const button = screen.getByRole("button", { name: /Delete In progress/ });
    expect(button).toBeDisabled();
    await user.click(button);
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("does not run drag handlers on a row when editing is off", () => {
    const { onDragOverEdge, onDrop } = renderRow({ draggable: false, dropEnabled: false });
    const target = screen.getByTestId("state-row");
    const NativeDrag = globalThis.DragEvent;
    class CoordDragEvent extends MouseEvent {}
    globalThis.DragEvent = CoordDragEvent as unknown as typeof DragEvent;
    try {
      fireEvent.dragOver(target, { clientY: 1 });
      fireEvent.drop(target, { clientY: 1 });
    } finally {
      globalThis.DragEvent = NativeDrag;
    }
    expect(onDragOverEdge).not.toHaveBeenCalled();
    expect(onDrop).not.toHaveBeenCalled();
  });

  it("accepts a drop into a one-row group that offers no grip", () => {
    const { onDragOverEdge, onDrop } = renderRow({ draggable: false });
    const target = screen.getByTestId("state-row");
    fireEvent.dragOver(target);
    fireEvent.drop(target);
    expect(onDragOverEdge).toHaveBeenCalledTimes(1);
    expect(onDrop).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("state-row-grip")).not.toBeInTheDocument();
  });
});
