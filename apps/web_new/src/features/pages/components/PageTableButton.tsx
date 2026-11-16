// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Table toolbar entry (parity/specs/pages.md §5.3, PAGE-033). One square
// icon tile matching the core twelve; pressing it drops a new table at
// the caret. It advertises no shortcut, so its tooltip is the name alone.

import { Tooltip } from "@pidash/kit";
import { Table } from "lucide-react";
import * as React from "react";

import { PAGE_MEDIA_CONTROL_LABELS } from "../lib/pageToolbar.js";

export const PAGE_TABLE_TEST_ID = "page-toolbar-table";

export interface PageTableButtonProps {
  onInsert: () => void;
}

export function PageTableButton({ onInsert }: PageTableButtonProps): React.ReactElement {
  return (
    <Tooltip content={<span className="font-medium">{PAGE_MEDIA_CONTROL_LABELS.table}</span>}>
      <button
        type="button"
        aria-label={PAGE_MEDIA_CONTROL_LABELS.table}
        data-testid={PAGE_TABLE_TEST_ID}
        // Keep the editor's selection: the table goes where the caret is.
        onMouseDown={(event) => event.preventDefault()}
        onClick={onInsert}
        className="grid size-7 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
      >
        <Table size={14} strokeWidth={1.75} aria-hidden="true" />
      </button>
    </Tooltip>
  );
}
