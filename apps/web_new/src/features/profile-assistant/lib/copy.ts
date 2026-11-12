// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Wording for the AI-assistant settings tab (parity/specs/profile.md §10,
// PROF-021..023). The oracle pins the section headings, field
// placeholders, button labels and every notice here, so treat these as
// contract strings; new prose (descriptions, hints) is written for this app.

export const ASSISTANT_TAB_HEADING = "AI Assistant";
export const ASSISTANT_TAB_INTRO =
  "Connect your own model provider to power the assistant. Tool calling varies by model, so a model with native function support gives the most reliable results.";

/** Section headings; the parity driver locates sections by these exact lines. */
export const LLM_SECTION_HEADING = "Language model";
export const VOICE_SECTION_HEADING = "Voice dictation";
export const TOOLS_SECTION_HEADING = "Tool servers (MCP)";
/** Deep-link anchor on the voice section; the assistant mic links here (AGT-051). */
export const VOICE_SECTION_ANCHOR = "voice-dictation";

export const PROVIDER_LABEL = "Provider";
export const BASE_URL_LABEL = "Base URL";
export const MODEL_LABEL = "Model";
export const API_KEY_LABEL = "API key";
export const AUTH_HEADER_LABEL = "Authorization header (optional)";
export const SERVER_NAME_LABEL = "Name";
export const SERVER_URL_LABEL = "Server URL";

export const PROVIDER_CHOICES = [
  { value: "openai_compatible", label: "OpenAI-compatible (OpenRouter, vLLM, …)" },
  { value: "anthropic", label: "Anthropic" },
] as const;

/** The provider a fresh config defaults to. */
export const DEFAULT_PROVIDER_KIND = "openai_compatible";

/** The provider kind that carries no endpoint of its own. */
export const NAMED_PROVIDER_KIND = "anthropic";

export const LLM_BASE_URL_PLACEHOLDER = "https://openrouter.ai/api/v1";
export const LLM_MODEL_PLACEHOLDER = "meta-llama/llama-3.3-70b-instruct";
export const LLM_KEY_PLACEHOLDER_EMPTY = "Your provider API key";
export const STT_BASE_URL_PLACEHOLDER = "https://api.openai.com/v1";
export const STT_MODEL_PLACEHOLDER = "whisper-1";
export const STT_KEY_PLACEHOLDER_EMPTY = "Your transcription API key";
export const SAVED_KEY_PLACEHOLDER = "•••• (saved) — enter to replace";
export const SERVER_NAME_PLACEHOLDER = "My tools";
export const SERVER_URL_PLACEHOLDER = "https://example.com/mcp";
export const AUTH_HEADER_PLACEHOLDER = "Bearer …";

/** Action labels. Save/Test/Remove repeat per section by design. */
export const SAVE_LABEL = "Save";
export const TEST_LABEL = "Test connection";
export const REMOVE_LABEL = "Remove";
export const ADD_SERVER_LABEL = "Add server";

export const LLM_SAVED_TITLE = "Saved";
export const LLM_SAVED_MESSAGE = "AI provider configuration updated.";
export const STT_SAVED_MESSAGE = "Voice dictation configuration updated.";
export const SAVE_FAILED_TITLE = "Save failed";
export const SAVE_FAILED_FALLBACK = "Invalid configuration";
export const CONNECTION_OK_TITLE = "Connection OK";
export const LLM_CONNECTION_OK_MESSAGE = "Your provider responded.";
export const STT_CONNECTION_OK_MESSAGE = "Your transcription endpoint responded.";
export const CONNECTION_FAILED_TITLE = "Connection failed";
export const UNKNOWN_ERROR_FALLBACK = "Unknown error";
export const LLM_REMOVED_TITLE = "Removed";
export const LLM_REMOVED_MESSAGE = "AI provider configuration deleted.";
export const STT_REMOVED_MESSAGE = "Voice dictation configuration deleted.";

export const SERVER_ADDED_TITLE = "Added";
export const SERVER_ADDED_MESSAGE = "Tool server added.";
export const SERVER_ADD_FAILED_TITLE = "Could not add server";
export const SERVER_ADD_FAILED_FALLBACK = "Invalid tool server.";
export const SERVER_UPDATE_FAILED_TITLE = "Update failed";
export const SERVER_UPDATE_FAILED_FALLBACK = "Could not update the tool server.";
export const SERVER_REMOVED_TITLE = "Removed";
export const SERVER_REMOVE_FAILED_TITLE = "Remove failed";
export const SERVER_REMOVE_FAILED_FALLBACK = "Could not remove the tool server.";

export const LAST_VERIFIED_PREFIX = "Last verified:";
export const LAST_VERIFIED_NEVER = "Never";

/** Voice section copy once the instance kill switch is off. */
export const VOICE_UNAVAILABLE_NOTE =
  "Voice dictation is turned off on this Pi Dash instance. Remove the saved key to clear it.";

/** Markers the oracle reads off a tool-server row. */
export const SERVER_DISABLED_MARKER = "(disabled)";
export const SERVER_LOCK_MARKER = "🔒";
export const SERVER_LOCK_LABEL = "Credentials saved";
export const SERVER_PREFIX_PREFIX = "Tools appear as";

/** Descriptions under each section heading. */
export const LLM_SECTION_DESCRIPTION =
  "Point the assistant at any OpenAI-compatible endpoint or use Anthropic. The key is stored server-side and never shown again.";
export const VOICE_SECTION_DESCRIPTION =
  "Hold the microphone in the chat composer to record; releasing transcribes through the endpoint saved here. Audio goes straight to that endpoint and Pi Dash keeps none of it.";
export const TOOLS_SECTION_DESCRIPTION =
  "MCP servers add tools to every assistant turn. An unreachable server is skipped for that run rather than blocking the assistant.";
