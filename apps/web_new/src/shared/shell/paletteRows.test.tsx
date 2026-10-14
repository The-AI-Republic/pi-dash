// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PidashCommand } from "../commands/index.js";
import { groupCommands, PaletteOption } from "./paletteRows.js";

function command(id: string, section?: string): PidashCommand {
  return { id, title: id, ...(section === undefined ? {} : { section }), run: () => undefined };
}

describe("groupCommands", () => {
  it("groups under first-seen headings with flat indices", () => {
    const groups = groupCommands([
      command("a", "Account"),
      command("b", "General"),
      command("c", "Account"),
      command("d"),
    ]);
    expect(groups.map((group) => group.section)).toEqual(["Account", "General"]);
    expect(groups[0]?.entries).toEqual([
      { command: expect.objectContaining({ id: "a" }), index: 0 },
      { command: expect.objectContaining({ id: "c" }), index: 2 },
    ]);
    expect(groups[1]?.entries).toEqual([
      { command: expect.objectContaining({ id: "b" }), index: 1 },
      { command: expect.objectContaining({ id: "d" }), index: 3 },
    ]);
  });

  it("groups an empty list into nothing", () => {
    expect(groupCommands([])).toEqual([]);
  });
});

describe("PaletteOption", () => {
  it("marks the active row and shows hint plus shortcut", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const onHover = vi.fn();
    render(
      <ul>
        <PaletteOption
          active
          title="Sign out"
          hint="ada@example.com"
          shortcut="mod+shift+q"
          onHover={onHover}
          onSelect={onSelect}
        />
      </ul>
    );
    const option = screen.getByRole("option", { name: /Sign out/ });
    expect(option).toHaveAttribute("aria-selected", "true");
    expect(option.className).toContain("bg-(--subtle)");
    expect(screen.getByText("ada@example.com")).toBeInTheDocument();
    await user.hover(option);
    expect(onHover).toHaveBeenCalledOnce();
    await user.click(option);
    expect(onSelect).toHaveBeenCalledOnce();
  });

  it("renders a plain idle row without hint or shortcut", () => {
    render(
      <ul>
        <PaletteOption active={false} title="Go home" onHover={() => undefined} onSelect={() => undefined} />
      </ul>
    );
    const option = screen.getByRole("option", { name: "Go home" });
    expect(option).toHaveAttribute("aria-selected", "false");
    expect(option.className).not.toContain("bg-(--subtle)");
  });
});
