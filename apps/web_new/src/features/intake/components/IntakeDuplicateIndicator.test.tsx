// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IssueSearchHit } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { IntakeDuplicateIndicator } from "./IntakeDuplicateIndicator.js";

function hit(id: string, name: string): IssueSearchHit {
  return {
    name,
    id,
    start_date: null,
    sequence_id: 3,
    project__name: "Apollo",
    project__identifier: "APO",
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    workspace__slug: "acme",
    state__name: null,
    state__group: null,
    state__color: null,
  };
}

describe("IntakeDuplicateIndicator", () => {
  it("renders nothing without matches (the OSS answer)", () => {
    render(<IntakeDuplicateIndicator matches={[]} />);
    expect(screen.queryByTestId("intake-duplicate-indicator")).toBeNull();
  });

  it("opens the comparison from the count button", () => {
    render(
      <IntakeDuplicateIndicator
        matches={[
          hit("11111111-1111-4111-8111-111111111111", "Password reset loop"),
          hit("22222222-2222-4222-8222-222222222222", "Reset the password flow"),
        ]}
      />
    );
    const button = screen.getByRole("button", { name: "2 possible duplicates" });
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("complementary", { name: "Possible duplicates" })).toBeNull();
    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    const panel = screen.getByRole("complementary", { name: "Possible duplicates" });
    expect(panel).toHaveTextContent("Password reset loop");
    expect(panel).toHaveTextContent("Reset the password flow");
  });
});
