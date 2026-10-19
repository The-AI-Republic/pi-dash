// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared filter-section chrome (parity/specs/issues.md §9, ISS-094 as
// restated in §22). Every dropdown section — the legacy dimensions and the
// display options — reads the same way: a title with the applied count and a
// chevron that collapses the body, a search box shared by the body, rows
// marked as checkboxes when several can apply or radios when only one can,
// an optional icon per row (which may pulse), a five-row cap with a
// view-all/less control, and an explicit placeholder when nothing matches.

import { ChevronDown, ChevronUp, Search } from "lucide-react";
import * as React from "react";

/** How many rows render before "View all" appears. */
export const FILTER_SECTION_CAP = 5;

/** One row a section offers. */
export interface FilterSectionOption {
  id: string;
  label: string;
  /** Extra text the search also matches, e.g. a short code or group. */
  searchText?: string | undefined;
  /** Leading marker; a live one can pulse. */
  icon?: React.ReactNode | undefined;
  /** The marker animates (the running cycle). */
  pulse?: boolean | undefined;
  /** The row cannot be picked (suspended member, reserved value). */
  disabled?: boolean | undefined;
}

export interface FilterSectionProps {
  title: string;
  /** Multi sections mark every applied row; single sections mark one. */
  kind: "multi" | "single";
  /**
   * How a row announces its kind. `semantic` renders real checkbox/radio
   * roles; `pressed` keeps a button with aria-pressed, which is how the
   * display dropdown has always exposed its choices.
   */
  selectionRole?: "semantic" | "pressed" | undefined;
  /**
   * Rows to show before "View all". The display dropdowns list a fixed
   * short set, so they pass a larger cap; the legacy dimensions keep five.
   */
  cap?: number | undefined;
  /** Applied rows; the title count reads their length. */
  applied: readonly string[];
  /** Title count override; absent reads the applied length. */
  count?: number | undefined;
  /** Marked rows when they are not a filter count (a stored choice that always exists). */
  checked?: readonly string[] | undefined;
  options: readonly FilterSectionOption[];
  onToggle: (id: string) => void;
  /** Controlled collapse, for a host that remembers it. */
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  /** Placeholder while the rows load; the placeholder wins over empty search. */
  loading?: boolean | undefined;
  loadingText?: string | undefined;
  /** Failure text; renders an alert in place of the rows. */
  errorText?: string | undefined;
  /** Untranslated technical id exposed for the parity gate. */
  testId?: string | undefined;
  /** Rows carry their own separator before section content. */
  className?: string | undefined;
  /** Accessible name of a single-choice row group. */
  groupLabel?: string | undefined;
  /**
   * A body that is not a row list (property pills, switches). It keeps the
   * title, count and collapse but has nothing to search or cap.
   */
  children?: React.ReactNode | undefined;
  /**
   * Content that renders below the (capped) rows inside the same body, such
   * as the custom row and calendar of a date section. It is not searched.
   */
  footer?: React.ReactNode | undefined;
}

function matches(option: FilterSectionOption, query: string): boolean {
  if (query === "") return true;
  return `${option.label} ${option.searchText ?? ""}`.toLowerCase().includes(query.toLowerCase());
}

/** One pickable row: a checkbox when several apply, a radio when one does. */
function OptionRow({
  option,
  kind,
  role,
  checked,
  onToggle,
}: {
  option: FilterSectionOption;
  kind: "multi" | "single";
  role: "semantic" | "pressed";
  checked: boolean;
  onToggle: () => void;
}): React.ReactElement {
  const semantics =
    role === "semantic"
      ? { role: kind === "single" ? "radio" : "checkbox", "aria-checked": checked }
      : { "aria-pressed": checked };
  return (
    <button
      type="button"
      {...semantics}
      aria-label={option.label}
      disabled={option.disabled ?? false}
      onClick={onToggle}
      className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-60"
    >
      <span
        aria-hidden="true"
        className={`grid size-3.5 shrink-0 place-items-center border ${
          kind === "single" ? "rounded-full" : "rounded"
        } ${checked ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--border)"}`}
      >
        {checked ? <span className="size-1.5 rounded-full bg-(--on-accent)" /> : null}
      </span>
      {option.icon === undefined ? null : (
        <span
          aria-hidden="true"
          data-testid="filter-option-icon"
          data-pulse={option.pulse === true ? "true" : "false"}
          className={`inline-flex shrink-0 items-center ${option.pulse === true ? "animate-pulse" : ""}`}
        >
          {option.icon}
        </span>
      )}
      <span className="text-body truncate text-(--text)">{option.label}</span>
    </button>
  );
}

