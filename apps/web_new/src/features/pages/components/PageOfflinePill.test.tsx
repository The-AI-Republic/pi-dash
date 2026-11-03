// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Offline pill (parity/specs/pages.md §8.3, PAGE-062): the pinned word in
// the header cluster while the browser is offline on an editable page.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { PAGE_OFFLINE_LABEL } from "../lib/pageOffline.js";
import { PageOfflinePill } from "./PageOfflinePill.js";

describe("PageOfflinePill", () => {
  it("reads the pinned offline word", () => {
    render(<PageOfflinePill />);
    expect(screen.getByTestId("page-offline-pill")).toHaveTextContent(PAGE_OFFLINE_LABEL);
    expect(PAGE_OFFLINE_LABEL).toBe("Offline");
  });
});
