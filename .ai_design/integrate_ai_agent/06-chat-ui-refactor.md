# 06 — Chat UI Refactor: Shared Chat Kit

Goal: extract the chat UI buried in the runner chat page into a reusable kit under
`core/components/chat/`, so the runner chat and the new assistant chat are **two consumers of one
implementation**. Behavior-preserving refactor; shipped as its own PR before any assistant UI.

Source of truth analyzed: `app/(all)/[workspaceSlug]/runners/chat/[runnerId]/page.tsx` (333 lines)
and `core/components/runners/chat/use-agent-chat-events.ts` (46 lines).

## 1. Line-by-line split of the current page

| Lines                                                                                      | What                                                      | Verdict                                                                       |
| ------------------------------------------------------------------------------------------ | --------------------------------------------------------- | ----------------------------------------------------------------------------- |
| 21-33 `assistantDeltaText`                                                                 | parse delta text out of event payload                     | **extract** (pure)                                                            |
| 35-38 `isRealtimeEvent`                                                                    | drop replayed-history deltas older than stream start      | **extract** (pure)                                                            |
| 40-74 `appendAssistantDelta`                                                               | immutably append delta to streaming message / create stub | **extract** (pure)                                                            |
| 76-84 `disabledReason`                                                                     | runner offline/revoked/busy + session closed/busy         | **split**: runner statuses stay in page; kit takes the final string as a prop |
| 92-97 stream state (`events`, `liveMessages`, `appliedDeltaSeqsRef`, `streamStartedAtRef`) | live-stream state machine                                 | **extract** → `useChatStream`                                                 |
| 99-113 runner detail + session list SWR + session selection                                | runner-specific data                                      | stays in page                                                                 |
| 115-126 reset effects on runner/session change                                             | half generic                                              | reset logic moves into `useChatStream` (keyed by session id)                  |
| 128-163 warm-up effect                                                                     | runner-specific                                           | stays in page                                                                 |
| 165-172 messages SWR → liveMessages seed                                                   | generic pattern                                           | `useChatStream` accepts `baseMessages`                                        |
| 174-198 `handleEvent` (seq dedup, delta apply, lifecycle → SWR mutate)                     | generic + consumer callback                               | **extract**; lifecycle events surfaced via `onLifecycleEvent` callback        |
| 200-243 ensureSession / send / stop / close                                                | session API calls are service-specific                    | stays in consumer (thin handlers)                                             |
| 248-265 header (runner name, status badge, close)                                          | runner-specific                                           | stays in page                                                                 |
| 267-301 message list + bubbles + empty state + debug event rows                            | generic UI (+ debug rows as slot)                         | **extract** → `ChatMessageList`, `ChatMessageBubble`                          |
| 303-328 composer (textarea, enter-to-send, send/stop buttons, reason)                      | generic UI                                                | **extract** → `ChatComposer`                                                  |
| hook `use-agent-chat-events.ts`                                                            | SSE subscribe, hardcoded `RunnerService` URL              | **generalize**: take `eventsUrl` param                                        |

## 2. The kit

```
apps/web/core/components/chat/
├── types.ts             # IChatMessage / IChatEvent (structural subsets — see §3)
├── delta.ts             # assistantDeltaText, isRealtimeEvent, appendAssistantDelta (moved verbatim)
├── use-chat-events.ts   # SSE subscription (generalized use-agent-chat-events)
├── use-chat-stream.ts   # live message state machine
├── message-list.tsx     # ChatMessageList + default ChatMessageBubble
└── composer.tsx         # ChatComposer
```

### use-chat-events.ts

```ts
export function useChatEvents(
  eventsUrl: string | null, // full SSE URL incl. ?after=; null disables
  onEvent: (event: IChatEvent) => void,
  onError?: (error: unknown) => void
): void;
```

Same body as today (EventSource, `chat.event` listener, seq tracking, refs for stable callbacks) —
the only change is `eventsUrl` replaces `service.chatEventsUrl(sessionId, initialAfter)`, removing
the `RunnerService` import. Consumers build the URL from their own service
(`RunnerService.chatEventsUrl(...)` / `AssistantService.eventsUrl(...)`).

### use-chat-stream.ts

