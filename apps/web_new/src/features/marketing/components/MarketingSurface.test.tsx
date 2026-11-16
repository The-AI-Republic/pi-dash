// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing surface (CLOUD-003–009): the address picks the body, and
// every body mounts inside the shared frame.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import type { PublicMarketingPath } from "../../../core/edition/index.js";
import MarketingSurface from "./MarketingSurface.js";

describe("MarketingSurface", () => {
  it.each(["/home", "/docs", "/downloads", "/pricing"] as const)(
    "mounts the %s body inside the frame with one top-level heading",
    (path: PublicMarketingPath) => {
      const { container } = render(<MarketingSurface path={path} />);
      expect(container.querySelector("header")).not.toBeNull();
      expect(container.querySelector("footer")).not.toBeNull();
      expect(container.querySelectorAll("h1")).toHaveLength(1);
    }
  );

  it("picks the body from the address", () => {
    const home = render(<MarketingSurface path="/home" />);
    expect(home.container.querySelectorAll("article")).toHaveLength(4);
    home.unmount();

    const docs = render(<MarketingSurface path="/docs" />);
    expect(docs.container.querySelectorAll("article")).toHaveLength(3);
    expect(docs.container.querySelectorAll("pre")).toHaveLength(5);
    docs.unmount();

    const downloads = render(<MarketingSurface path="/downloads" />);
    expect(downloads.container.querySelectorAll("article")).toHaveLength(3);
    expect(downloads.container.querySelector("#cli-heading")).not.toBeNull();
    downloads.unmount();

    const pricing = render(<MarketingSurface path="/pricing" />);
    expect(pricing.container.querySelectorAll('[data-testid="pricing-tier"]')).toHaveLength(2);
    expect(pricing.container.querySelectorAll('[data-testid="pricing-faq"]')).toHaveLength(4);
    pricing.unmount();
  });
});
