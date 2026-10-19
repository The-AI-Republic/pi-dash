// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Apps pointer (CLOUD-036): names the sidebar catalog and nothing else. The
// settings page must not grow a second, drifting copy of the catalog.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AppsPointer } from "./AppsPointer.js";

describe("AppsPointer", () => {
  it("sends readers to the sidebar Apps section", () => {
    render(<AppsPointer />);
    const pointer = screen.getByTestId("cloud-assistant-apps-pointer");
    expect(pointer).toHaveTextContent("Looking for apps?");
    expect(pointer).toHaveTextContent("sidebar under Apps");
  });

  it("carries no controls or links of its own", () => {
    render(<AppsPointer />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("link")).toBeNull();
  });
});
