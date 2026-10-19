// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy for the cloud assistant tab (parity/specs/cloud.md §7). Slices
// author their own wording; the phrases the oracle driver matches are
// kept here so a single edit cannot drift one surface from another.

export const TAB_HEADING = "AI Assistant";
export const TAB_INTRO =
  "Choose which language model answers the assistant. Managed lanes use your account; a self-supplied key points at a provider you control.";

export const ADD_ACTION = "Add connection";
export const BACK_ACTION = "Back to connections";

export const EMPTY_TITLE = "No AI provider connected yet";
export const EMPTY_BODY = "Add a connection so the assistant has a model to answer with.";

export const NOT_CONNECTED_MARKER = "Not connected";
export const IN_USE_MARKER = "In use";
export const INCLUDED_MARKER = "Included with your account";
export const PLAN_SELECTED_MARKER = "Using ChatGPT plan";
export const PLAN_CONNECTED_MARKER = "ChatGPT plan connected";
export const MANAGE_USAGE_LABEL = "Manage usage";
export const MANAGE_USAGE_HREF = "https://chatgpt.com/settings/usage";

export const SIGN_IN_ACTION = "Continue with ChatGPT";
export const SIGN_IN_PENDING_ACTION = "Waiting for OpenAI…";
export const SIGN_IN_BODY =
  "Sign in with ChatGPT to let Pi Dash use your eligible ChatGPT plan for AI requests. A new tab opens for sign-in, and the connection is stored encrypted.";
export const SIGN_IN_UNAVAILABLE_BODY = "ChatGPT sign-in is not available on this deployment.";

export const STALE_NOTICE =
  "This session predates OpenHub access. Sign out and back in to use this lane — refreshing the page does not pick up the new permissions.";

export const TEST_ACTION = "Test";
export const REMOVE_ACTION = "Remove";
export const DISCONNECT_ACTION = "Disconnect";
export const CONFIRM_REMOVE_ACTION = "Confirm remove";
export const CANCEL_ACTION = "Cancel";

export const SWITCHED_TITLE = "Switched";
export const SWITCH_FAILED_TITLE = "Could not switch";
export const MODEL_FAILED_TITLE = "Could not change model";
export const REMOVE_CONFIRM_TITLE = "Remove this connection?";
export const REMOVE_CONFIRM_BODY = "The assistant stops using it immediately. You can add it again later.";
export const REMOVED_TITLE = "Removed";
export const REMOVE_FAILED_TITLE = "Could not remove";
export const DISCONNECT_FAILED_TITLE = "Could not disconnect";
export const DISCONNECTED_TITLE = "ChatGPT disconnected";
export const DISCONNECTED_BODY =
  "Pi Dash no longer has access to your ChatGPT account. You can sign in again at any time.";
export const TEST_OK_TITLE = "Connection OK";
export const TEST_OK_BODY = "Your provider responded.";
export const TEST_FAILED_TITLE = "Connection failed";
export const TEST_FAILED_FALLBACK = "Unknown error";

export const PROVIDER_LABEL = "Provider";
export const COMPATIBLE_PROVIDER_LABEL = "OpenAI-compatible";
export const ANTHROPIC_PROVIDER_LABEL = "Anthropic";
export const BASE_URL_LABEL = "Base URL";
export const BASE_URL_PLACEHOLDER = "https://openrouter.ai/api/v1";
export const MODEL_LABEL = "Model";
export const MODEL_PLACEHOLDER = "meta-llama/llama-3.3-70b-instruct";
export const API_KEY_LABEL = "API key";
export const API_KEY_PLACEHOLDER = "Your provider API key";
export const BYOK_MISSING_BASE = "Enter the address your provider answers on.";
export const BYOK_MISSING_MODEL = "Name the model the assistant should call.";
export const BYOK_MISSING_KEY = "Paste the key for this provider.";
export const ADDED_TITLE = "Connection added";
export const ADD_FAILED_TITLE = "Could not add connection";
export const ADD_FAILED_FALLBACK = "Invalid configuration";
export const ADD_SAVING_ACTION = "Adding…";

export const POPUP_BLOCKED_TITLE = "Popup blocked";
export const POPUP_BLOCKED_BODY = "Allow popups for this site, then try again.";
export const SIGN_IN_START_FAILED_TITLE = "Could not start ChatGPT sign-in";
export const SIGN_IN_START_FAILED_FALLBACK = "Unknown error";
export const SIGN_IN_CLOSED_BODY = "The ChatGPT sign-in tab was closed. Try again.";
export const SIGN_IN_OK_TITLE = "ChatGPT plan connected";
export const SIGN_IN_OK_BODY =
  "Pi Dash now uses your ChatGPT plan for AI requests. Usage counts against your plan's limits.";
export const SIGN_IN_FAILED_TITLE = "OpenAI sign-in failed";
export const SIGN_IN_FAILED_BODY = "The sign-in could not be completed. Try again.";
export const SIGN_IN_TIMEOUT_TITLE = "Sign-in timed out";
export const SIGN_IN_TIMEOUT_BODY = "The OpenAI sign-in was not completed. Try again.";

export const TOOL_SERVERS_HEADING = "Tool servers (MCP)";
export const TOOL_SERVERS_INTRO =
  "Register the tool servers the assistant may call. Each server claims a tool prefix so agents can tell their tools apart.";
export const TOOL_SERVER_NAME_PLACEHOLDER = "My tools";
export const TOOL_SERVER_URL_PLACEHOLDER = "https://example.com/mcp";
export const TOOL_SERVER_AUTH_PLACEHOLDER = "Bearer …";
export const TOOL_SERVER_NAME_LABEL = "Name";
export const TOOL_SERVER_URL_LABEL = "Server address";
export const TOOL_SERVER_AUTH_LABEL = "Authorization header (optional)";
export const TOOL_SERVER_ADD_ACTION = "Add server";
export const TOOL_SERVER_EMPTY = "No tool servers registered yet.";
export const TOOL_SERVER_ADDED_TITLE = "Added";
export const TOOL_SERVER_ADDED_BODY = "Tool server added.";
export const TOOL_SERVER_ADD_FAILED_TITLE = "Could not add server";
export const TOOL_SERVER_ADD_FAILED_BODY = "Invalid tool server.";
export const TOOL_SERVER_UPDATE_FAILED_TITLE = "Update failed";
export const TOOL_SERVER_UPDATE_FAILED_BODY = "Could not update the tool server.";
export const TOOL_SERVER_REMOVE_FAILED_TITLE = "Remove failed";
export const TOOL_SERVER_REMOVE_FAILED_BODY = "Could not remove the tool server.";
export const TOOL_SERVER_REMOVED_TITLE = "Removed";
export const TOOL_SERVER_DISABLED_MARKER = "(disabled)";
export const TOOL_SERVER_LOCKED_MARKER = "Locked";
export const TOOL_SERVER_ENABLE_ACTION = "Enable";
export const TOOL_SERVER_DISABLE_ACTION = "Disable";

export const APPS_POINTER =
  "Looking for apps? The catalog lives in the sidebar under Apps — search, install and connect from there.";
