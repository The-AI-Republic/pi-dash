// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const deferred = vi.hoisted(() => {
  let resolve: (() => void) | null = null;
  const promise = new Promise<{ default: () => React.ReactElement }>((settle) => {
    resolve = () => settle({ default: () => <div data-testid="calendar-loaded" /> });
  });
  return { promise, resolve: () => resolve?.() };
});

vi.mock("./CalendarLayout.js", async () => {
  const loaded = await deferred.promise;
  return { CalendarLayout: loaded.default };
});

import { CalendarEntry } from "./CalendarEntry.js";

describe("CalendarEntry", () => {
  it("shows the calendar skeleton until the lazy layout resolves", async () => {
    render(
      <CalendarEntry
        workspaceSlug="acme"
        projectId="project"
        projectIdentifier="WEB"
        entity={{ scope: "project", projectId: "project" }}
        canEdit
      />
    );

    expect(screen.getByTestId("issues-skeleton-calendar")).toBeInTheDocument();
    await act(async () => deferred.resolve());
    expect(await screen.findByTestId("calendar-loaded")).toBeInTheDocument();
  });
});
