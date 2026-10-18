// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Side rail (NEWFRONT-540, RUN-004). Header, overview link and one chat
// contact per runner with a status dot; a per-scope empty sentence.
import type { Runner } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to?: string; children?: React.ReactNode; "data-testid"?: string; "aria-label"?: string }) => (
      <a href={props.to} data-testid={props["data-testid"]}>
        {props.children}
      </a>
    ),
  };
});

import { RunnersRail, chatHref, RAIL_HEADER } from "./RunnersRail.js";

const runner = (id: string, name: string, status: Runner["status"] = "online"): Runner =>
  ({ id, name, status, enrolled_at: "2024-01-01T00:00:00Z" }) as Runner;

describe("chatHref", () => {
  it("addresses a runner's chat under the scope base", () => {
    expect(chatHref("/acme/runners", "r-1")).toBe("/acme/runners/chat/r-1");
  });
});

describe("RunnersRail", () => {
  it("renders the header, overview link and dotted contacts in the workspace scope", () => {
    render(
      <RunnersRail
        scope={{ workspaceSlug: "acme" }}
        runners={[runner("r-1", "alpha"), runner("r-2", "beta", "revoked")]}
        isProjectScope={false}
      />
    );
    expect(screen.getByText(RAIL_HEADER)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Overview/ })).toHaveAttribute("href", "/acme/runners");
    const contacts = screen.getAllByTestId("runners-rail-contact");
    expect(contacts.map((node) => node.getAttribute("href"))).toEqual([
      "/acme/runners/chat/r-1",
      "/acme/runners/chat/r-2",
    ]);
    for (const contact of contacts) {
      // The glyph plus the status dot: the parity rail counts two icons.
      expect(contact.querySelectorAll("svg")).toHaveLength(2);
    }
    expect(screen.queryByTestId("runners-rail-empty")).toBeNull();
  });

  it("links contacts into the project chat tree", () => {
    render(
      <RunnersRail
        scope={{ workspaceSlug: "acme", projectId: "p-1" }}
        runners={[runner("r-1", "alpha")]}
        isProjectScope
      />
    );
    expect(screen.getByRole("link", { name: /Overview/ })).toHaveAttribute("href", "/acme/projects/p-1/runners");
    expect(screen.getByTestId("runners-rail-contact")).toHaveAttribute("href", "/acme/projects/p-1/runners/chat/r-1");
  });

  it("shows the project empty sentence when no runners are connected", () => {
    render(<RunnersRail scope={{ workspaceSlug: "acme", projectId: "p-1" }} runners={[]} isProjectScope />);
    expect(screen.getByTestId("runners-rail-empty")).toHaveTextContent("No runners connected to this project yet.");
    expect(screen.queryAllByTestId("runners-rail-contact")).toEqual([]);
  });

  it("shows the workspace empty sentence", () => {
    render(<RunnersRail scope={{ workspaceSlug: "acme" }} runners={[]} isProjectScope={false} />);
    expect(screen.getByTestId("runners-rail-empty")).toHaveTextContent("No runners connected yet.");
  });
});
