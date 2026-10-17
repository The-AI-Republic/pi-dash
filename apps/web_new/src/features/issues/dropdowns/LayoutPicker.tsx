// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Layout picker (parity/specs/issues.md §17, ISS-219). A
// search-disabled dropdown over the five layouts in switcher order
// with icon plus label and a checkmark on the selected entry; an
// exclusion list hides options. Entries reuse the header switcher's
// rows so both controls name and mark layouts identically.

import { Button, Menu } from "@pidash/kit";
import * as React from "react";

import { LayoutIcon } from "../components/LayoutIcon.js";
import { layoutMenuItems } from "../components/layoutMenuItems.js";
import { LAYOUT_LABELS, type LayoutKind } from "../layouts/kinds.js";
import { layoutOptionList } from "./layoutOptions.js";

export interface LayoutPickerProps {
  /** Selected layout. The trigger always names it, even when excluded. */
  value: LayoutKind;
  onChange: (kind: LayoutKind) => void;
  /** Kinds hidden from the menu. */
  excluded?: readonly LayoutKind[] | undefined;
  /** Accessible name of the trigger. Defaults to Layout. */
  label?: string | undefined;
  disabled?: boolean | undefined;
  testId?: string | undefined;
}

/** Layout dropdown for the view-create form and layout switchers. */
export function LayoutPicker({
  value,
  onChange,
  excluded,
  label = "Layout",
  disabled = false,
  testId = "layout-picker",
}: LayoutPickerProps): React.ReactElement | null {
  const options = layoutOptionList(excluded);
  if (options.length === 0) return null;
  return (
    <div data-testid={testId}>
      <Menu
        trigger={
          <Button
            variant="secondary"
            size="small"
            disabled={disabled}
            aria-label={`${label}: ${LAYOUT_LABELS[value]}`}
            data-testid={`${testId}-trigger`}
          >
            <LayoutIcon kind={value} />
            {LAYOUT_LABELS[value]}
          </Button>
        }
        items={layoutMenuItems(
          options.map((option) => option.kind),
          value,
          onChange
        )}
      />
    </div>
  );
}
