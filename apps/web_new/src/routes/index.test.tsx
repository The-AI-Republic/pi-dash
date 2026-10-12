// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the entry branch decision plus prop passing.
vi.mock("../features/auth/index.js", () => ({
  SignInCard: (props: { next?: string }) => <div data-testid="sign-in-card" data-next={props.next ?? ""} />,
}));

import { EntryScreen } from "./index.js";

describe("EntryScreen", () => {
  it("renders the sign-in card with the return path for signed-out visitors", () => {
    render(<EntryScreen signedIn={false} next="/acme/" />);
    expect(screen.getByTestId("sign-in-card")).toHaveAttribute("data-next", "/acme/");
    expect(screen.queryByRole("heading", { name: "Welcome to Pi Dash" })).toBeNull();
  });

  it("renders the workspace-less notice for signed-in visitors", () => {
    render(<EntryScreen signedIn={true} />);
    expect(screen.getByRole("heading", { name: "Welcome to Pi Dash" })).not.toBeNull();
    expect(screen.queryByTestId("sign-in-card")).toBeNull();
  });
});
