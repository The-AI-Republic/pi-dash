// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Drafts unknown-address boundary (DRAFT-024): a per-draft-style address
// renders the illustrated not-found surface carrying the pinned phrase,
// with a way home and no draft rows.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  heads: [] as Array<Record<string, unknown>>,
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (options: unknown) => ({ options }),
}));

vi.mock("../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../shared/shell/index.js")>()),
  useDocumentHead: (head: Record<string, unknown>) => {
    harness.heads.push(head);
  },
}));

import { DRAFT_UNKNOWN_PHRASE, Route } from "./$.js";

beforeEach(() => {
  harness.heads = [];
});

function renderBoundary(): void {
  const Component = Route.options.component!;
  render(<Component />);
}

describe("drafts unknown address", () => {
  it("carries the pinned phrase with a way home", () => {
    renderBoundary();
    expect(DRAFT_UNKNOWN_PHRASE).toBe("cannot be found");
    expect(screen.getByText(/cannot be found/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to home" })).toHaveAttribute("href", "/");
  });

  it("renders no draft rows and stays out of crawlers", () => {
    renderBoundary();
    expect(document.querySelectorAll("[data-draft-block]")).toHaveLength(0);
    expect(harness.heads).toContainEqual({ title: "404 — Pi Dash", robots: "noindex, nofollow" });
  });
});
