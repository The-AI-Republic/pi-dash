// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant copy (parity/specs/agents.md §9). Every string here is pinned
// by an oracle scenario or is new wording written for this app.

/** Landing headline, matched exactly by the AGT-038 scenario. */
export const ASSISTANT_HEADLINE = "How can I help you today?";
/** Landing caption, matched exactly by the AGT-038 scenario. */
export const ASSISTANT_CAPTION = "Ask about your issues, create work, or start a coding run.";
/** Placeholder shared by the landing, thread and dashboard composers. */
export const ASSISTANT_PLACEHOLDER = "Ask Pi Dash to do something…";
/** Composer label for assistive technology. */
export const ASSISTANT_COMPOSER_LABEL = "Message the assistant";
/** Label of the OpenHub model picker in the composer (NEWPIDASH-303). */
export const ASSISTANT_MODEL_PICKER_LABEL = "Assistant model";
/** Failure notice title when the composer model picker's write is refused. */
export const MODEL_PICKER_FAILED_TITLE = "Model change failed";
/** Fallback detail when the model write carries no server message. */
export const MODEL_PICKER_FAILED_FALLBACK = "The model could not be changed";
/** Notice title when a free-plan pick would charge credit the plan cannot pay. */
export const MODEL_PICKER_BLOCKED_TITLE = "Model needs credit";
/** Body of the blocked-pick notice, mirroring the Settings guard. */
export const MODEL_PICKER_BLOCKED_BODY =
  "Free plans can use free models only. Switch to a free model or top up to use this one.";
/** Send action label. */
export const ASSISTANT_SEND_LABEL = "Send message";
/** Stop action label (owned by the thread slice; the gate only proves it hides). */
export const ASSISTANT_STOP_LABEL = "Stop response";
/** Setup card title, matched exactly by the AGT-039 scenario. */
export const SETUP_CARD_TITLE = "Set up your AI assistant";
/**
 * Setup card body. The scenario pins two phrases — bringing your own key
 * and acting with exactly the user's permissions — so both live here.
 */
export const SETUP_CARD_BODY =
  "Bring your own LLM provider key. The assistant then works across this workspace with exactly your permissions — never more.";
/** Setup card action, matched exactly by the AGT-039 scenario. */
export const SETUP_CARD_ACTION = "Configure provider";
/**
 * Where the setup card and the voice cell send the user (main#670): the
 * account settings page's assistant tab, inside the workspace they are
 * already in. The profile area owns the page; the caller supplies the slug.
 */
export function providerSettingsHref(workspaceSlug: string): string {
  return `/${workspaceSlug}/settings/account/ai-assistant/`;
}
/** Deep-link anchor of the voice section inside the assistant settings page (AGT-051). */
export const VOICE_DICTATION_HASH = "voice-dictation";
/** Deep-link anchor of the Models section (NEWPIDASH-394, design section 7.4). */
export const MODELS_SECTION_HASH = "assistant-models";
/** Label of the web-assistant surface picker in Settings. */
export const WEB_ASSISTANT_LABEL = "Web assistant";
/** Label of the desktop-assistant surface picker in Settings. */
export const DESKTOP_ASSISTANT_LABEL = "Desktop assistant";
/** Label of the agent surface picker in Settings. */
export const AGENT_LABEL = "Pi Dash Agent";
/** Placeholder option while a surface has no usable choice. */
export const SURFACE_MODEL_EMPTY_OPTION = "No model available";
/** Mic label when dictation is ready, matched exactly by the AGT-050/051/052/057 scenarios. */
export const MIC_READY_LABEL = "Hold to dictate";
/** Mic label while a recording runs, matched exactly by the AGT-050 scenario. */
export const MIC_RECORDING_LABEL = "Release to transcribe";
/** Mic label with no speech key stored, matched exactly by the AGT-051/057 scenarios. */
export const MIC_SETUP_LABEL = "Set up voice dictation";
/** Recording hint lead; the elapsed readout follows. Matched by the AGT-050 scenario. */
export const MIC_RECORDING_HINT = "Recording — release to transcribe";
/** Hint while an utterance uploads. */
export const MIC_TRANSCRIBING_HINT = "Transcribing…";
/** Hint for a refused microphone, matched exactly by the AGT-050 scenario. */
export const MIC_DENIED_HINT =
  "Microphone access is blocked. Enable it in your browser's site settings, then try again.";
/** Hint when the host has no microphone, matched exactly by the AGT-050 scenario. */
export const MIC_MISSING_HINT = "No microphone found.";
/** Hint when capture fails for any other reason, matched exactly by the AGT-050 scenario. */
export const MIC_FAILED_HINT = "Couldn't access the microphone.";
/** Fallback when an upload failure carries no message of its own. */
export const MIC_FAILED_FALLBACK = "Voice input failed";
/** Keyless hint lead, matched exactly by the AGT-051/057 scenarios; the setup link follows. */
export const MIC_SETUP_HINT = "Voice dictation isn't set up.";
/** Keyless hint action, matched exactly by the AGT-051/057 scenarios. */
export const MIC_SETUP_ACTION = "Configure it";
/** Reminder line for a composer that cannot send without a key (AGT-040). */
export const API_KEY_REMINDER = "Please set your API key in Settings first to start using AI Assistant.";
/** Failure notice title for a conversation that could not start. */
export const START_FAILED_TITLE = "Unable to start chat";
/** Failure notice title when the first message of a new conversation cannot be delivered. */
export const HANDOFF_FAILED_TITLE = "Could not send your message";
/** Keyless send notice title on the dashboard card (AGT-048). */
export const API_KEY_REQUIRED_TITLE = "API key required";
/** Fallback label for a conversation without a title. */
export const UNTITLED_CONVERSATION = "Untitled conversation";
/** Empty-state line inside a conversation with no messages yet. */
export const EMPTY_CONVERSATION_HINT = ASSISTANT_CAPTION;
/** Empty state for the conversation history (members only; not shown on a refusal). */
export const NO_CONVERSATIONS = "No conversations yet.";
/** New-conversation action in the assistant page header. */
export const NEW_CONVERSATION_LABEL = "New chat";
/**
 * History button label in the assistant page header, and the title of the
 * history dialog it opens (parity/specs/agents.md §11, AGT-044/AGT-061).
 */
export const CHAT_HISTORY_TITLE = "Chat History";
/** Fallback label for a tool-activity link the server sent without one. */
export const TOOL_LINK_FALLBACK = "View";
/**
 * Heading of the desktop panel shown when the Pi Dash Agent is
 * unavailable (NEWPIDASH-300). The server's reason sentence follows.
 */
export const DESKTOP_AGENT_UNAVAILABLE_TITLE = "The Pi Dash Agent isn't available";

/**
 * Dashboard card suggestion chips (AGT-048), in display order. Picking one
 * fills the draft; it never sends.
 */
export const ASSISTANT_CARD_SUGGESTIONS: readonly string[] = [
  "What's assigned to me?",
  "Create an issue in…",
  "Summarize open issues in…",
];
