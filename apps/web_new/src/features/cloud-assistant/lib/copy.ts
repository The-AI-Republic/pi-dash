// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy for the cloud assistant tab (parity/specs/cloud.md §7). The tab's
// own wording is written here from the spec; only the phrases a parity
// driver matches (titles, actions, markers and the stale-session notice)
// are fixed, so one edit cannot drift those from another surface.

export const TAB_HEADING = "AI Assistant";
export const TAB_INTRO =
  "One credential answers at a time. A managed lane draws on your account; a key you supply points the assistant at a provider you run.";

export const ADD_ACTION = "Add connection";
export const BACK_ACTION = "Back to connections";

export const EMPTY_TITLE = "No AI provider connected yet";
export const EMPTY_BODY = "Connect a provider and the assistant has a model to answer with.";

export const NOT_CONNECTED_MARKER = "Not connected";
export const IN_USE_MARKER = "In use";
export const INCLUDED_MARKER = "Included in your plan";
export const PLAN_SELECTED_MARKER = "ChatGPT plan in use";
export const PLAN_CONNECTED_MARKER = "ChatGPT plan linked";
export const MANAGE_USAGE_LABEL = "Manage plan usage";
export const MANAGE_USAGE_HREF = "https://chatgpt.com/settings/usage";

export const SIGN_IN_ACTION = "Continue with ChatGPT";
export const SIGN_IN_PENDING_ACTION = "Waiting for OpenAI…";
export const SIGN_IN_BODY =
  "Connecting an OpenAI account lets the assistant spend your ChatGPT plan allowance. The provider's page opens in a separate window; the credential is stored encrypted.";
export const SIGN_IN_UNAVAILABLE_BODY = "OpenAI sign-in is switched off for this deployment.";

export const STALE_NOTICE =
  "This session predates OpenHub access. Sign out and back in to enable it — a page refresh alone will not pick up the new permissions.";

export const TEST_ACTION = "Test";
export const REMOVE_ACTION = "Remove";
export const DISCONNECT_ACTION = "Disconnect";
export const CONFIRM_REMOVE_ACTION = "Confirm remove";
export const CANCEL_ACTION = "Cancel";

export const SWITCHED_TITLE = "Switched";
export const SWITCH_FAILED_TITLE = "Switch failed";
export const MODEL_FAILED_TITLE = "Model change failed";
export const REMOVE_CONFIRM_TITLE = "Remove this connection?";
export const REMOVE_CONFIRM_BODY =
  "Its key is deleted right away. Nothing else changes, and you can connect it again later.";
export const REMOVED_TITLE = "Removed";
export const REMOVE_FAILED_TITLE = "Could not remove";
export const DISCONNECT_FAILED_TITLE = "Disconnect failed";
export const DISCONNECTED_TITLE = "OpenAI account disconnected";
export const DISCONNECTED_BODY =
  "The stored credential was removed. Connect the account again whenever you want to use it.";
export const TEST_OK_TITLE = "Connection works";
export const TEST_OK_BODY = "The provider accepted a request with this connection.";
export const TEST_FAILED_TITLE = "Connection failed";
export const TEST_FAILED_FALLBACK = "Unexpected response";

export const PROVIDER_LABEL = "Provider";
export const COMPATIBLE_PROVIDER_LABEL = "OpenAI-compatible";
export const ANTHROPIC_PROVIDER_LABEL = "Anthropic";
export const BASE_URL_LABEL = "Base URL";
export const BASE_URL_PLACEHOLDER = "https://provider.example/v1";
export const MODEL_LABEL = "Model";
export const MODEL_PLACEHOLDER = "The model your provider serves";
export const API_KEY_LABEL = "API key";
export const API_KEY_PLACEHOLDER = "Paste the provider's key";
export const BYOK_MISSING_BASE = "Enter the address your provider answers on.";
export const BYOK_MISSING_MODEL = "Name the model the assistant should call.";
export const BYOK_MISSING_KEY = "Paste the key for this provider.";
export const ADDED_TITLE = "Connection added";
export const ADD_FAILED_TITLE = "Could not add connection";
export const ADD_FAILED_FALLBACK = "The server rejected the configuration";
export const ADD_SAVING_ACTION = "Adding…";

export const POPUP_BLOCKED_TITLE = "Popup blocked";
export const POPUP_BLOCKED_BODY = "The sign-in window could not open. Allow pop-ups for this site and try again.";
export const SIGN_IN_START_FAILED_TITLE = "Could not start ChatGPT sign-in";
export const SIGN_IN_START_FAILED_FALLBACK = "No detail returned";
export const SIGN_IN_CLOSED_TITLE = "Sign-in window closed";
export const SIGN_IN_CLOSED_BODY = "The sign-in window closed before OpenAI answered. Start it again.";
export const SIGN_IN_OK_TITLE = "OpenAI account ready";
export const SIGN_IN_OK_BODY =
  "The OpenAI account is connected, and the assistant will draw on its ChatGPT plan allowance.";
export const SIGN_IN_FAILED_TITLE = "OpenAI sign-in failed";
export const SIGN_IN_FAILED_BODY = "OpenAI reported that the sign-in could not finish. Start it again.";
export const SIGN_IN_TIMEOUT_TITLE = "Sign-in timed out";
export const SIGN_IN_TIMEOUT_BODY = "No answer arrived from OpenAI before the five-minute window ended.";

export const TOOL_SERVERS_HEADING = "Tool servers (MCP)";
export const TOOL_SERVERS_INTRO =
  "Servers listed here are offered to the assistant as tools. Give each one a name and the address it answers on.";
export const TOOL_SERVER_NAME_PLACEHOLDER = "Team tools";
export const TOOL_SERVER_URL_PLACEHOLDER = "https://tools.example/mcp";
export const TOOL_SERVER_AUTH_PLACEHOLDER = "Paste the server's access token";
export const TOOL_SERVER_NAME_LABEL = "Name";
export const TOOL_SERVER_URL_LABEL = "Server address";
export const TOOL_SERVER_AUTH_LABEL = "Access credential (optional)";
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

export const APPS_POINTER = "Looking for apps? Open Apps in the sidebar to browse, install and connect them.";
