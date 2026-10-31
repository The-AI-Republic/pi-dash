// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { PasswordStrengthHint } from "./PasswordStrengthHint.js";

describe("PasswordStrengthHint", () => {
  it("marks the met and unmet rules", () => {
    render(<PasswordStrengthHint secret="weakpass1" />);
    const group = screen.getByRole("group", { name: "Password strength" });
    expect(group).toHaveTextContent("At least 8 characters");
    expect(group).toHaveTextContent("An upper-case letter");
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(5);
    expect(items[0]).toHaveClass("text-(--success)");
    expect(items[1]).toHaveClass("text-(--text-muted)");
  });

  it("marks every rule met on a strong secret", () => {
    render(<PasswordStrengthHint secret="Tq7!mZ2@kP9#xR" />);
    for (const item of screen.getAllByRole("listitem")) {
      expect(item).toHaveClass("text-(--success)");
    }
  });
});
