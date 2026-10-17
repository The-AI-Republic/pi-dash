// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Button } from "./Button";
import { Combobox, filterComboboxOptions, shouldAutoFocusSearch, type ComboboxOption } from "./Combobox";
import { IconButton } from "./IconButton";

const OPTIONS: ComboboxOption[] = [
  { id: "urgent", label: "Urgent" },
  { id: "high", label: "High" },
  { id: "medium", label: "Medium" },
  { id: "low", label: "Low" },
  { id: "none", label: "None", searchText: "no priority unset" },
];

function setup(props: Partial<React.ComponentProps<typeof Combobox>> = {}) {
  const onSelect = vi.fn();
  const onOpenChange = vi.fn();
  const view = render(
    <Combobox
      trigger={<Button>Priority</Button>}
      options={OPTIONS}
      value={null}
      onSelect={onSelect}
      listLabel="Priorities"
      emptyText="No matching results"
      onOpenChange={onOpenChange}
      {...props}
    />
  );
  return { onSelect, onOpenChange, ...view };
}

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.textContent ?? "");
}

afterEach(() => {
  vi.unstubAllGlobals();
  const proto = Element.prototype as unknown as Record<string, unknown>;
  if ("scrollIntoView" in proto) delete proto["scrollIntoView"];
});

describe("filterComboboxOptions", () => {
  it("returns every option for a blank query", () => {
    expect(filterComboboxOptions(OPTIONS, "  ")).toEqual(OPTIONS);
  });

  it("matches labels case-insensitively", () => {
    expect(filterComboboxOptions(OPTIONS, "URG").map((option) => option.id)).toEqual(["urgent"]);
  });

  it("matches the extra search text", () => {
    expect(filterComboboxOptions(OPTIONS, "unset").map((option) => option.id)).toEqual(["none"]);
  });

  it("matches nothing on a miss", () => {
    expect(filterComboboxOptions(OPTIONS, "zzz")).toEqual([]);
  });
});

describe("shouldAutoFocusSearch", () => {
  it("focuses by default", () => {
    expect(shouldAutoFocusSearch()).toBe(true);
  });

  it("skips focusing on coarse pointers", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    expect(shouldAutoFocusSearch()).toBe(false);
  });

  it("focuses on fine pointers", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    expect(shouldAutoFocusSearch()).toBe(true);
  });
});

