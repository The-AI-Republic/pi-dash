/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
import type { IIssueDisplayProperties, TIssue } from "@pi-dash/types";

export type TWorkItemLayoutAdditionalProperties = {
  displayProperties: IIssueDisplayProperties;
  issue: TIssue;
};

export function WorkItemLayoutAdditionalProperties(_props: TWorkItemLayoutAdditionalProperties) {
  return <></>;
}
