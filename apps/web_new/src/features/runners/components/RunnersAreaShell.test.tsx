// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Area shell (NEWFRONT-540, RUN-001/RUN-004). Loading renders nothing; the
// denial renders inside the supplied chrome without the rail; the admitted
// shell composes rail plus a content main carrying the tab strip.
import type { Runner } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to?: string; children?: React.ReactNode; "aria-current"?: "page" }) => (
      <a href={props.to} aria-current={props["aria-current"]}>
        {props.children}
      </a>
    ),
  };
});

import { RUNNERS_DENIED_HEADING } from "./NotAuthorizedView.js";
import { isProjectScope, RunnersAreaShell } from "./RunnersAreaShell.js";

const runner = { id: "r-1", name: "alpha", status: "online", enrolled_at: null } as Runner;

describe("isProjectScope", () => {
  it("detects a project id", () => {
    expect(isProjectScope({ workspaceSlug: "acme" })).toBe(false);
    expect(isProjectScope({ workspaceSlug: "acme", projectId: "p-1" })).toBe(true);
  });
});

describe("RunnersAreaShell", () => {
  it("renders nothing while the roles load", () => {
    const { container } = render(
      <RunnersAreaShell scope={{ workspaceSlug: "acme" }} access="loading" runners={[]} pathname="/acme/runners">
        <p>child</p>
      </RunnersAreaShell>
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the denial inside the chrome but without the rail", () => {
    render(
      <RunnersAreaShell
        scope={{ workspaceSlug: "acme" }}
        access="denied"
        runners={[]}
        pathname="/acme/runners"
        chrome={<header>Workspace chrome</header>}
      >
        <p>child</p>
      </RunnersAreaShell>
    );
    expect(screen.getByText("Workspace chrome")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: RUNNERS_DENIED_HEADING })).toBeInTheDocument();
    expect(screen.queryByTestId("runners-rail")).toBeNull();
    expect(screen.queryByRole("main", { name: "Runners content" })).toBeNull();
    expect(screen.queryByText("child")).toBeNull();
  });

  it("composes the rail and the content column for an admitted viewer", () => {
    render(
      <RunnersAreaShell
        scope={{ workspaceSlug: "acme" }}
        access="allowed"
        runners={[runner]}
        pathname="/acme/runners/runs"
        chrome={<header>Workspace chrome</header>}
      >
        <p>child</p>
      </RunnersAreaShell>
    );
    expect(screen.getByText("Workspace chrome")).toBeInTheDocument();
    expect(screen.getByTestId("runners-rail")).toBeInTheDocument();
    const main = screen.getByRole("main", { name: "Runners content" });
    expect(main).toContainElement(screen.getByText("child"));
    expect(screen.getByTestId("runners-tabs")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Runs" })).toHaveAttribute("aria-current", "page");
  });

  it("renders without chrome", () => {
    render(
      <RunnersAreaShell scope={{ workspaceSlug: "acme" }} access="allowed" runners={[]} pathname="/acme/runners">
        <p>child</p>
      </RunnersAreaShell>
    );
    expect(screen.getByTestId("runners-rail")).toBeInTheDocument();
    expect(screen.queryByText("Workspace chrome")).toBeNull();
  });
});
