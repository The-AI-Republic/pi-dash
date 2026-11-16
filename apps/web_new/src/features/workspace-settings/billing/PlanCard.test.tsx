// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plan card (parity/specs/workspace-settings.md §29/§30, WSET-029/030): the
// name and popularity marker, the frequency switch with its selected half,
// the repriced amount with a discount only where yearly undercuts, the
// external action target, and the highlight list.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { findBillingPlan, type PlanDefinition } from "./model.js";
import { PlanCard } from "./PlanCard.js";

function renderCard(id: "pro" | "business" | "enterprise", frequency: "month" | "year" = "month") {
  const onFrequencyChange = vi.fn();
  const view = render(
    <PlanCard plan={findBillingPlan(id)} frequency={frequency} onFrequencyChange={onFrequencyChange} />
  );
  return {
    onFrequencyChange,
    unmount: view.unmount,
    rerender: (next: "month" | "year") =>
      view.rerender(<PlanCard plan={findBillingPlan(id)} frequency={next} onFrequencyChange={onFrequencyChange} />),
  };
}

describe("PlanCard", () => {
  it("heads the card with the plan name and marks the popular plan", () => {
    const { unmount } = renderCard("pro");
    expect(screen.getByRole("heading", { name: "Pro" })).toBeInTheDocument();
    expect(screen.getByText("Popular")).toBeInTheDocument();
    unmount();
    renderCard("business");
    expect(screen.queryByText("Popular")).not.toBeInTheDocument();
  });

  it("shows both frequency halves with the page's choice selected", () => {
    renderCard("pro", "month");
    const monthly = screen.getByRole("button", { name: "Monthly" });
    const yearly = screen.getByRole("button", { name: "Yearly" });
    expect(monthly).toHaveAttribute("aria-pressed", "true");
    expect(yearly).toHaveAttribute("aria-pressed", "false");
  });

  it("reports a frequency pick and reprices on the next render", async () => {
    const user = userEvent.setup();
    const { onFrequencyChange, rerender } = renderCard("pro");
    await user.click(screen.getByRole("button", { name: "Yearly" }));
    expect(onFrequencyChange).toHaveBeenCalledWith("year");
    expect(screen.getByTestId("billing-price-pro")).toHaveTextContent("$8");
    rerender("year");
    expect(screen.getByTestId("billing-price-pro")).toHaveTextContent("$6");
    expect(screen.getByRole("button", { name: "Yearly" })).toHaveAttribute("aria-pressed", "true");
  });

  it("shows no discount badge when a priced plan's yearly price does not undercut", () => {
    // Spec §29: the badge appears only where yearly undercuts monthly. No
    // catalog entry is priced without a discount today, so exercise the
    // rule through a priced variant that has none.
    const priced = findBillingPlan("pro");
    const undiscounted: PlanDefinition = { ...priced, price: { ...priced.price, discount: null } };
    render(<PlanCard plan={undiscounted} frequency="year" onFrequencyChange={vi.fn()} />);
    expect(screen.getByTestId("billing-price-pro")).toHaveTextContent("$6");
    expect(screen.getByText("per user / month")).toBeInTheDocument();
    expect(screen.queryByTestId("billing-discount-pro")).not.toBeInTheDocument();
  });

  it("keeps the discount badge on the priced plan only", () => {
    const { unmount } = renderCard("pro");
    expect(screen.getByTestId("billing-discount-pro")).toHaveTextContent("-25%");
    unmount();
    renderCard("enterprise");
    expect(screen.queryByTestId("billing-discount-enterprise")).not.toBeInTheDocument();
  });

  it("quotes unpriced plans on both frequencies with the switch still present", () => {
    const { rerender } = renderCard("business");
    expect(screen.getByTestId("billing-price-business")).toHaveTextContent("Quote on request");
    rerender("year");
    expect(screen.getByTestId("billing-price-business")).toHaveTextContent("Quote on request");
    expect(screen.getByRole("button", { name: "Monthly" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Yearly" })).toBeInTheDocument();
  });

  it("points the action at the external page for the frequency in a new tab", () => {
    const { rerender } = renderCard("pro");
    const action = screen.getByRole("link", { name: /Upgrade to Pro/ });
    expect(action).toHaveAttribute("href", "https://airepublic.com/pricing/pro?plan=month");
    expect(action).toHaveAttribute("target", "_blank");
    rerender("year");
    expect(screen.getByRole("link", { name: /Upgrade to Pro/ })).toHaveAttribute(
      "href",
      "https://airepublic.com/pricing/pro?plan=year"
    );
  });

  it("labels business as a sales conversation while linking its pricing page", () => {
    renderCard("business");
    expect(screen.getByRole("link", { name: /Talk to Sales/ })).toHaveAttribute(
      "href",
      "https://airepublic.com/pricing/business?plan=month"
    );
  });

  it("renders the plan highlights beneath the action", () => {
    renderCard("enterprise");
    expect(screen.getByText("Private and managed deployments")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Talk to Sales/ })).toHaveAttribute(
      "href",
      "https://airepublic.com/contact"
    );
  });
});
