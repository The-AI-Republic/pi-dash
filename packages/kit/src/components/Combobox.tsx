// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Generic searchable dropdown (parity/specs/issues.md §17, ISS-207): a
// caller-rendered trigger opening a popup with a search field and an
// option list. Single- or multi-select; multi stays open across picks.
// Keyboard: Enter toggles the trigger and picks the highlighted option,
// Escape clears a non-empty query first and otherwise closes, Tab
// closes, arrows move the highlight. The search field takes focus on
// open except on coarse-pointer devices. Outside-click closes through
// the Base popover. Disabled blocks opening and renders the trigger
// with a not-allowed cursor, so no clear action is reachable.
import { Popover as BasePopover } from "@base-ui-components/react/popover";
import { Check } from "lucide-react";
import * as React from "react";
import { cn } from "../lib/cn";
import { pickDefined } from "../lib/props";
import { FOCUS_RING } from "../lib/styles";
import { fieldInputClasses } from "./FieldScaffold";
import { Spinner } from "./Spinner";

/** One row in the dropdown list. */
export interface ComboboxOption {
  /** Stable id reported to onSelect; unique within the list. */
  id: string;
  /** Accessible name of the option. */
  label: string;
  /** Extra searchable text (a display name behind a short label). */
  searchText?: string;
  /** Suspended and similar rows render but never activate. */
  disabled?: boolean;
  /** Leading mark (avatar, icon, logo tile); hidden from assistive tech. */
  icon?: React.ReactNode;
  /** Trailing mark (a state pill); hidden from assistive tech. */
  trailing?: React.ReactNode;
  /** Rich row body replacing the plain label. */
  content?: React.ReactNode;
}

export interface ComboboxProps {
  /**
   * Element rendered as the dropdown trigger: any button variant, an
   * icon-only button, or a custom node. It must carry an accessible
   * name and forward props to its root (kit Button and IconButton do).
   */
  trigger: React.ReactElement<Record<string, unknown>>;
  options: ComboboxOption[];
  /** Single-select id (or null) / multi-select ids. */
  value: string | readonly string[] | null;
  multiple?: boolean;
  /** Fired with the picked id; multi callers toggle it into value. */
  onSelect: (id: string) => void;
  /** Accessible name of the option list. */
  listLabel: string;
  /** Accessible name of the search field. */
  searchLabel?: string;
  /** False renders a bare list (the layout picker has no search). */
  searchEnabled?: boolean;
  /** Text shown when no option matches (per-picker wording). */
  emptyText: string;
  /** True while the caller loads options; shows the loading row. */
  loading?: boolean;
  /** Visible loading text next to the spinner. */
  loadingText?: string;
  /** Failure text; renders an alert row instead of the list when set. */
  errorText?: string | undefined;
  disabled?: boolean;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  /** Extra popup classes (callers widen it for long labels). */
  className?: string | undefined;
}

const popupClasses = "w-64 rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-2) shadow-popover";

const optionClasses =
  "flex w-full cursor-default items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-body text-left text-(--text) outline-none not-data-[disabled]:hover:bg-(--subtle) data-[active]:bg-(--subtle) data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50";

/** Case-insensitive substring match over label plus search text. */
export function filterComboboxOptions(options: ComboboxOption[], query: string): ComboboxOption[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return options;
  return options.filter((option) => `${option.label} ${option.searchText ?? ""}`.toLowerCase().includes(needle));
}

/**
 * The search field takes focus on open, except on coarse-pointer
 * devices where the on-screen keyboard would cover the list. A missing
 * matchMedia (tests, SSR) keeps the focusing behavior.
 */
export function shouldAutoFocusSearch(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return true;
  return !window.matchMedia("(pointer: coarse)").matches;
}

