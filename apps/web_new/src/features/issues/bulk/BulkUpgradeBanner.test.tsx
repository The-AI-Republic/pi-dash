// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { BULK_UPGRADE_HREF, BulkUpgradeBanner } from "./BulkUpgradeBanner.js";

describe("BulkUpgradeBanner", () => {
  it("renders nothing without a selection", () => {
    const { container } = render(<BulkUpgradeBanner count={0} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("names the count and links to the upgrade page", () => {
    render(<BulkUpgradeBanner count={1} />);
    expect(screen.getByTestId("bulk-upgrade-banner")).toHaveTextContent("1 work item selected.");
    expect(screen.getByRole("link", { name: "Upgrade" })).toHaveAttribute("href", BULK_UPGRADE_HREF);
  });

  it("pluralizes multiple selected work items", () => {
    render(<BulkUpgradeBanner count={4} />);
    expect(screen.getByTestId("bulk-upgrade-banner")).toHaveTextContent("4 work items selected.");
    expect(screen.getByTestId("bulk-upgrade-banner")).toHaveTextContent("Unlock multi-field editing");
    expect(screen.queryByText(/Change state, priority/)).not.toBeInTheDocument();
  });
});
