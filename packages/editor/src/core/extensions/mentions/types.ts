/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash types
import type { TSearchEntities } from "@apple-pi-dash/types";

export enum EMentionComponentAttributeNames {
  ID = "id",
  ENTITY_IDENTIFIER = "entity_identifier",
  ENTITY_NAME = "entity_name",
}

export type TMentionComponentAttributes = {
  [EMentionComponentAttributeNames.ID]: string | null;
  [EMentionComponentAttributeNames.ENTITY_IDENTIFIER]: string | null;
  [EMentionComponentAttributeNames.ENTITY_NAME]: TSearchEntities | null;
};