export function Combobox({
  trigger,
  options,
  value,
  multiple = false,
  onSelect,
  listLabel,
  searchLabel = "Search options",
  searchEnabled = true,
  emptyText,
  loading = false,
  loadingText = "Loading",
  errorText,
  disabled = false,
  open: controlledOpen,
  onOpenChange,
  className,
}: ComboboxProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const [activePosition, setActivePosition] = React.useState(0);
  const searchRef = React.useRef<HTMLInputElement>(null);
  const listRef = React.useRef<HTMLUListElement>(null);
  const baseId = React.useId();
  const listId = `${baseId}-list`;

  const open = controlledOpen ?? uncontrolledOpen;

  function requestOpen(next: boolean): void {
    if (disabled && next) return;
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const filtered = React.useMemo(() => filterComboboxOptions(options, query), [options, query]);
  const enabledIds = React.useMemo(
    () => filtered.filter((option) => !option.disabled).map((option) => option.id),
    [filtered]
  );
  const activeId = enabledIds[Math.min(activePosition, Math.max(enabledIds.length - 1, 0))] ?? null;
  const activeIndex = activeId === null ? -1 : filtered.findIndex((option) => option.id === activeId);

  function isSelected(id: string): boolean {
    return multiple ? (value as readonly string[]).includes(id) : value === id;
  }

  function choose(id: string): void {
    onSelect(id);
    if (!multiple) requestOpen(false);
  }

  // A closing popup forgets its query; a fresh query re-highlights the
  // first enabled match so Enter always has a target.
  React.useEffect(() => {
    if (!open) setQuery("");
    setActivePosition(0);
  }, [open, query, options]);

  // Base moves focus on open; point it at the search field (or the bare
  // list when search is off). Coarse pointers keep focus on the trigger
  // so the on-screen keyboard never covers the list. The callback refs
  // below repeat the same move when the popup mounts late, after Base's
  // own focus pass has already run.
  function resolveInitialFocus(): HTMLElement | false | null {
    if (!searchEnabled) return listRef.current;
    if (!shouldAutoFocusSearch()) return false;
    return searchRef.current;
  }

  // Stable across renders within one open session so typing and picking
  // never yank focus back; re-created per open so mounting focuses once.
  const focusSearch = React.useCallback(
    (node: HTMLInputElement | null) => {
      searchRef.current = node;
      if (node !== null && open && shouldAutoFocusSearch()) node.focus();
    },
    [open]
  );

  const focusList = React.useCallback(
    (node: HTMLUListElement | null) => {
      listRef.current = node;
      if (node !== null && open && !searchEnabled) node.focus();
    },
    [open, searchEnabled]
  );

  React.useEffect(() => {
    if (!open || activeIndex < 0) return;
    const node = document.getElementById(`${listId}-option-${activeIndex}`);
    if (node !== null && typeof node.scrollIntoView === "function") node.scrollIntoView({ block: "nearest" });
  }, [open, activeIndex, listId]);

  function moveHighlight(delta: -1 | 1): void {
    setActivePosition((position) => Math.min(Math.max(position + delta, 0), Math.max(enabledIds.length - 1, 0)));
  }

  function onPopupKeyDown(event: React.KeyboardEvent): void {
    if (event.key === "Tab") {
      requestOpen(false);
      return;
    }
    if (event.key === "Escape") {
      requestOpen(false);
      return;
    }
    const target = event.target as Element | null;
    if (target !== null && typeof target.closest === "function" && target.closest('[role="option"]') !== null) {
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveHighlight(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moveHighlight(-1);
    } else if (event.key === "Enter") {
      if (activeId !== null) {
        event.preventDefault();
        choose(activeId);
      }
    }
  }

  const renderedTrigger =
    disabled && trigger.props["disabled"] !== true
      ? React.cloneElement(trigger, {
          disabled: true,
          className: cn(trigger.props["className"] as string | undefined, "cursor-not-allowed"),
        })
      : trigger;

  return (
    <BasePopover.Root {...pickDefined({ open, onOpenChange: (next: boolean) => requestOpen(next) })}>
      <BasePopover.Trigger render={renderedTrigger} />
      <BasePopover.Portal>
        <BasePopover.Positioner sideOffset={4} align="start">
          <BasePopover.Popup
            className={cn(popupClasses, className)}
            initialFocus={resolveInitialFocus}
            onKeyDown={onPopupKeyDown}
          >
            {searchEnabled ? (
              <input
                ref={focusSearch}
                role="combobox"
                aria-label={searchLabel}
                aria-expanded={open}
                aria-controls={listId}
                aria-activedescendant={activeIndex < 0 ? undefined : `${listId}-option-${activeIndex}`}
                autoComplete="off"
                spellCheck={false}
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Escape" && query !== "") {
                    event.stopPropagation();
                    setQuery("");
                  }
                }}
                className={cn(fieldInputClasses, FOCUS_RING, "text-body h-8")}
              />
            ) : null}
            {errorText !== undefined ? (
              <p role="alert" className="text-body px-(--space-3) py-(--space-4) text-(--danger)">
                {errorText}
              </p>
            ) : loading ? (
              <div className="text-body flex items-center gap-(--space-2) px-(--space-3) py-(--space-4) text-(--text-muted)">
                <Spinner label={loadingText} size="small" />
                <span aria-hidden="true">{loadingText}</span>
              </div>
            ) : filtered.length === 0 ? (
              <p role="status" className="text-body px-(--space-3) py-(--space-4) text-(--text-muted)">
                {emptyText}
              </p>
            ) : (
              <ul
                ref={focusList}
                id={listId}
                role="listbox"
                aria-label={listLabel}
                aria-activedescendant={activeIndex < 0 ? undefined : `${listId}-option-${activeIndex}`}
                tabIndex={searchEnabled ? undefined : -1}
                className={cn("max-h-64 overflow-y-auto outline-none", searchEnabled && "mt-(--space-2)")}
              >
                {filtered.map((option, index) => {
                  const selected = isSelected(option.id);
                  return (
                    <li key={option.id}>
                      <button
                        type="button"
                        role="option"
                        id={`${listId}-option-${index}`}
                        aria-selected={selected}
                        disabled={option.disabled ?? false}
                        data-active={option.id === activeId || undefined}
                        onClick={() => {
                          choose(option.id);
                        }}
                        className={optionClasses}
                      >
                        {option.icon === undefined ? null : (
                          <span aria-hidden="true" className="inline-flex shrink-0 items-center">
                            {option.icon}
                          </span>
                        )}
                        <span className="min-w-0 flex-1 truncate">{option.content ?? option.label}</span>
                        {selected ? (
                          <span aria-hidden="true" className="inline-flex shrink-0 items-center text-(--accent)">
                            <Check size={16} strokeWidth={1.5} />
                          </span>
                        ) : null}
                        {option.trailing === undefined ? null : (
                          <span aria-hidden="true" className="inline-flex shrink-0 items-center">
                            {option.trailing}
                          </span>
                        )}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </BasePopover.Popup>
        </BasePopover.Positioner>
      </BasePopover.Portal>
    </BasePopover.Root>
  );
}
