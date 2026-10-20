// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ProjectMark } from "./projectIcon.js";

const STAR: Extract<ProjectMark, { kind: "icon" }> = { kind: "icon", name: "star", color: null };

/** A fresh wrapper instance, as on the first paint of a route. */
async function freshWrapper(): Promise<typeof import("./ProjectIcon.js")> {
  vi.resetModules();
  return import("./ProjectIcon.js");
}

afterEach(() => {
  vi.resetModules();
});

describe("ProjectIcon", () => {
  it("shows the short code's initial until the glyph chunk arrives", async () => {
    const { ProjectIcon } = await freshWrapper();
    render(<ProjectIcon mark={STAR} identifier="seed" size={20} testId="glyph" />);
    expect(screen.getByText("S")).toBeInTheDocument();
    expect(screen.queryByTestId("glyph")).not.toBeInTheDocument();
    const glyph = await screen.findByTestId("glyph");
    expect(glyph.getAttribute("width")).toBe("20");
    expect(glyph.getAttribute("stroke")).toBe("currentColor");
    expect(screen.queryByText("S")).not.toBeInTheDocument();
  });

  it("draws a coloured glyph after loading, with no fallback in the document", async () => {
    const { ProjectIcon } = await freshWrapper();
    render(
      <ProjectIcon
        mark={{ kind: "icon", name: "rocket_launch", color: "#f97316" }}
        identifier="WEB"
        size={22}
        testId="project-logo-glyph-test"
      />
    );
    const glyph = await screen.findByTestId("project-logo-glyph-test");
    expect(glyph.getAttribute("stroke")).toBe("#f97316");
  });

  it("keeps the initial for a name this frontend cannot draw", async () => {
    const { ProjectIcon } = await freshWrapper();
    render(<ProjectIcon mark={{ kind: "icon", name: "no_such_symbol", color: null }} identifier="WEB" size={20} />);
    expect(await screen.findByText("W")).toBeInTheDocument();
    await waitFor(() => expect(document.querySelector("svg")).toBeNull());
  });

  it("reuses one load across marks and renders a later mark without the initial", async () => {
    const { ProjectIcon } = await freshWrapper();
    const first = render(<ProjectIcon mark={STAR} identifier="SEED" size={20} testId="first" />);
    await screen.findByTestId("first");
    first.rerender(<ProjectIcon mark={{ ...STAR, name: "anchor" }} identifier="SEED" size={20} testId="second" />);
    expect(await screen.findByTestId("second")).toBeInTheDocument();
    expect(screen.queryByText("S")).not.toBeInTheDocument();
  });
});
