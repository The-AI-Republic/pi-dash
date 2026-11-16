// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project tab bar (parity/specs/shell.md §7, SHELL-075/076/078/079). The
// visible tabs render as links with context menus; narrow widths pool
// trailing tabs behind the overflow trigger with the user-hidden ones,
// and the active tab pins itself into view.

import { Link } from "@tanstack/react-router";
import { ContextMenu, IconButton, Popover } from "@pidash/kit";
import { Ellipsis, Eye, Star } from "lucide-react";
import * as React from "react";

import { countFittingTabs, resolveOverflow, type ProjectTab } from "../tabs.js";

/** Gap between tab cells, matching the row below. */
const TAB_GAP_PX = 4;
/** Width kept for the overflow trigger while tabs pool. */
const OVERFLOW_RESERVE_PX = 40;

export interface TabBarProps {
  tabs: ProjectTab[];
  activeKey: string | null;
  defaultTab: string;
  hiddenTabs: string[];
  onToggleDefault: (tabKey: string) => void;
  onHideTab: (tabKey: string) => void;
  onShowTab: (tabKey: string) => void;
}

export function TabBar({
  tabs,
  activeKey,
  defaultTab,
  hiddenTabs,
  onToggleDefault,
  onHideTab,
  onShowTab,
}: TabBarProps): React.ReactElement {
  const candidates = tabs.filter((tab) => !hiddenTabs.includes(tab.key));
  const hidden = tabs.filter((tab) => hiddenTabs.includes(tab.key));
  const itemRefs = React.useRef<Array<HTMLDivElement | null>>([]);
  // Last measured cell widths by candidate index. Pooled tabs unmount,
  // so their widths persist here instead of reading zero and oscillating
  // the count between passes.
  const knownWidths = React.useRef<number[]>([]);
  const [measuredWidth, setMeasuredWidth] = React.useState<number | null>(null);
  const [visibleCount, setVisibleCount] = React.useState(candidates.length);

  // Membership signature: a re-fit follows flag and role changes, not
  // only resizes.
  const candidateKey = candidates.map((tab) => tab.key).join(",");

  const containerRef = React.useRef<HTMLDivElement | null>(null);

  React.useEffect(() => {
    if (typeof ResizeObserver === "undefined") return;
    // Post-mount the container is always attached; StrictMode remounts
    // re-observe through the cleanup below.
    const node = containerRef.current as HTMLDivElement;
    const observer = new ResizeObserver((entries) => {
      const first = entries[0];
      if (first) setMeasuredWidth(first.contentRect.width);
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  React.useEffect(() => {
    // Unmeasured (first paint, or no observer API): show every candidate.
    if (measuredWidth === null) return;
    for (let index = 0; index < candidates.length; index += 1) {
      const cell = itemRefs.current[index];
      if (cell) knownWidths.current[index] = cell.offsetWidth;
    }
    const widths = candidates.map((_, index) => knownWidths.current[index] ?? 0);
    setVisibleCount(
      countFittingTabs({
        itemWidths: widths,
        gap: TAB_GAP_PX,
        overflowWidth: OVERFLOW_RESERVE_PX,
        containerWidth: measuredWidth,
      })
    );
  }, [measuredWidth, candidates.length, candidateKey, visibleCount]);

  const { visible, overflow, hasOverflow } = resolveOverflow({ candidates, hidden, visibleCount, activeKey });

  return (
    <div ref={containerRef} className="flex min-w-0 flex-1 items-center gap-1">
      {visible.map((tab) => {
        const candidateIndex = candidates.indexOf(tab);
        const active = tab.key === activeKey;
        const isDefault = tab.key === defaultTab;
        return (
          <div
            key={tab.key}
            ref={(element) => {
              itemRefs.current[candidateIndex] = element;
            }}
            className="relative flex shrink-0 items-center"
          >
            {active ? (
              <span aria-hidden="true" className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-(--accent)" />
            ) : null}
            <ContextMenu
              trigger={
                <span className="flex items-center">
                  <Link
                    to={tab.href}
                    aria-current={active ? "page" : undefined}
                    className={`text-body flex items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) whitespace-nowrap hover:bg-(--subtle) ${
                      active ? "text-(--text)" : "text-(--text-muted)"
                    }`}
                  >
                    {tab.name}
                    {isDefault ? (
                      <span aria-hidden="true" className="ml-(--space-2) inline-block size-1 rounded-full bg-current" />
                    ) : null}
                  </Link>
                </span>
              }
              items={[
                {
                  label: isDefault ? "Clear default" : "Set as default",
                  onSelect: () => onToggleDefault(tab.key),
                },
                { label: "Hide in more menu", onSelect: () => onHideTab(tab.key) },
              ]}
            />
          </div>
        );
      })}
      {hasOverflow ? (
        <Popover
          trigger={
            <IconButton label="More project tabs" data-testid="tab-overflow-trigger">
              <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
            </IconButton>
          }
        >
          <div data-testid="tab-overflow-menu" className="flex flex-col gap-(--space-1)">
            {overflow.map((tab) => {
              const concealed = hiddenTabs.includes(tab.key);
              const isDefault = tab.key === defaultTab;
              return (
                <div key={tab.key} className="flex items-center gap-(--space-1)">
                  <Link
                    to={tab.href}
                    aria-current={tab.key === activeKey ? "page" : undefined}
                    className="text-body min-w-0 flex-1 truncate rounded-(--radius-control) px-(--space-2) py-(--space-2) text-(--text) hover:bg-(--subtle)"
                  >
                    {tab.name}
                  </Link>
                  {concealed ? (
                    <IconButton label={`Show ${tab.name}`} title="Show" onClick={() => onShowTab(tab.key)}>
                      <Eye size={14} strokeWidth={1.5} aria-hidden="true" />
                    </IconButton>
                  ) : null}
                  <IconButton
                    label={isDefault ? `Clear ${tab.name} as the default tab` : `Set ${tab.name} as the default tab`}
                    title={isDefault ? "Clear default" : "Set as default"}
                    onClick={() => onToggleDefault(tab.key)}
                  >
                    <Star size={14} strokeWidth={1.5} aria-hidden="true" fill={isDefault ? "currentColor" : "none"} />
                  </IconButton>
                </div>
              );
            })}
          </div>
        </Popover>
      ) : null}
    </div>
  );
}
