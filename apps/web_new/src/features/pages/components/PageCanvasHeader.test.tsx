// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Canvas header type preferences (parity/specs/pages.md §9.1, PAGE-072):
// a header handed no stored pair still lines up with the body's default
// size and face, and an explicit pair lands on the same title shell.

import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PageCanvasHeader } from "./PageCanvasHeader.js";

function mount(props: Partial<React.ComponentProps<typeof PageCanvasHeader>> = {}): void {
  render(
    <PageCanvasHeader
      title={{ pageId: "page-1", name: "Launch notes", editable: false }}
      logoProps={{}}
      editable={false}
      titleEmpty={false}
      onMarkPick={vi.fn()}
      {...props}
    />
  );
}

afterEach(cleanup);

describe("PageCanvasHeader type preferences (PAGE-072)", () => {
  it("defaults the title to the body's large sans-serif pair", () => {
    mount();
    expect(screen.getByTestId("page-detail-title")).toHaveClass("text-size-large-font", "text-face-sans-serif");
  });

  it("carries an explicit compact serif pair on the title shell", () => {
    mount({ fontSize: "small-font", fontStyle: "serif" });
    expect(screen.getByTestId("page-detail-title")).toHaveClass("text-size-small-font", "text-face-serif");
  });
});
