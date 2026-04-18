/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash imports
import type { IMarking } from "@apple-pi-dash/editor";
import { cn } from "@apple-pi-dash/utils";

export type THeadingComponentProps = {
  marking: IMarking;
  onClick: (event: React.MouseEvent<HTMLButtonElement, MouseEvent>) => void;
};

const COMMON_CLASSNAME =
  "flex-shrink-0 w-full py-1 text-left font-medium text-tertiary hover:text-accent-primary truncate transition-colors";

export function OutlineHeading1({ marking, onClick }: THeadingComponentProps) {
  return (
    <button type="button" onClick={onClick} className={cn(COMMON_CLASSNAME, "pl-1 text-13")}>
      {marking.text}
    </button>
  );
}

export function OutlineHeading2({ marking, onClick }: THeadingComponentProps) {
  return (
    <button type="button" onClick={onClick} className={cn(COMMON_CLASSNAME, "pl-2 text-11")}>
      {marking.text}
    </button>
  );
}

export function OutlineHeading3({ marking, onClick }: THeadingComponentProps) {
  return (
    <button type="button" onClick={onClick} className={cn(COMMON_CLASSNAME, "pl-4 text-11")}>
      {marking.text}
    </button>
  );
}
