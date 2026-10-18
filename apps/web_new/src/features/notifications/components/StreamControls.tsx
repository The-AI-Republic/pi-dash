// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inbox stream controls (written from parity/specs/notifications.md §7,
// §8; NTF-013-016). The inbox header carries four controls: refresh,
// mark-all-read, the origin filter menu and the overflow mode menu.
// Refresh and mark-all-read disable themselves while their own request
// runs, so a repeat press cannot start a second flight; both show a
// spinner while they work. The origin menu is a multi-select with
// checkmarks, the overflow menu three rows whose checkmarks follow the
// active modes.
import { Check, ChevronDown, MoreVertical, RefreshCw } from "lucide-react";
import { Button, IconButton, Menu, Spinner, Tooltip } from "@pidash/kit";
import * as React from "react";

import {
  isModeEnabled,
  NOTIFICATION_MODES,
  NOTIFICATION_MODE_LABELS,
  NOTIFICATION_ORIGIN_LABELS,
  NOTIFICATION_ORIGINS,
  type NotificationOrigin,
  type OriginFilter,
  type StreamMode,
  type StreamModes,
} from "../lib/stream.js";

export interface StreamControlsProps {
  origins: OriginFilter;
  modes: StreamModes;
  /** A refresh request is in flight. */
  refreshing: boolean;
  /** A bulk mark-all-read request is in flight. */
  markingRead: boolean;
  onRefresh: () => void;
  onMarkAllRead: () => void;
  onToggleOrigin: (origin: NotificationOrigin) => void;
  onToggleMode: (mode: StreamMode) => void;
}

/** One checkmark column: empty, so a row keeps its width when toggled. */
function CheckSlot({ checked }: { checked: boolean }): React.ReactElement {
  return (
    <span
      data-testid="stream-control-check"
      data-checked={checked ? "true" : "false"}
      aria-hidden="true"
      className="ml-auto inline-flex w-4 justify-center"
    >
      {checked ? <Check size={14} aria-hidden="true" /> : null}
    </span>
  );
}

export function StreamControls({
  origins,
  modes,
  refreshing,
  markingRead,
  onRefresh,
  onMarkAllRead,
  onToggleOrigin,
  onToggleMode,
}: StreamControlsProps): React.ReactElement {
  return (
    <div data-testid="inbox-stream-controls" className="flex shrink-0 items-center justify-end gap-(--space-1)">
      <Tooltip content="Mark all as read">
        <IconButton
          label="Mark all as read"
          data-testid="inbox-mark-all-read"
          aria-busy={markingRead || undefined}
          disabled={markingRead}
          onClick={onMarkAllRead}
          className="relative"
        >
          <Check size={16} aria-hidden="true" className={markingRead ? "invisible" : undefined} />
          {markingRead ? (
            <span data-testid="inbox-mark-all-read-progress" className="absolute inline-flex">
              <Spinner size="small" label="Marking all as read" />
            </span>
          ) : null}
        </IconButton>
      </Tooltip>
      <Tooltip content="Refresh">
        <IconButton
          label="Refresh"
          data-testid="inbox-refresh"
          aria-busy={refreshing || undefined}
          disabled={refreshing}
          onClick={onRefresh}
        >
          <RefreshCw
            data-testid={refreshing ? "inbox-refresh-progress" : "inbox-refresh-icon"}
            size={16}
            aria-hidden="true"
            className={refreshing ? "animate-spin" : undefined}
          />
        </IconButton>
      </Tooltip>
      <Menu
        trigger={
          <Button variant="ghost" size="small" data-testid="inbox-filter-trigger" aria-label="Filter by origin">
            Filter
            <ChevronDown size={14} aria-hidden="true" />
          </Button>
        }
        items={NOTIFICATION_ORIGINS.map((origin) => {
          const checked = origins.includes(origin);
          return {
            label: NOTIFICATION_ORIGIN_LABELS[origin],
            onSelect: () => onToggleOrigin(origin),
            itemKey: origin,
            closeOnClick: false,
            content: (
              <span
                role="menuitemcheckbox"
                aria-checked={checked}
                data-testid={`inbox-filter-origin-${origin}`}
                data-origin={origin}
                className="flex w-full items-center gap-(--space-2)"
              >
                <span>{NOTIFICATION_ORIGIN_LABELS[origin]}</span>
                <CheckSlot checked={checked} />
              </span>
            ),
          };
        })}
      />
      <Menu
        trigger={
          <IconButton label="More options" data-testid="inbox-overflow-trigger">
            <MoreVertical size={16} aria-hidden="true" />
          </IconButton>
        }
        items={NOTIFICATION_MODES.map((mode) => ({
          label: NOTIFICATION_MODE_LABELS[mode],
          onSelect: () => onToggleMode(mode),
          itemKey: mode,
          content: (
            <span
              data-testid={`inbox-mode-${mode}`}
              data-mode={mode}
              className="flex w-full items-center gap-(--space-2)"
            >
              <span>{NOTIFICATION_MODE_LABELS[mode]}</span>
              <CheckSlot checked={isModeEnabled(modes, mode)} />
            </span>
          ),
        }))}
      />
    </div>
  );
}