describe("Combobox", () => {
  it("opens on trigger click with a focused search and all options", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    const search = screen.getByRole("combobox", { name: "Search options" });
    expect(search).toHaveFocus();
    expect(search).toHaveAttribute("aria-expanded", "true");
    expect(optionNames()).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
  });

  it("opens on Enter and toggles shut on Enter again", async () => {
    const user = userEvent.setup();
    setup();
    const trigger = screen.getByRole("button", { name: "Priority" });
    trigger.focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("listbox", { name: "Priorities" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("notifies open changes", () => {
    const { onOpenChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
  });

  it("filters on search and names the empty state", async () => {
    const user = userEvent.setup();
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    await user.type(screen.getByRole("combobox"), "urg");
    expect(optionNames()).toEqual(["Urgent"]);
    await user.type(screen.getByRole("combobox"), "zzz");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
  });

  it("clears the query on first Escape and closes on the second", async () => {
    const user = userEvent.setup();
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    const search = screen.getByRole("combobox");
    await user.type(search, "urg");
    expect(optionNames()).toEqual(["Urgent"]);
    await user.keyboard("{Escape}");
    expect(screen.getByRole("combobox")).toHaveValue("");
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    expect(optionNames()).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("forgets the query after closing", async () => {
    const user = userEvent.setup();
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    await user.type(screen.getByRole("combobox"), "urg");
    await user.keyboard("{Escape}");
    await user.keyboard("{Escape}");
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(screen.getByRole("combobox")).toHaveValue("");
    expect(optionNames()).toHaveLength(5);
  });

  it("closes on outside click and on Tab", async () => {
    const user = userEvent.setup();
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    await user.click(document.body);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    await user.tab();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("moves the highlight with arrows and picks with Enter", async () => {
    const user = userEvent.setup();
    const { onSelect } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    const search = screen.getByRole("combobox");
    expect(search).toHaveAttribute("aria-activedescendant", expect.stringContaining("-option-0"));
    await user.keyboard("{ArrowDown}");
    expect(search).toHaveAttribute("aria-activedescendant", expect.stringContaining("-option-1"));
    await user.keyboard("{ArrowUp}");
    await user.keyboard("{ArrowUp}");
    expect(search).toHaveAttribute("aria-activedescendant", expect.stringContaining("-option-0"));
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith("urgent");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Priority" })).toHaveFocus();
  });

  it("skips disabled options in highlight and activation", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <Combobox
        trigger={<Button>Members</Button>}
        options={[
          { id: "ana", label: "Ana" },
          { id: "zoe", label: "Zoe", disabled: true },
          { id: "max", label: "Max" },
        ]}
        value={null}
        onSelect={onSelect}
        listLabel="Members"
        emptyText="No matching results"
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    const search = screen.getByRole("combobox");
    expect(search).toHaveAttribute("aria-activedescendant", expect.stringContaining("-option-0"));
    await user.keyboard("{ArrowDown}");
    expect(search).toHaveAttribute("aria-activedescendant", expect.stringContaining("-option-2"));
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith("max");
    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    fireEvent.click(screen.getByRole("option", { name: "Zoe" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("does nothing on Enter when no option can activate", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <Combobox
        trigger={<Button>Members</Button>}
        options={[{ id: "zoe", label: "Zoe", disabled: true }]}
        value={null}
        onSelect={onSelect}
        listLabel="Members"
        emptyText="No matching results"
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    await user.keyboard("{Enter}");
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByRole("listbox")).toBeInTheDocument();
  });

  it("does not double-fire Enter on a focused option", async () => {
    const user = userEvent.setup();
    const { onSelect } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    screen.getByRole("option", { name: "High" }).focus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith("high");
  });

  it("keeps multi-select open across picks", () => {
    const onSelect = vi.fn();
    render(
      <Combobox
        trigger={<Button>Assignees</Button>}
        options={OPTIONS}
        value={["high"]}
        multiple
        onSelect={onSelect}
        listLabel="Assignees"
        emptyText="No matching results"
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Assignees" }));
    fireEvent.click(screen.getByRole("option", { name: "Urgent" }));
    expect(onSelect).toHaveBeenCalledWith("urgent");
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "High" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("option", { name: "Urgent" })).toHaveAttribute("aria-selected", "false");
  });

  it("marks the single selection", () => {
    setup({ value: "low" });
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(screen.getByRole("option", { name: "Low" })).toHaveAttribute("aria-selected", "true");
  });

  it("renders icons, trailing marks and rich content", () => {
    render(
      <Combobox
        trigger={<Button>Members</Button>}
        options={[
          {
            id: "ana",
            label: "Ana",
            icon: <i data-testid="avatar-ana" />,
            trailing: <span data-testid="pill-ana">Suspended</span>,
          },
          { id: "max", label: "Max", content: <strong data-testid="rich-max">Max Power</strong> },
        ]}
        value={null}
        onSelect={() => undefined}
        listLabel="Members"
        emptyText="No matching results"
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    expect(screen.getByTestId("avatar-ana")).toBeInTheDocument();
    expect(screen.getByTestId("pill-ana")).toBeInTheDocument();
    expect(screen.getByTestId("rich-max")).toBeInTheDocument();
  });

  it("shows the loading row while options resolve", () => {
    setup({ loading: true, loadingText: "Loading members", options: [] });
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading members");
  });

  it("shows the empty text for an empty list", () => {
    setup({ options: [] });
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
  });

  it("reports load failures instead of the list", () => {
    setup({ errorText: "Couldn't load members.", loading: true });
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Couldn't load members.");
  });

  it("disables the trigger with a not-allowed cursor and blocks opening", () => {
    const { onOpenChange } = setup({ disabled: true });
    const trigger = screen.getByRole("button", { name: "Priority" });
    expect(trigger).toBeDisabled();
    expect(trigger.className).toContain("cursor-not-allowed");
    fireEvent.click(trigger);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("leaves an already-disabled trigger untouched", () => {
    render(
      <Combobox
        trigger={
          <Button disabled className="custom">
            Priority
          </Button>
        }
        options={OPTIONS}
        value={null}
        onSelect={() => undefined}
        listLabel="Priorities"
        emptyText="No matching results"
        disabled
      />
    );
    expect(screen.getByRole("button", { name: "Priority" }).className).toContain("custom");
  });

  it("accepts icon-only and custom triggers", () => {
    const onSelect = vi.fn();
    const { rerender } = render(
      <Combobox
        trigger={
          <IconButton label="Pick priority">
            <span aria-hidden="true">!</span>
          </IconButton>
        }
        options={OPTIONS}
        value={null}
        onSelect={onSelect}
        listLabel="Priorities"
        emptyText="No matching results"
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Pick priority" }));
    fireEvent.click(screen.getByRole("option", { name: "Low" }));
    expect(onSelect).toHaveBeenCalledWith("low");

    rerender(
      <Combobox
        trigger={<span data-testid="custom-trigger">Urgent</span>}
        options={OPTIONS}
        value={null}
        onSelect={onSelect}
        listLabel="Priorities"
        emptyText="No matching results"
      />
    );
    fireEvent.click(screen.getByTestId("custom-trigger"));
    expect(screen.getByRole("listbox")).toBeInTheDocument();
  });

  it("supports controlled open state", () => {
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <Combobox
        trigger={<Button>Priority</Button>}
        options={OPTIONS}
        value={null}
        onSelect={() => undefined}
        listLabel="Priorities"
        emptyText="No matching results"
        open={false}
        onOpenChange={onOpenChange}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    rerender(
      <Combobox
        trigger={<Button>Priority</Button>}
        options={OPTIONS}
        value={null}
        onSelect={() => undefined}
        listLabel="Priorities"
        emptyText="No matching results"
        open
        onOpenChange={onOpenChange}
      />
    );
    expect(screen.getByRole("listbox")).toBeInTheDocument();
  });

  it("renders a bare keyboard-navigable list when search is off", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <Combobox
        trigger={<Button>Layout</Button>}
        options={OPTIONS}
        value={null}
        onSelect={onSelect}
        listLabel="Layouts"
        emptyText="No matching results"
        searchEnabled={false}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Layout" }));
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    const list = screen.getByRole("listbox", { name: "Layouts" });
    expect(list).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(list).toHaveAttribute("aria-activedescendant", expect.stringContaining("-option-1"));
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith("high");
  });

  it("closes on Escape from a focused option", async () => {
    const user = userEvent.setup();
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    screen.getByRole("option", { name: "High" }).focus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("skips focusing the search on coarse pointers", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    expect(screen.getByRole("combobox")).not.toHaveFocus();
  });

  it("keeps the highlight visible while navigating", async () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(Element.prototype, "scrollIntoView", { value: scrollIntoView, configurable: true });
    const user = userEvent.setup();
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    scrollIntoView.mockClear();
    await user.keyboard("{ArrowDown}");
    expect(scrollIntoView).toHaveBeenCalled();
  });
});
