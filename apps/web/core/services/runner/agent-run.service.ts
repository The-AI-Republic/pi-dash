/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
// services
import { APIService } from "@/services/api.service";

export type TCreateAgentRunPayload = {
  workspace: string;
  prompt: string;
  work_item?: string;
  pod?: string;
  run_config?: Record<string, unknown>;
  required_capabilities?: string[];
};

export type TAgentRun = {
  id: string;
  workspace: string;
  status: string;
  prompt: string;
  work_item?: string | null;
  created_at: string;
  // additional fields exist on the server response but are not typed here
  // because the UI does not consume them yet.
};

export class AgentRunService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  async createAgentRun(data: TCreateAgentRunPayload): Promise<TAgentRun> {
    return this.post(`/api/runners/runs/`, data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data ?? error;
      });
  }
}
