// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ProjectsListSearch } from "./ProjectsListSearch.js";

function Harness() {
  const [query, setQuery] = React.useState("");
  return (
    <div>
      <button type="button">Outside</button>
      <ProjectsListSearch query={query} onQueryChange={setQuery} />
    </div>
  );
}

function setup() {
  const user = userEvent.setup();
  render(<Harness />);
  return user;
}

describe("ProjectsListSearch", () => {
  it("expands from the magnifier, focuses and filters live", async () => {
    const user = setup();

    expect(screen.queryByPlaceholderText("Search")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Search projects" }));
    const field = screen.getByPlaceholderText("Search");
    expect(field).toHaveFocus();
    await user.type(field, "Apple");
    expect(field).toHaveValue("Apple");
  });

  it("clears text on the first escape and collapses on the second", async () => {
    const user = setup();

    await user.click(screen.getByRole("button", { name: "Search projects" }));
    const field = screen.getByPlaceholderText("Search");
    await user.type(field, "Apple");
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.getByPlaceholderText("Search")).toHaveValue("");
    fireEvent.keyDown(screen.getByPlaceholderText("Search"), { key: "Escape" });
    expect(screen.queryByPlaceholderText("Search")).toBeNull();
    expect(screen.getByRole("button", { name: "Search projects" })).not.toBeNull();
  });

  it("collapses on outside clicks only when blank", async () => {
    const user = setup();

    await user.click(screen.getByRole("button", { name: "Search projects" }));
    fireEvent.pointerDown(screen.getByPlaceholderText("Search"));
    expect(screen.getByPlaceholderText("Search")).not.toBeNull();

    fireEvent.pointerDown(screen.getByRole("button", { name: "Outside" }));
    expect(screen.queryByPlaceholderText("Search")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Search projects" }));
    await user.type(screen.getByPlaceholderText("Search"), "Apple");
    fireEvent.pointerDown(screen.getByRole("button", { name: "Outside" }));
    expect(screen.getByPlaceholderText("Search")).toHaveValue("Apple");
  });

  it("resets and collapses from the clear button", async () => {
    const user = setup();

    await user.click(screen.getByRole("button", { name: "Search projects" }));
    await user.type(screen.getByPlaceholderText("Search"), "Apple");
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(screen.queryByPlaceholderText("Search")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Search projects" }));
    expect(screen.getByPlaceholderText("Search")).toHaveValue("");
  });
});
