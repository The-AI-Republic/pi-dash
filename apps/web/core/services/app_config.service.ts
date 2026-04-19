/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// services
import { API_BASE_URL } from "@pi-dash/constants";
import { APIService } from "@/services/api.service";
// helper
// types
// FIXME:
// import { TAppConfig } from "@pi-dash/types";

export class AppConfigService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  async envConfig(): Promise<any> {
    return this.get("/api/configs/", {
      headers: { "Content-Type": "application/json" },
    })
      .then((response) => response.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }
}
