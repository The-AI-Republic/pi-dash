// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ADMIN_ENTRY_HREF, PRODUCT_MARK_SRC, PRODUCT_NAME } from "./brand.js";
import { NOT_READY_BODY, NOT_READY_HEADING, NotReadyScreen } from "./NotReadyScreen.js";

describe("NotReadyScreen", () => {
  it("welcomes the administrator with setup guidance and one entry", () => {
    render(<NotReadyScreen />);
    expect(screen.getByTestId("instance-not-ready")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: NOT_READY_HEADING })).toBeInTheDocument();
    expect(screen.getByText(NOT_READY_BODY)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Get started" })).toHaveAttribute("href", ADMIN_ENTRY_HREF);
    expect(ADMIN_ENTRY_HREF).toBe("/god-mode/");
  });

  it("carries the product mark and no workspace content or navigation", () => {
    render(<NotReadyScreen />);
    expect(screen.getByAltText(`${PRODUCT_NAME} mark`)).toHaveAttribute("src", PRODUCT_MARK_SRC);
    expect(screen.queryAllByRole("navigation")).toHaveLength(0);
    expect(screen.queryAllByRole("link")).toHaveLength(1);
    expect(document.title).toBe(`${PRODUCT_NAME} - Setup`);
  });
});
