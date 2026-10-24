// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace active cycles (parity/specs/cycles.md §16, CYC-043): the
// paid upsell's pinned shape — Pro badge, snapshots heading, six
// benefit cards, and the pricing upgrade that opens in a new tab.

import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import {
  ACTIVES_BENEFITS,
  ACTIVES_PRICING_HREF,
  ACTIVES_SNAPSHOTS_HEADING,
  ACTIVES_UPGRADE_LABEL,
  WorkspaceActivesUpsell,
} from "./WorkspaceActivesUpsell.js";

describe("WorkspaceActivesUpsell", () => {
  it("badges the paid feature and heads the snapshots", () => {
    render(<WorkspaceActivesUpsell />);
    expect(screen.getByTestId("workspace-actives-badge")).toHaveTextContent("Pro");
    const heading = screen.getByRole("heading", { level: 2 });
    expect(heading).toHaveTextContent("snapshots");
    expect(heading).toHaveTextContent(ACTIVES_SNAPSHOTS_HEADING);
  });

  it("cards exactly six benefits, each headed", () => {
    render(<WorkspaceActivesUpsell />);
    const cards = within(screen.getByTestId("workspace-actives-benefits")).getAllByRole("listitem");
    expect(cards).toHaveLength(6);
    expect(ACTIVES_BENEFITS).toHaveLength(6);
    for (const card of cards) {
      expect(within(card).getByRole("heading", { level: 3 })).toBeInTheDocument();
    }
    expect(cards.map((card) => within(card).getByRole("heading", { level: 3 }).textContent)).toEqual(
      ACTIVES_BENEFITS.map((benefit) => benefit.title)
    );
  });

  it("upgrades to pricing in a new tab", () => {
    render(<WorkspaceActivesUpsell />);
    const upgrade = screen.getByRole("link", { name: ACTIVES_UPGRADE_LABEL });
    expect(upgrade).toHaveAttribute("href", ACTIVES_PRICING_HREF);
    expect(upgrade.getAttribute("href")).toContain("pricing");
    expect(upgrade).toHaveAttribute("target", "_blank");
  });
});
