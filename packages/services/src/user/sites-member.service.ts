/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash imports
import { API_BASE_URL } from "@apple-pi-dash/constants";
import type { TPublicMember } from "@apple-pi-dash/types";
// api service
import { APIService } from "../api.service";

/**
 * Service class for managing members operations within apple pi dash sites application.
 * Extends APIService to handle HTTP requests to the member-related endpoints.
 * @extends {APIService}
 * @remarks This service is only available for apple pi dash sites
 */
export class SitesMemberService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  /**
   * Retrieves a list of members for a specific anchor.
   * @param {string} anchor - The anchor identifier
   * @returns {Promise<TPublicMember[]>} The list of members
   * @throws {Error} If the API request fails
   */
  async list(anchor: string): Promise<TPublicMember[]> {
    return this.get(`/api/public/anchor/${anchor}/members/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }
}