```ts
interface UseChatStreamOptions<M extends IChatMessage> {
  streamKey: string | null; // session/thread id — state resets when it changes
  eventsUrl: string | null;
  baseMessages: M[] | undefined; // fetched transcript (SWR); re-seeds liveMessages
  onLifecycleEvent?: (event: IChatEvent) => void; // non-delta events → consumer mutates SWR
  makeStreamingStub?: (event: IChatEvent, delta: string, seq: number) => M; // default: current stub shape
}
interface UseChatStreamResult<M> {
  messages: M[]; // base + live streaming deltas applied
  events: IChatEvent[]; // raw event log (for debug rows / activity slots)
}
```

Encapsulates exactly today's logic: per-seq dedup (`appliedDeltaSeqsRef`), `streamStartedAt`
realtime filter, `assistant_delta` → `appendAssistantDelta`, everything else → `onLifecycleEvent`;
resets all state when `streamKey` changes (today's lines 121-126).

### message-list.tsx / composer.tsx

```tsx
interface ChatMessageListProps<M extends IChatMessage> {
  messages: M[];
  renderMessage?: (message: M) => ReactNode; // default = ChatMessageBubble (user right / other left)
  emptyState?: ReactNode; // default "No messages"
  footer?: ReactNode; // runner page: debug event rows; assistant: typing/tool rows
}
interface ChatComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: () => void;
  onStop?: () => void; // shown instead of send while busy
  busy: boolean; // active turn → stop button
  sending: boolean;
  disabledReason?: string | null; // rendered above input; disables textarea
  placeholder?: string;
}
```

JSX moved from lines 267-328 with the same Tailwind classes (`bg-accent-primary`, `bg-layer-1`,
`border-subtle`…) — zero visual change for runner chat.

## 3. Types

Kit defines **structural** interfaces in `chat/types.ts` so existing types satisfy them without
modification:

```ts
interface IChatMessage {
  id: string;
  role: string;
  content: string;
  status: string;
  seq: number;
  created_at: string;
}
interface IChatEvent {
  seq: number;
  kind: string;
  message?: string | null;
  payload: Record<string, unknown>;
  created_at: string;
  session?: string;
  thread?: string;
} // runner emits `session`,
// assistant emits `thread`; kit reads neither
```

`IAgentChatMessage`/`IAgentChatEvent` (in `@pi-dash/types`) are assignable to these as-is. The
assistant's message type (extra `kind` for tool rows, `usage`, etc.) extends `IChatMessage`. The
kit stays in `apps/web/core/components/chat/` (not `packages/ui`) since it depends on app-level
types/services conventions; promote later only if another app needs it.

## 4. Shared event contract (backend alignment)

Precisely scoped (the earlier "same vocabulary" claim was too loose): the kit is
**kind-agnostic except for one kind**. The load-bearing shared contract is:

1. SSE event name `chat.event` with the envelope `{seq, kind, message, payload, created_at, …}`;
2. the `assistant_delta` kind, whose payload the kit's `delta.ts` parses
   (`payload.params.delta` string — the assistant backend commits to emitting exactly this shape,
   02-backend.md §8.3);
3. every other kind flows untouched to `onLifecycleEvent` — runner chat's kinds (`turn_started`,
   `chat_failed`, `chat_warmed`, …) and the assistant's kinds (`message_created`,
   `message_completed`, `tool_call`, `tool_result`, `turn_completed`, `turn_cancelled`,
   `turn_failed`, per 02 §8.3) are each consumer's own business.

The assistant consumer's `onLifecycleEvent` upserts the fully-serialized message carried in
`payload.message` into its transcript keyed by message id (03-frontend.md §3) — tool rows render
through `renderMessage`; the kit itself never interprets those kinds.

## 5. Runner chat page after refactor (target shape)

Keeps: runner/session SWR + selection, warm-up effect, `disabledReason` runner statuses, header,
ensureSession/send/stop/close handlers, debug event rows (passed as `footer`).
Deletes: ~150 lines of helpers/state/JSX now imported from the kit. Net: page shrinks to a
runner-specific shell (~180 lines), and `core/components/runners/chat/use-agent-chat-events.ts`
is deleted (grep confirms this page is its only importer — no shim needed).

## 6. Verification (behavior-preserving guarantee)

- Unit tests for `delta.ts` pure functions (delta extraction shapes: string delta, `{text}` object,
  `params.text` fallback; append-to-streaming vs create-stub vs ignore-completed paths) and for
  `use-chat-stream` (seq dedup, reset-on-key-change, realtime filter).
- Runner chat E2E/manual pass: send, stream, stop, close, warm-up, offline runner disabled state —
  unchanged visuals (same classes) and behavior.
- PR is refactor-only: no new features, no assistant code, reviewable as pure code motion.
