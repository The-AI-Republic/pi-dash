// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-005 favorites, creation conditions and creator picker.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PagesFilterMenu } from "./PagesFilterMenu.js";

const FILTERS = { favorites: false, created: null, creator: null };
const CREATORS = [
  { id: "u1", name: "Ada Byron", avatarUrl: null },
  { id: "u2", name: "Zed Shaw", avatarUrl: null },
];

function open() {
  fireEvent.click(screen.getByRole("button", { name: "Filters" }));
}

describe("PagesFilterMenu", () => {
  it("toggles favorites", () => {
    const onFiltersChange = vi.fn();
    render(
      <PagesFilterMenu filters={FILTERS} creators={CREATORS} currentUserId="u1" onFiltersChange={onFiltersChange} />
    );
    open();
    fireEvent.click(screen.getByRole("button", { name: "Favorites" }));
    expect(onFiltersChange).toHaveBeenCalledWith({ ...FILTERS, favorites: true });
  });

  it("applies and removes a creation condition", () => {
    const onFiltersChange = vi.fn();
    const { rerender } = render(
      <PagesFilterMenu filters={FILTERS} creators={CREATORS} currentUserId="u1" onFiltersChange={onFiltersChange} />
    );
    open();
    fireEvent.click(screen.getByRole("button", { name: "1 week ago" }));
    expect(onFiltersChange).toHaveBeenCalledWith({ ...FILTERS, created: "1_week_ago" });
    rerender(
      <PagesFilterMenu
        filters={{ ...FILTERS, created: "1_week_ago" }}
        creators={CREATORS}
        currentUserId="u1"
        onFiltersChange={onFiltersChange}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "1 week ago" }));
    expect(onFiltersChange).toHaveBeenLastCalledWith(FILTERS);
  });

  it("renames You, searches and picks a creator", () => {
    const onFiltersChange = vi.fn();
    render(
      <PagesFilterMenu filters={FILTERS} creators={CREATORS} currentUserId="u1" onFiltersChange={onFiltersChange} />
    );
    open();
    expect(screen.getByRole("button", { name: /You/ })).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "zed" } });
    expect(screen.queryByRole("button", { name: /You/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Zed Shaw/ }));
    expect(onFiltersChange).toHaveBeenCalledWith({ ...FILTERS, creator: "u2" });
  });

  it("shows the empty creator message", () => {
    render(<PagesFilterMenu filters={FILTERS} creators={[]} currentUserId={null} onFiltersChange={() => undefined} />);
    open();
    expect(screen.getByText("No members found.")).toBeInTheDocument();
  });

  it("removes a picked creator when it is chosen again", () => {
    const onFiltersChange = vi.fn();
    render(
      <PagesFilterMenu
        filters={{ ...FILTERS, creator: "u2" }}
        creators={CREATORS}
        currentUserId="u1"
        onFiltersChange={onFiltersChange}
      />
    );
    open();
    fireEvent.click(screen.getByRole("button", { name: /Zed Shaw/ }));
    expect(onFiltersChange).toHaveBeenCalledWith({ ...FILTERS, creator: null });
  });

  it("renders a creator avatar when the directory has one", () => {
    render(
      <PagesFilterMenu
        filters={FILTERS}
        creators={[{ id: "u2", name: "Zed Shaw", avatarUrl: "https://x.test/z.png" }]}
        currentUserId="u1"
        onFiltersChange={() => undefined}
      />
    );
    open();
    expect(screen.getByRole("img", { name: "Zed Shaw" }).querySelector("img")).toHaveAttribute(
      "src",
      "https://x.test/z.png"
    );
  });

  it("applies and removes an explicit calendar day", () => {
    const onFiltersChange = vi.fn();
    const { rerender } = render(
      <PagesFilterMenu filters={FILTERS} creators={CREATORS} currentUserId="u1" onFiltersChange={onFiltersChange} />
    );
    open();
    const apply = screen.getByRole("button", { name: "Apply" });
    expect(apply).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Custom creation date"), { target: { value: "2026-10-20" } });
    fireEvent.click(apply);
    expect(onFiltersChange).toHaveBeenCalledWith({ ...FILTERS, created: { custom: "2026-10-20" } });
    rerender(
      <PagesFilterMenu
        filters={{ ...FILTERS, created: { custom: "2026-10-20" } }}
        creators={CREATORS}
        currentUserId="u1"
        onFiltersChange={onFiltersChange}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(onFiltersChange).toHaveBeenLastCalledWith(FILTERS);
  });
});
