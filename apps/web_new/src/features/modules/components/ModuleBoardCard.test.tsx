// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One board card (parity/specs/modules.md §3.6, MOD-020): truncated name
// with a full-name tooltip, status slot, peek trigger, done-over-total
// count, lead or no-lead mark, segmented bar, and the date and action
// slots. A press on the card body opens the detail page; a press on any
// control stays put.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className, ...rest }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className} {...rest}>
      {children}
    </a>
  ),
  useNavigate: () => routerHarness.navigate,
}));

import { MODULES_LEAD_ID, MODULES_MODULE_ID, MODULES_PROJECT_ID, makeModuleRow } from "../testing.js";
import { ModuleBoardCard } from "./ModuleBoardCard.js";

const LEAD = { name: "Ada Lovelace", avatarUrl: null };

describe("ModuleBoardCard", () => {
  it("links the name and keeps the full name on hover", () => {
    const name = "Card With An Exceptionally Long Name That Surely Truncates Inside A Narrow Column";
    render(<ModuleBoardCard workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow({ name })} />);
    expect(screen.getByRole("link", { name })).toHaveAttribute(
      "href",
      `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`
    );
    const label = screen.getByTestId("modules-board-card-name");
    expect(label).toHaveClass("truncate");
    expect(label).toHaveAttribute("title", name);
  });

  it("carries the module identity on the card", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ name: "Card", status: "paused" })}
      />
    );
    const card = screen.getByTestId("modules-board-card");
    expect(card).toHaveAttribute("data-module-id", MODULES_MODULE_ID);
    expect(card).toHaveAttribute("data-module-name", "Card");
    expect(card).toHaveAttribute("data-module-status", "paused");
  });

  it("shows the done-over-total work-item count", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ total_issues: 2, completed_issues: 1 })}
      />
    );
    expect(screen.getByTestId("modules-board-card-count")).toHaveTextContent("1/2 work items");
  });

  it("shows the lead's avatar when one is set", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ lead_id: MODULES_LEAD_ID })}
        lead={LEAD}
      />
    );
    const lead = screen.getByTestId("modules-card-lead");
    expect(lead).toHaveAttribute("data-lead-kind", "avatar");
    expect(within(lead).getByText("AL")).toBeInTheDocument();
  });

  it("passes the lead's avatar image through when the directory has one", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ lead_id: MODULES_LEAD_ID })}
        lead={{ name: "Ada Lovelace", avatarUrl: "https://cdn.test/ada.png" }}
      />
    );
    const lead = screen.getByTestId("modules-card-lead");
    expect(lead).toHaveAttribute("data-lead-kind", "avatar");
    // The avatar keeps the name on its wrapper and paints the directory
    // image inside it.
    expect(within(lead).getByRole("img", { name: "Ada Lovelace" })).toBe(lead.firstElementChild);
    expect(lead.querySelector("img")).toHaveAttribute("src", "https://cdn.test/ada.png");
  });

  it("shows the no-lead mark for a module without a lead", () => {
    render(
      <ModuleBoardCard workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} lead={LEAD} />
    );
    const lead = screen.getByTestId("modules-card-lead");
    expect(lead).toHaveAttribute("data-lead-kind", "none");
    expect(lead).toHaveAttribute("title", "No lead");
    expect(within(lead).getByText("No lead")).toHaveClass("sr-only");
  });

  it("shows the no-lead mark when the lead slot has no directory entry yet", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ lead_id: MODULES_LEAD_ID })}
        lead={null}
      />
    );
    expect(screen.getByTestId("modules-card-lead")).toHaveAttribute("data-lead-kind", "none");
  });

  it("draws the segmented work-item bar from the module's mix", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ total_issues: 4, completed_issues: 2, started_issues: 2 })}
      />
    );
    const segments = screen.getAllByTestId("modules-card-progress-segment");
    expect(segments).toHaveLength(2);
    expect(segments[0]).toHaveAttribute("data-segment-group", "completed");
    expect(segments[0]?.getAttribute("style")).toContain("width: 50%");
    expect(segments[1]).toHaveAttribute("data-segment-group", "started");
  });

  it("opens the peek from the info control without navigating", () => {
    const onOpenPeek = vi.fn();
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        onOpenPeek={onOpenPeek}
      />
    );
    const control = screen.getByRole("button", { name: "Open preview of Delivery" });
    expect(control).toHaveAttribute("data-testid", "modules-board-card-peek");
    fireEvent.click(control);
    expect(onOpenPeek).toHaveBeenCalledWith(MODULES_MODULE_ID);
    expect(routerHarness.navigate).not.toHaveBeenCalled();
  });

  it("navigates to the detail page from the card body", () => {
    routerHarness.navigate.mockReset();
    render(<ModuleBoardCard workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />);
    fireEvent.click(screen.getByTestId("modules-board-card-count"));
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`,
    });
  });

  it("leaves every control's press to the control", () => {
    routerHarness.navigate.mockReset();
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        statusControl={<button type="button">Status</button>}
        dates={<button type="button">Dates</button>}
        favorite={<button type="button">Favorite</button>}
        menu={<button type="button">Menu</button>}
      />
    );
    for (const name of ["Status", "Dates", "Favorite", "Menu"]) {
      fireEvent.click(screen.getByRole("button", { name }));
    }
    // The name stays a real anchor, so the browser follows it; the card's
    // own navigation must not fire a second time.
    expect(screen.getByTestId("modules-board-card-link")).toHaveAttribute(
      "href",
      `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`
    );
    expect(routerHarness.navigate).not.toHaveBeenCalled();
  });

  it("stops a control's keyboard press before the card's host hears it", () => {
    const hostHeard = vi.fn();
    render(
      <div onKeyDown={hostHeard}>
        <ModuleBoardCard
          workspaceSlug="acme"
          projectId={MODULES_PROJECT_ID}
          module={makeModuleRow({ name: "Card" })}
          favorite={<button type="button">Favorite</button>}
        />
      </div>
    );
    // The slot stands the press down, so a host shortcut above the card
    // never fires for a key pressed inside one of its controls.
    fireEvent.keyDown(screen.getByRole("button", { name: "Favorite" }), { key: "Enter" });
    expect(hostHeard).not.toHaveBeenCalled();
  });

  it("leaves a press on the name link to the link itself", () => {
    routerHarness.navigate.mockReset();
    render(<ModuleBoardCard workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />);
    const link = screen.getByTestId("modules-board-card-link");
    // jsdom has no navigation, so the anchor's own default action stands
    // down; what matters is that the card does not add a second one.
    link.addEventListener("click", (event) => {
      event.preventDefault();
    });
    fireEvent.click(screen.getByTestId("modules-board-card-name"));
    expect(routerHarness.navigate).not.toHaveBeenCalled();
  });

  it("swallows the press on a slot wrapper even when the wrapper has no control", () => {
    routerHarness.navigate.mockReset();
    render(<ModuleBoardCard workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />);
    fireEvent.click(screen.getByTestId("modules-card-dates"));
    expect(routerHarness.navigate).not.toHaveBeenCalled();
  });

  it("mounts the status, date and action slots", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        statusControl={<span data-testid="card-status" />}
        dates={<span data-testid="card-dates" />}
        favorite={<span data-testid="card-favorite" />}
        menu={<button type="button" data-testid="card-menu" />}
      />
    );
    expect(within(screen.getByTestId("modules-card-status")).getByTestId("card-status")).toBeInTheDocument();
    expect(screen.getByTestId("modules-card-dates")).toHaveClass("py-0.5");
    expect(within(screen.getByTestId("modules-card-dates")).getByTestId("card-dates")).toBeInTheDocument();
    const actions = screen.getByTestId("modules-card-actions");
    expect(within(screen.getByTestId("modules-card-favorite")).getByTestId("card-favorite")).toBeInTheDocument();
    expect(within(screen.getByTestId("modules-card-menu")).getByTestId("card-menu")).toBeInTheDocument();
    expect(screen.getByTestId("modules-board-card-link")).not.toContainElement(actions);
  });

  it("is not draggable and offers no export, import or print action (MOD-041)", () => {
    render(<ModuleBoardCard workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />);
    const card = screen.getByTestId("modules-board-card");
    expect(card).not.toHaveAttribute("draggable");
    for (const handler of ["ondragstart", "ondragend", "ondragover", "ondrop", "ondragenter", "ondragleave"]) {
      expect(card).not.toHaveAttribute(handler);
    }
    expect(card.querySelector("[draggable], [tabindex]")).toBeNull();
    expect(card.textContent ?? "").not.toMatch(/export|import|print|download|csv/i);
    // The only control is the peek trigger; the rest arrive through slots.
    expect(within(card).getByRole("button")).toHaveAttribute("data-testid", "modules-board-card-peek");
  });
});
