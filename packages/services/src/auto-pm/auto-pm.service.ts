// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

import { API_BASE_URL } from "@pi-dash/constants";
import type { IAutoPMSettings } from "@pi-dash/types";
import { APIService } from "../api.service";

// "Auto Project Management" client (internally: loop). Read the instance's
// enabled jobs and toggle each on/off; the master switch pauses all of them.
export class AutoPMService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  async getSettings(): Promise<IAutoPMSettings> {
    return this.get(`/api/users/me/auto-pm/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async setMasterEnabled(enabled: boolean): Promise<IAutoPMSettings> {
    return this.patch(`/api/users/me/auto-pm/`, { enabled })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async setJobEnabled(slug: string, enabled: boolean): Promise<IAutoPMSettings> {
    return this.patch(`/api/users/me/auto-pm/jobs/${slug}/`, { enabled })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}
