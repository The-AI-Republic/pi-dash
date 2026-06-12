// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

import { API_BASE_URL } from "@pi-dash/constants";
import type {
  IAssistantMessage,
  IAssistantSendResponse,
  IAssistantThread,
  IUserLLMConfig,
  IUserLLMConfigInput,
} from "@pi-dash/types";
import { APIService } from "../api.service";

export class AssistantService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  private base(slug: string): string {
    return `/api/workspaces/${slug}/assistant`;
  }

  async listThreads(slug: string): Promise<IAssistantThread[]> {
    return this.get(`${this.base(slug)}/threads/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async createThread(slug: string, title?: string): Promise<IAssistantThread> {
    return this.post(`${this.base(slug)}/threads/`, { title: title ?? "" })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async updateThread(
    slug: string,
    threadId: string,
    data: Partial<Pick<IAssistantThread, "title" | "is_archived">>
  ): Promise<IAssistantThread> {
    return this.patch(`${this.base(slug)}/threads/${threadId}/`, data)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async deleteThread(slug: string, threadId: string): Promise<void> {
    return this.delete(`${this.base(slug)}/threads/${threadId}/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async listMessages(slug: string, threadId: string, after = 0, limit = 100): Promise<IAssistantMessage[]> {
    return this.get(`${this.base(slug)}/threads/${threadId}/messages/`, { params: { after, limit } })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async sendMessage(slug: string, threadId: string, content: string): Promise<IAssistantSendResponse> {
    return this.post(`${this.base(slug)}/threads/${threadId}/messages/`, { content })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async cancel(slug: string, threadId: string): Promise<void> {
    return this.post(`${this.base(slug)}/threads/${threadId}/cancel/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  eventsUrl(slug: string, threadId: string, after = 0): string {
    const url = `${this.baseURL}${this.base(slug)}/threads/${threadId}/events/`;
    return after > 0 ? `${url}?after=${after}` : url;
  }

  // --- BYOK LLM config (user-level) ---

  async getLLMConfig(): Promise<IUserLLMConfig> {
    return this.get(`/api/users/me/llm-config/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async putLLMConfig(data: IUserLLMConfigInput): Promise<IUserLLMConfig> {
    return this.put(`/api/users/me/llm-config/`, data)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async deleteLLMConfig(): Promise<void> {
    return this.delete(`/api/users/me/llm-config/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async testLLMConfig(): Promise<{ ok: boolean; error_code?: string; detail?: string }> {
    return this.post(`/api/users/me/llm-config/test/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}
