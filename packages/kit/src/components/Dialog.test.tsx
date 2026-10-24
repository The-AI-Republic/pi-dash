// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

function setup() {
  return render(
    <Dialog
      trigger={<Button>Edit title</Button>}
      title="Edit title"
      description="Shown in the list"
      actions={<Button variant="primary">Save</Button>}
    >
      <p>Dialog body</p>
    </Dialog>
  );
}

describe("Dialog", () => {
  it("opens from the trigger and traps focus inside", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Edit title" }));
    const dialog = screen.getByRole("dialog", { name: "Edit title" });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByText("Shown in the list")).toBeInTheDocument();
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("closes on Escape and returns focus to the trigger", async () => {
    const user = userEvent.setup();
    setup();
    const trigger = screen.getByRole("button", { name: "Edit title" });
    await user.click(trigger);
    expect(screen.getByRole("dialog", { name: "Edit title" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("closes from the close button", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Edit title" }));
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("focuses the caller's initial control instead of the close button", async () => {
    function Harness(): React.ReactElement {
      const field = React.useRef<HTMLInputElement>(null);
      return (
        <Dialog title="Rename" open onOpenChange={() => undefined} initialFocus={field}>
          <input ref={field} aria-label="Name" />
        </Dialog>
      );
    }
    render(<Harness />);
    await waitFor(() => expect(screen.getByLabelText("Name")).toHaveFocus());
  });

  it("moves no focus on open when the caller passes initialFocus={false}", async () => {
    render(
      <Dialog title="Rename" open onOpenChange={() => undefined} initialFocus={false}>
        <input aria-label="Name" />
      </Dialog>
    );
    expect(screen.getByLabelText("Name")).toBeInTheDocument();
    // Past Base UI's deferred initial-focus pass: focus stays put.
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(document.activeElement).toBe(document.body);
  });

  it("leaves the header close control out when the caller orders its own actions", () => {
    render(
      <Dialog title="Key created" open onOpenChange={() => undefined} closeButton={false}>
        <p>Secret</p>
      </Dialog>
    );
    expect(screen.getByRole("dialog", { name: "Key created" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close dialog" })).toBeNull();
  });

  it("uses the default body spacing below the title block", () => {
    render(
      <Dialog title="Confirm" open onOpenChange={() => undefined}>
        <p>Body copy</p>
      </Dialog>
    );
    const body = screen.getByText("Body copy");
    expect(body.parentElement).toHaveClass("mt-(--space-8)");
  });

  it("contains tall bodies inside the viewport with panel scroll", () => {
    // PSET-056: without these the scheduler install form clips past the
    // viewport with body scroll locked and its tabs unreachable.
    render(
      <Dialog title="Confirm" open onOpenChange={() => undefined}>
        <p>Body copy</p>
      </Dialog>
    );
    const dialog = screen.getByRole("dialog", { name: "Confirm" });
    expect(dialog).toHaveClass("max-h-[90vh]");
    expect(dialog).toHaveClass("overflow-y-auto");
  });

  it("renders its title as the shared dialog heading style", () => {
    render(
      <Dialog title="Confirm" open onOpenChange={() => undefined}>
        <p>Body copy</p>
      </Dialog>
    );
    expect(screen.getByRole("heading", { name: "Confirm" })).toHaveClass("text-h4-medium");
  });

  it("accepts a body class override so the copy can sit tight below the title", () => {
    render(
      <Dialog title="Confirm" open onOpenChange={() => undefined} bodyClassName="mt-(--space-4)">
        <p>Body copy</p>
      </Dialog>
    );
    const body = screen.getByText("Body copy");
    expect(body.parentElement).toHaveClass("mt-(--space-4)");
    expect(body.parentElement).not.toHaveClass("mt-(--space-8)");
  });

  it("ignores Escape but still closes from the close button when controlled", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    let open = true;
    const { rerender } = render(
      <Dialog
        title="Updates"
        open={open}
        onOpenChange={(next, details) => {
          onOpenChange(next, details);
          open = next;
        }}
        ignoreEscapeKey
      >
        <p>Dialog body</p>
      </Dialog>
    );
    const sync = () =>
      rerender(
        <Dialog
          title="Updates"
          open={open}
          onOpenChange={(next, details) => {
            onOpenChange(next, details);
            open = next;
          }}
          ignoreEscapeKey
        >
          <p>Dialog body</p>
        </Dialog>
      );
    expect(screen.getByRole("dialog", { name: "Updates" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Updates" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(onOpenChange).toHaveBeenCalledWith(false, expect.anything());
    sync();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("ignores Escape and the backdrop while non-dismissable", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    render(
      <Dialog title="Saving" open onOpenChange={onOpenChange} nonDismissable>
        <p>Working</p>
      </Dialog>
    );
    await user.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Saving" })).toBeInTheDocument();
    await user.click(document.querySelector("[data-base-ui-backdrop]") ?? document.body);
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Saving" })).toBeInTheDocument();
  });
});
