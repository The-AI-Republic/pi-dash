// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The layout glyph is decorative: it carries no accessible name of its
// own, so the button or menu entry supplies one.

import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ModuleLayoutIcon } from "./ModuleLayoutIcon.js";

describe("ModuleLayoutIcon", () => {
  it("renders a hidden glyph for each layout", () => {
    for (const layout of ["list", "board", "timeline"] as const) {
      const { container, unmount } = render(<ModuleLayoutIcon layout={layout} />);
      const svg = container.querySelector("svg");
      expect(svg).not.toBeNull();
      expect(svg).toHaveAttribute("aria-hidden", "true");
      unmount();
    }
  });
});
