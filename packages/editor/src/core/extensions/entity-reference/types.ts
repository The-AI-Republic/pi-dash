/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export enum EEntityReferenceAttributeNames {
  ENTITY_NAME = "entity_name",
  ENTITY_IDENTIFIER = "entity_identifier",
  PROJECT_IDENTIFIER = "project_identifier",
  WORKSPACE_IDENTIFIER = "workspace_identifier",
  PROJECT_ID = "project_id",
  LABEL = "data-label",
}

export type TEntityReferenceAttributes = {
  [EEntityReferenceAttributeNames.ENTITY_NAME]: string | undefined;
  [EEntityReferenceAttributeNames.ENTITY_IDENTIFIER]: string | undefined;
  [EEntityReferenceAttributeNames.PROJECT_IDENTIFIER]: string | undefined;
  [EEntityReferenceAttributeNames.WORKSPACE_IDENTIFIER]: string | undefined;
  [EEntityReferenceAttributeNames.PROJECT_ID]: string | undefined;
  [EEntityReferenceAttributeNames.LABEL]: string | undefined;
};
