// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { RichTextView } from "./RichTextView.js";

describe("RichTextView", () => {
  it("renders stored markup under its label", () => {
    render(<RichTextView html="<p>Hello <strong>there</strong></p>" label="Description" />);
    const region = screen.getByRole("region", { name: "Description" });
    expect(region).toContainHTML("<strong>there</strong>");
    expect(region).toHaveTextContent("Hello there");
  });

  it("strips unsafe markup before rendering", () => {
    render(<RichTextView html='<p>ok</p><script>alert(1)</script><img src="x" onerror="alert(2)"> ' label="Note" />);
    const region = screen.getByRole("region", { name: "Note" });
    expect(region.querySelector("script")).toBeNull();
    expect(region.innerHTML).not.toContain("onerror");
    expect(region).toHaveTextContent("ok");
  });

  it("re-sanitizes when the stored HTML changes", () => {
    const { rerender } = render(<RichTextView html="<p>first</p>" label="Body" />);
    rerender(<RichTextView html="<p>second</p>" label="Body" />);
    expect(screen.getByRole("region", { name: "Body" })).toHaveTextContent("second");
  });
});
