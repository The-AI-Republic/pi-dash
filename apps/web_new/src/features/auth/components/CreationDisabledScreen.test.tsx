// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  CREATION_DISABLED_ASK_LABEL,
  CREATION_DISABLED_BACK_LABEL,
  CREATION_DISABLED_HEADING,
  CreationDisabledScreen,
} from "./CreationDisabledScreen.js";

describe("CreationDisabledScreen", () => {
  it("explains the restriction and goes back on request", async () => {
    const user = userEvent.setup();
    const onBack = vi.fn();
    render(
      <CreationDisabledScreen
        requester={{ firstName: "Ada", lastName: "Lovelace", email: "ada@example.com" }}
        onBack={onBack}
      />
    );
    expect(screen.getByRole("heading", { name: CREATION_DISABLED_HEADING })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: CREATION_DISABLED_BACK_LABEL }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("links an unaddressed admin request signed by the requester", () => {
    render(
      <CreationDisabledScreen
        requester={{ firstName: "Ada", lastName: "Lovelace", email: "ada@example.com" }}
        onBack={() => {}}
      />
    );
    const link = screen.getByRole("link", { name: CREATION_DISABLED_ASK_LABEL });
    const href = link.getAttribute("href") ?? "";
    expect(href.startsWith("mailto:?")).toBe(true);
    const query = new URLSearchParams(href.slice("mailto:?".length));
    expect(query.get("subject")).toBe("Requesting a new workspace");
    expect(query.get("body")).toContain("Ada Lovelace");
    expect(query.get("body")).toContain("ada@example.com");
  });
});
