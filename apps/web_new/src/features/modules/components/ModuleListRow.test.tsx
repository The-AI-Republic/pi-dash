// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One list row (parity/specs/modules.md §3.5, MOD-012): the progress marker
// (check, alert or percentage), the name as a real anchor to the detail
// page, the info control that opens the peek, and the archived mode where a
// press on the row opens the peek instead of leaving the screen.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className, ...rest }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className} {...rest}>
      {children}
    </a>
  ),
}));

import { MODULES_MODULE_ID, MODULES_PROJECT_ID, makeModuleRow } from "../testing.js";
import { ModuleListRow } from "./ModuleListRow.js";

describe("ModuleListRow", () => {
  it("links the name to the module detail page", () => {
    render(<ModuleListRow workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />);
    const link = screen.getByRole("link", { name: "Delivery" });
    expect(link).toHaveAttribute("href", `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`);
    expect(screen.getByTestId("modules-row-name")).toHaveClass("truncate");
  });

  it("carries the module identity on the row", () => {
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ status: "in-progress" })}
      />
    );
    const row = screen.getByTestId("modules-row");
    expect(row).toHaveAttribute("data-module-id", MODULES_MODULE_ID);
    expect(row).toHaveAttribute("data-module-name", "Delivery");
    expect(row).toHaveAttribute("data-module-status", "in-progress");
    expect(row).toHaveAttribute("data-module-archived", "false");
  });

  it("fills the ring with the floored percentage while work remains", () => {
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ total_issues: 4, completed_issues: 1 })}
      />
    );
    const progress = screen.getByTestId("modules-row-progress");
    expect(progress).toHaveAttribute("data-progress-kind", "percent");
    expect(progress).toHaveAttribute("data-progress-percent", "25");
    expect(progress).toHaveTextContent("25%");
    expect(within(progress).getByRole("img", { name: "25% complete" })).toBeInTheDocument();
  });

  it("shows a check once everything is done", () => {
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ total_issues: 2, completed_issues: 2 })}
      />
    );
    const progress = screen.getByTestId("modules-row-progress");
    expect(progress).toHaveAttribute("data-progress-kind", "check");
    expect(progress).not.toHaveAttribute("data-progress-percent");
    expect(within(progress).getByRole("img", { name: "All work items done" })).toBeInTheDocument();
  });

  it("shows the alert for a finished module below full completion", () => {
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ status: "completed", total_issues: 2, completed_issues: 1 })}
      />
    );
    const progress = screen.getByTestId("modules-row-progress");
    expect(progress).toHaveAttribute("data-progress-kind", "alert");
    expect(within(progress).getByRole("img", { name: "50% complete while finished" })).toBeInTheDocument();
  });

  it("keeps the percentage for a cancelled module", () => {
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ status: "cancelled", total_issues: 2 })}
      />
    );
    expect(screen.getByTestId("modules-row-progress")).toHaveAttribute("data-progress-kind", "percent");
  });

  it("opens the peek from the info control without following the link", () => {
    const onOpenPeek = vi.fn();
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        onOpenPeek={onOpenPeek}
      />
    );
    const control = screen.getByRole("button", { name: "Open preview of Delivery" });
    expect(control).toHaveAttribute("data-testid", "modules-row-peek");
    fireEvent.click(control);
    expect(onOpenPeek).toHaveBeenCalledWith(MODULES_MODULE_ID);
  });

  it("hides the peek trigger until hover unless the row always shows its actions", () => {
    const { rerender } = render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        onOpenPeek={vi.fn()}
      />
    );
    const hoverOnly = screen.getByTestId("modules-row-peek").className;
    expect(hoverOnly).toContain("group-hover:opacity-100");
    // Narrow fine-pointer viewports (a squeezed desktop window, the
    // desktop shell) never hover, so the trigger stays visible there too.
    expect(hoverOnly).toContain("max-md:opacity-100");
    rerender(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        onOpenPeek={vi.fn()}
        alwaysShowActions
      />
    );
    expect(screen.getByTestId("modules-row-peek").className).not.toContain("opacity-0");
  });

  it("opens the peek from a press on the archived row body", () => {
    const onOpenPeek = vi.fn();
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        archived
        onOpenPeek={onOpenPeek}
      />
    );
    const row = screen.getByTestId("modules-row");
    expect(row).toHaveAttribute("data-module-archived", "true");
    fireEvent.click(screen.getByTestId("modules-row-progress"));
    expect(onOpenPeek).toHaveBeenCalledWith(MODULES_MODULE_ID);
  });

  it("opens the peek instead of navigating from an archived row's name", () => {
    const onOpenPeek = vi.fn();
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        archived
        onOpenPeek={onOpenPeek}
      />
    );
    const link = screen.getByTestId("modules-row-link");
    const follow = fireEvent.click(link);
    // fireEvent reports false when a handler cancelled the default, so the
    // address stays on the archives screen.
    expect(follow).toBe(false);
    expect(onOpenPeek).toHaveBeenCalledWith(MODULES_MODULE_ID);
  });

  it("keeps an archived row's controls doing their own thing", () => {
    const onOpenPeek = vi.fn();
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        archived
        onOpenPeek={onOpenPeek}
        menu={<button type="button">Row menu</button>}
        trailing={<button type="button">Status</button>}
      />
    );
    fireEvent.click(screen.getByTestId("modules-row-peek"));
    expect(onOpenPeek).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Row menu" }));
    fireEvent.click(screen.getByRole("button", { name: "Status" }));
    expect(onOpenPeek).toHaveBeenCalledTimes(1);
  });

  it("leaves a live row's name press to its anchor", () => {
    const onOpenPeek = vi.fn();
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        onOpenPeek={onOpenPeek}
      />
    );
    const link = screen.getByTestId("modules-row-link");
    link.addEventListener("click", (event) => {
      event.preventDefault();
    });
    fireEvent.click(screen.getByTestId("modules-row-name"));
    expect(onOpenPeek).not.toHaveBeenCalled();
  });

  it("follows its link on a live row body press", () => {
    const onOpenPeek = vi.fn();
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        onOpenPeek={onOpenPeek}
      />
    );
    fireEvent.click(screen.getByTestId("modules-row-progress"));
    expect(onOpenPeek).not.toHaveBeenCalled();
  });

  it("never sends an archived row to the live detail page, even without a peek writer", () => {
    render(<ModuleListRow workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} archived />);
    // The body has no writer to call, and the name anchor stands down either
    // way: an archived module has no live detail page to open.
    expect(fireEvent.click(screen.getByTestId("modules-row-progress"))).toBe(true);
    expect(fireEvent.click(screen.getByTestId("modules-row-link"))).toBe(false);
  });

  it("mounts the narrow-screen menu slot only when a menu is supplied", () => {
    const { rerender } = render(
      <ModuleListRow workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />
    );
    expect(screen.queryByTestId("modules-row-menu")).not.toBeInTheDocument();
    rerender(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        menu={<button type="button">Row menu</button>}
      />
    );
    const slot = screen.getByTestId("modules-row-menu");
    expect(slot).toHaveClass("md:hidden");
    expect(within(slot).getByRole("button", { name: "Row menu" })).toBeInTheDocument();
  });

  it("offers no reorder handle and no export, import or print action (MOD-041)", () => {
    render(<ModuleListRow workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />);
    const row = screen.getByTestId("modules-row");
    expect(row).not.toHaveAttribute("draggable");
    for (const handler of ["ondragstart", "ondragend", "ondragover", "ondrop", "ondragenter", "ondragleave"]) {
      expect(row).not.toHaveAttribute(handler);
    }
    expect(row.querySelector("[draggable], [tabindex]")).toBeNull();
    expect(row.textContent ?? "").not.toMatch(/export|import|print|download|csv/i);
    expect(within(row).queryAllByRole("button", { name: /export|import|print|download|csv/i })).toEqual([]);
    // The only controls are the name anchor and the peek trigger.
    expect(within(row).getByRole("button")).toHaveAttribute("data-testid", "modules-row-peek");
  });

  it("mounts the leading and trailing slots beside the link", () => {
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        leading={<span data-testid="leading" />}
        trailing={<button type="button" data-testid="trailing" />}
      />
    );
    expect(screen.getByTestId("leading")).toBeInTheDocument();
    expect(screen.getByTestId("trailing")).toBeInTheDocument();
    expect(screen.getByTestId("modules-row-link")).not.toContainElement(screen.getByTestId("trailing"));
  });
});
