// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tab strip (NEWFRONT-540, RUN-003). Three links off the scope base; the
// active marker derives from the pathname and the overview matches exactly.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: {
      to?: string;
      children?: React.ReactNode;
      "aria-current"?: "page";
      activeOptions?: { exact?: boolean };
    }) => (
      <a
        href={props.to}
        aria-current={props["aria-current"]}
        data-exact={props.activeOptions?.exact === true ? "true" : undefined}
      >
        {props.children}
      </a>
    ),
  };
});

import { RUNNERS_TABS, runnersActiveTabKey, RunnersTabStrip } from "./RunnersTabStrip.js";

const workspace = { workspaceSlug: "acme" };
const project = { workspaceSlug: "acme", projectId: "p-1" };

describe("runnersActiveTabKey", () => {
  it("selects the tab that owns the pathname", () => {
    expect(runnersActiveTabKey("/acme/runners", workspace)).toBe("overview");
    expect(runnersActiveTabKey("/acme/runners/", workspace)).toBe("overview");
    expect(runnersActiveTabKey("/acme/runners/runs", workspace)).toBe("runs");
    expect(runnersActiveTabKey("/acme/runners/runs/a-run", workspace)).toBe("runs");
    expect(runnersActiveTabKey("/acme/runners/runs/a-run/", workspace)).toBe("runs");
    expect(runnersActiveTabKey("/acme/runners/approvals", workspace)).toBe("approvals");
    expect(runnersActiveTabKey("/acme/projects/p-1/runners/runs", project)).toBe("runs");
    expect(runnersActiveTabKey("/acme/projects/p-1/runners/runs/a-run", project)).toBe("runs");
  });

  it("returns null on child routes and foreign paths", () => {
    expect(runnersActiveTabKey("/acme/runners/detail/r-1", workspace)).toBeNull();
    expect(runnersActiveTabKey("/acme/runners/chat/r-1", workspace)).toBeNull();
    expect(runnersActiveTabKey("/acme/runnersX", workspace)).toBeNull();
  });
});

describe("RunnersTabStrip", () => {
  it("offers the three routes with the workspace base", () => {
    render(<RunnersTabStrip scope={workspace} pathname="/acme/runners" />);
    const links = screen.getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual(["Overview", "Runs", "Approvals"]);
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "/acme/runners",
      "/acme/runners/runs",
      "/acme/runners/approvals",
    ]);
    expect(links.map((link) => link.getAttribute("aria-current"))).toEqual(["page", null, null]);
  });

  it("uses the project base and marks the runs tab", () => {
    render(<RunnersTabStrip scope={project} pathname="/acme/projects/p-1/runners/runs" />);
    const links = screen.getAllByRole("link");
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "/acme/projects/p-1/runners",
      "/acme/projects/p-1/runners/runs",
      "/acme/projects/p-1/runners/approvals",
    ]);
    expect(links.map((link) => link.getAttribute("aria-current"))).toEqual([null, "page", null]);
  });

  it("keeps the overview unselected on a child route", () => {
    render(<RunnersTabStrip scope={workspace} pathname="/acme/runners/detail/r-1" />);
    expect(screen.getAllByRole("link").map((link) => link.getAttribute("aria-current"))).toEqual([null, null, null]);
  });

  it("matches the overview and approvals links exactly (NEWFRONT-557)", () => {
    // The router marks fuzzy-active links aria-current="page" itself,
    // which would re-mark the overview link on every child route; runs
    // keeps the fuzzy default so a run detail still selects it.
    render(<RunnersTabStrip scope={workspace} pathname="/acme/runners/runs" />);
    expect(screen.getAllByRole("link").map((link) => link.getAttribute("data-exact"))).toEqual(["true", null, "true"]);
  });

  it("declares the tab vocabulary once", () => {
    expect(RUNNERS_TABS.map((tab) => tab.key)).toEqual(["overview", "runs", "approvals"]);
  });
});
