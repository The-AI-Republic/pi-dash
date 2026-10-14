// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";
import { ProjectMemberStack, selectStackSplit, type MemberStackEntry } from "./ProjectMemberStack.js";

function entry(id: string, name = `Member ${id}`): MemberStackEntry {
  return { id, name, avatarUrl: null };
}

describe("selectStackSplit", () => {
  it("renders small teams whole", () => {
    expect(selectStackSplit(0)).toEqual({ shown: 0, overflow: 0 });
    expect(selectStackSplit(1)).toEqual({ shown: 1, overflow: 0 });
    expect(selectStackSplit(3)).toEqual({ shown: 3, overflow: 0 });
  });

  it("collapses larger teams past the cap", () => {
    expect(selectStackSplit(4)).toEqual({ shown: 2, overflow: 2 });
    expect(selectStackSplit(9)).toEqual({ shown: 2, overflow: 7 });
  });
});

describe("ProjectMemberStack", () => {
  it("names an empty team", () => {
    render(<ProjectMemberStack members={[]} />);
    expect(screen.getByTestId("project-member-stack-empty")).toHaveTextContent("No members yet");
    expect(screen.queryByTestId("project-member-stack")).toBeNull();
  });

  it("stacks a single member with its accessible count", () => {
    render(<ProjectMemberStack members={[entry("m1", "Ada Lovelace")]} />);
    const stack = screen.getByTestId("project-member-stack");
    expect(stack).toHaveAttribute("aria-label", "1 member");
    expect(within(stack).getByRole("img", { name: "Ada Lovelace" })).not.toBeNull();
    expect(screen.queryByTestId("project-member-stack-overflow")).toBeNull();
  });

  it("renders three members whole with a plural count", () => {
    render(<ProjectMemberStack members={[entry("m1"), entry("m2"), entry("m3")]} />);
    const stack = screen.getByTestId("project-member-stack");
    expect(stack).toHaveAttribute("aria-label", "3 members");
    expect(within(stack).getAllByRole("img")).toHaveLength(3);
  });

  it("collapses four members into two avatars plus an overflow bubble", () => {
    render(<ProjectMemberStack members={[entry("m1"), entry("m2"), entry("m3"), entry("m4")]} />);
    const stack = screen.getByTestId("project-member-stack");
    expect(within(stack).getAllByRole("img")).toHaveLength(2);
    const overflow = screen.getByTestId("project-member-stack-overflow");
    expect(overflow).toHaveTextContent("+2");
    expect(overflow).toHaveAttribute("aria-label", "2 more members");
  });

  it("passes avatar images through to the avatar", () => {
    render(<ProjectMemberStack members={[{ id: "m1", name: "Ada Lovelace", avatarUrl: "https://img.test/a.png" }]} />);
    const image = within(screen.getByTestId("project-member-stack")).getByRole("img", { name: "Ada Lovelace" });
    expect(image.querySelector("img")).toHaveAttribute("src", "https://img.test/a.png");
  });
});
