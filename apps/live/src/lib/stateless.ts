/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { onStatelessPayload } from "@hocuspocus/server";
import { DocumentCollaborativeEvents } from "@pi-dash/editor/lib";
import type { TDocumentEventsServer } from "@pi-dash/editor/lib";

/**
 * Broadcast the client event to all the clients so that they can update their state
 * @param param0
 */
export const onStateless = async ({ payload, document }: onStatelessPayload) => {
  const response = DocumentCollaborativeEvents[payload as TDocumentEventsServer]?.client;
  if (response) {
    document.broadcastStateless(response);
  }
};