/** Props of the section frame: the title bar and the body it reveals. */
export interface FilterSectionFrameProps {
  title: string;
  /** Rendered heading text; usually the title with its applied count. */
  countLabel: string;
  /** Controlled open state; absent keeps the frame's own. */
  open?: boolean | undefined;
  onToggle?: (() => void) | undefined;
  testId?: string | undefined;
  className?: string | undefined;
  children: React.ReactNode;
}

/**
 * The section's title bar and collapsible body, without the row machinery.
 * The shared section wraps it; a host whose body is not a pickable list
 * (the display panel's pills and switches) uses it directly so the chrome
 * still matches.
 */
export function FilterSectionFrame({
  title,
  countLabel,
  open,
  onToggle,
  testId,
  className,
  children,
}: FilterSectionFrameProps): React.ReactElement {
  const [ownOpen, setOwnOpen] = React.useState(true);
  const expanded = open ?? ownOpen;
  const toggle = onToggle ?? ((): void => setOwnOpen(!ownOpen));
  return (
    <section data-testid={testId} data-section-title={title} aria-label={title} className={className}>
      <div className="flex items-center justify-between gap-(--space-2)">
        <h3 className="text-caption text-(--text-muted)">{countLabel}</h3>
        <button
          type="button"
          aria-label={`${expanded ? "Collapse" : "Expand"} ${title} section`}
          aria-expanded={expanded}
          onClick={toggle}
          className="grid size-5 shrink-0 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle)"
        >
          {expanded ? <ChevronUp size={14} aria-hidden="true" /> : <ChevronDown size={14} aria-hidden="true" />}
        </button>
      </div>
      {expanded ? <div className="mt-(--space-1)">{children}</div> : null}
    </section>
  );
}

/**
 * The shared section. It owns collapse, the search text, and the cap; the
 * host owns which rows exist and which are applied.
 */
export function FilterSection({
  title,
  kind,
  selectionRole = "semantic",
  cap = FILTER_SECTION_CAP,
  applied,
  count: countOverride,
  checked: checkedOverride,
  options,
  onToggle,
  open: controlledOpen,
  onOpenChange,
  loading = false,
  loadingText = "Loading values",
  errorText,
  testId,
  className,
  groupLabel,
  children,
  footer,
}: FilterSectionProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(true);
  const open = controlledOpen ?? uncontrolledOpen;
  const [query, setQuery] = React.useState("");
  const [expanded, setExpanded] = React.useState(false);

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const appliedSet = new Set(checkedOverride ?? applied);
  const visible = options.filter((option) => matches(option, query));
  const capped = expanded ? visible : visible.slice(0, cap);
  const count = countOverride ?? applied.length;
  const countLabel = count > 0 ? `${title} (${count})` : title;
  return (
    <FilterSectionFrame
      title={title}
      countLabel={countLabel}
      open={open}
      onToggle={() => setOpen(!open)}
      testId={testId}
      className={className}
    >
      {open ? (
        <div
          className="mt-(--space-1)"
          {...(selectionRole === "semantic" && kind === "single" && groupLabel !== undefined
            ? { role: "radiogroup", "aria-label": groupLabel }
            : {})}
        >
          {children !== undefined ? (
            children
          ) : (
            <>
              <label className="mb-(--space-1) flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-2)">
                <Search size={12} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
                <input
                  type="search"
                  value={query}
                  onChange={(event) => {
                    setQuery(event.target.value);
                    setExpanded(false);
                  }}
                  aria-label={`Search ${title}`}
                  placeholder={`Search ${title.toLowerCase()}`}
                  className="text-body w-full bg-transparent py-(--space-1) text-(--text) outline-none placeholder:text-(--text-placeholder)"
                />
              </label>
              {errorText !== undefined ? (
                <p role="alert" className="text-caption px-(--space-3) py-(--space-2) text-(--danger)">
                  {errorText}
                </p>
              ) : loading ? (
                <p className="text-caption px-(--space-3) py-(--space-2) text-(--text-muted)">{loadingText}</p>
              ) : visible.length === 0 ? (
                <p className="text-caption px-(--space-3) py-(--space-2) text-(--text-muted) italic">
                  No matches found
                </p>
              ) : (
                <>
                  {capped.map((option) => (
                    <OptionRow
                      key={option.id}
                      option={option}
                      kind={kind}
                      role={selectionRole}
                      checked={appliedSet.has(option.id)}
                      onToggle={() => onToggle(option.id)}
                    />
                  ))}
                  {visible.length > cap ? (
                    <button
                      type="button"
                      onClick={() => setExpanded(!expanded)}
                      className="text-caption px-(--space-3) py-(--space-1) font-medium text-(--accent)"
                    >
                      {expanded ? "View less" : "View all"}
                    </button>
                  ) : null}
                </>
              )}
              {footer === undefined ? null : <div className="mt-(--space-1) border-t border-(--border)">{footer}</div>}
            </>
          )}
        </div>
      ) : null}
    </FilterSectionFrame>
  );
}
