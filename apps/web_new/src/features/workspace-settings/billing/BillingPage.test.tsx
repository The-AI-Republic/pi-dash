// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Billing page body (parity/specs/workspace-settings.md §29–§32,
// WSET-029/030/032): the community summary, the three-plan comparison with
// independent per-card frequency choices, an action that opens an external
// page without touching the workspace, and no trial, portal, checkout,
// subscription or upgrade-dialog surface anywhere on the page.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { BillingPage } from "./BillingPage.js";

function renderPage() {
  return render(<BillingPage workspaceSlug="acme" />);
}

describe("BillingPage", () => {
  it("shows the community summary with its plan description", () => {
    renderPage();
    const community = screen.getByTestId("billing-community");
    expect(within(community).getByRole("heading", { name: "Community" })).toBeInTheDocument();
    expect(screen.getByTestId("billing-community-description")).toHaveTextContent(/Unlimited projects/);
  });

  it("lists exactly pro, business and enterprise under All plans", () => {
    renderPage();
    expect(screen.getByRole("heading", { name: "All plans" })).toBeInTheDocument();
    const cards = screen.getAllByTestId(/^billing-plan-(pro|business|enterprise)$/);
    expect(cards.map((card) => card.getAttribute("aria-label"))).toEqual([
      "Pro plan",
      "Business plan",
      "Enterprise plan",
    ]);
    expect(screen.queryByTestId("billing-plan-free")).not.toBeInTheDocument();
  });

  it("defaults every card to monthly and reprices only the card that changes", async () => {
    const user = userEvent.setup();
    renderPage();
    expect(screen.getByTestId("billing-price-pro")).toHaveTextContent("$8");
    const business = screen.getByTestId("billing-plan-business");
    await user.click(within(business).getByRole("button", { name: "Yearly" }));
    expect(screen.getByTestId("billing-price-pro")).toHaveTextContent("$8");
    const pro = screen.getByTestId("billing-plan-pro");
    await user.click(within(pro).getByRole("button", { name: "Yearly" }));
    expect(screen.getByTestId("billing-price-pro")).toHaveTextContent("$6");
    expect(screen.getByTestId("billing-price-enterprise")).toHaveTextContent("Quote on request");
  });

  it("keeps monthly per card: switching one back leaves the other on yearly", async () => {
    const user = userEvent.setup();
    renderPage();
    const pro = screen.getByTestId("billing-plan-pro");
    await user.click(within(pro).getByRole("button", { name: "Yearly" }));
    await user.click(within(pro).getByRole("button", { name: "Monthly" }));
    expect(within(pro).getByRole("button", { name: "Monthly" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("billing-price-pro")).toHaveTextContent("$8");
    expect(screen.getByTestId("billing-plan-business")).toHaveAttribute("data-frequency", "month");
  });

  it("points each action at its external page for the chosen frequency", async () => {
    const user = userEvent.setup();
    renderPage();
    const pro = screen.getByTestId("billing-plan-pro");
    await user.click(within(pro).getByRole("button", { name: "Yearly" }));
    const actions = screen.getAllByRole("link").filter((link) => link.getAttribute("target") === "_blank");
    expect(actions.map((link) => link.getAttribute("href"))).toEqual([
      "https://airepublic.com/pricing/pro?plan=year",
      "https://airepublic.com/pricing/business?plan=month",
      "https://airepublic.com/contact",
    ]);
  });

  it("leaves the page unchanged after an action: no plan swap, notice or dialog", async () => {
    const user = userEvent.setup();
    const { container } = renderPage();
    await user.click(screen.getByRole("link", { name: /Upgrade to Pro/ }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getAllByTestId(/^billing-plan-(pro|business|enterprise)$/)).toHaveLength(3);
    expect(container.querySelector("[data-testid='workspace-settings-billing']")).toBeInTheDocument();
  });

  it("offers no trial, portal, checkout, subscription or upgrade-dialog surface", () => {
    const { container } = renderPage();
    const text = [...container.querySelectorAll("body *")]
      .filter((node) => node.children.length === 0)
      .map((node) => (node.textContent ?? "").trim())
      .filter((part) => part.length > 0)
      .join(" ");
    expect(text).not.toMatch(/trial/i);
    expect(text).not.toMatch(/portal/i);
    expect(text).not.toMatch(/checkout/i);
    expect(text).not.toMatch(/current plan/i);
    expect(text).not.toMatch(/cancel subscription/i);
    expect(text).not.toMatch(/communit(y)? entry/i);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("collapses the full comparison until asked", async () => {
    const user = userEvent.setup();
    renderPage();
    expect(screen.queryAllByRole("table")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Compare all features" }));
    expect(screen.getByRole("heading", { name: "Project + work tracking" })).toBeInTheDocument();
  });
});
