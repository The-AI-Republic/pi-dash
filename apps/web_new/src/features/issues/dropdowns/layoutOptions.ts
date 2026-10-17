// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Layout options behind the layout picker (parity/specs/issues.md
// §17, ISS-219). The five layouts in switcher order with an exclusion
// list hiding options; labels reuse the layout kinds so the picker
// and the header switcher name layouts identically.

import { LAYOUT_LABELS, LAYOUT_ORDER, type LayoutKind } from "../layouts/kinds.js";

/** One layout row in the picker. */
export interface LayoutOption {
  kind: LayoutKind;
  label: string;
}

/** Layouts in switcher order minus the excluded kinds. */
export function layoutOptionList(excluded?: readonly LayoutKind[]): LayoutOption[] {
  const hidden = new Set(excluded ?? []);
  return LAYOUT_ORDER.filter((kind) => !hidden.has(kind)).map((kind) => ({ kind, label: LAYOUT_LABELS[kind] }));
}
