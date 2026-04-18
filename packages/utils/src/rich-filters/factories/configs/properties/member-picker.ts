/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash imports
import type { IUserLite, TFilterProperty } from "@apple-pi-dash/types";
import { EQUALITY_OPERATOR } from "@apple-pi-dash/types";
// local imports
import type { TCreateFilterConfig } from "../shared";
import { createFilterConfig, createOperatorConfigEntry } from "../shared";
import type { TCreateUserFilterParams, TCustomPropertyFilterParams } from "./shared";
import { getMemberMultiSelectConfig } from "./shared";

/**
 * Member picker property filter specific params
 */
type TCreateMemberPickerPropertyFilterParams = TCustomPropertyFilterParams<IUserLite> & TCreateUserFilterParams;

/**
 * Get the member picker property filter config
 * @param params - The filter params
 * @returns The member picker property filter config
 */
export const getMemberPickerPropertyFilterConfig =
  <P extends TFilterProperty>(key: P): TCreateFilterConfig<P, TCreateMemberPickerPropertyFilterParams> =>
  (params: TCreateMemberPickerPropertyFilterParams) =>
    createFilterConfig({
      id: key,
      ...params,
      label: params.propertyDisplayName,
      icon: params.filterIcon,
      supportedOperatorConfigsMap: new Map([
        createOperatorConfigEntry(EQUALITY_OPERATOR.EXACT, params, (updatedParams) =>
          getMemberMultiSelectConfig(updatedParams, EQUALITY_OPERATOR.EXACT)
        ),
      ]),
    });
