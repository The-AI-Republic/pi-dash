// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board header add control (parity/specs/issues.md §5, ISS-036): the
// direct create entry, the cycle/module two-entry menu, and the disabled
// state where creation is off.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { KanbanHeaderCreate } from "./KanbanHeaderCreate.js";

describe("KanbanHeaderCreate", () => {
  it("opens the create modal directly in project context", async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn();
    render(
      <KanbanHeaderCreate
        columnTitle="Todo"
        menu={false}
        disabled={false}
        onCreate={onCreate}
        onAddExisting={vi.fn()}
      />
    );
    await user.click(screen.getByTestId("kanban-header-create"));
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("activates from the keyboard", async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn();
    render(
      <KanbanHeaderCreate
        columnTitle="Todo"
        menu={false}
        disabled={false}
        onCreate={onCreate}
        onAddExisting={vi.fn()}
      />
    );
    screen.getByTestId("kanban-header-create").focus();
    await user.keyboard(" ");
    await user.keyboard("{Enter}");
    expect(onCreate).toHaveBeenCalledTimes(2);
  });

  it("offers create and add-existing in cycle and module context", async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn();
    const onAddExisting = vi.fn();
    render(
      <KanbanHeaderCreate
        columnTitle="Todo"
        menu={true}
        disabled={false}
        onCreate={onCreate}
        onAddExisting={onAddExisting}
      />
    );
    await user.click(screen.getByTestId("kanban-header-create"));
    await user.click(await screen.findByRole("menuitem", { name: "Add an existing work item" }));
    expect(onAddExisting).toHaveBeenCalledTimes(1);
    expect(onCreate).not.toHaveBeenCalled();
  });

  it("keeps menu trigger keys from reaching the header", async () => {
    const user = userEvent.setup();
    const onHeaderKey = vi.fn();
    render(
      <div onKeyDown={onHeaderKey}>
        <KanbanHeaderCreate
          columnTitle="Todo"
          menu={true}
          disabled={false}
          onCreate={vi.fn()}
          onAddExisting={vi.fn()}
        />
      </div>
    );
    const trigger = screen.getByTestId("kanban-header-create");
    trigger.focus();
    await user.keyboard(" ");
    await user.keyboard("{Enter}");
    expect(onHeaderKey).not.toHaveBeenCalled();
    await user.keyboard("a");
    expect(onHeaderKey).toHaveBeenCalledTimes(1);
  });

  it("disables where creation is off", async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn();
    render(
      <KanbanHeaderCreate
        columnTitle="Todo"
        menu={false}
        disabled={true}
        disabledReason="Creation is off in this grouping"
        onCreate={onCreate}
        onAddExisting={vi.fn()}
      />
    );
    const trigger = screen.getByTestId("kanban-header-create");
    expect(trigger).toHaveAttribute("aria-disabled", "true");
    expect(trigger).toHaveAttribute("title", "Creation is off in this grouping");
    await user.click(trigger);
    expect(onCreate).not.toHaveBeenCalled();
  });
});
