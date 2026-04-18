/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
import { observer } from "mobx-react";
// apple pi dash imports
import type { TFilterValue, TFilterProperty } from "@apple-pi-dash/types";
// local imports
import type { TFilterValueInputProps } from "@/components/rich-filters/shared";

export const AdditionalFilterValueInput = observer(function AdditionalFilterValueInput<
  P extends TFilterProperty,
  V extends TFilterValue,
>(_props: TFilterValueInputProps<P, V>) {
  return (
    // Fallback
    <div className="flex h-full cursor-not-allowed items-center px-4 text-11 text-placeholder transition-opacity duration-200">
      Filter type not supported
    </div>
  );
});
