// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import type { OpenhubApp } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { AppCard } from "./AppCard.js";

describe("AppCard", () => {
  it("renders the app identity the cards slice builds on", () => {
    const app: OpenhubApp = { appId: "app-notes", slug: "notes", name: "Notes Pro" };
    render(
      <ul>
        <AppCard app={app} />
      </ul>
    );
    const card = screen.getByTestId("app-card");
    expect(card).toHaveAttribute("data-app-id", "app-notes");
    expect(screen.getByTestId("app-card-name")).toHaveTextContent("Notes Pro");
  });
});
