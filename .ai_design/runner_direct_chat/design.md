# Runner Direct Chat: Remote UI for Headless Agents

> Directory: `.ai_design/runner_direct_chat/`
>
> This design adds first-class direct chat with a Pi Dash runner while keeping
> the existing task/run workflow intact. It borrows BrowserX's session/channel
> shape and Codex app-server's thread/turn/item lifecycle, but implements the
> feature in Pi Dash's current Django + Rust runner + React route structure.

## 1. Problem Statement

Pi Dash currently treats a runner as an asynchronous task executor. The cloud
creates an `AgentRun`, assigns it to an idle runner, the runner starts a fresh
local agent session, and the cloud records lifecycle events until the run is
terminal.

That works for issue work, scheduler ticks, and "Comment & Run", but it does
not support the second operating mode we need:

- an operator opens "AI Agents",
- selects a live runner,
- chats directly with that runner as a remote headless agent,
- gets streaming assistant output and tool/approval events,
- can close the chat without turning it into an issue task.

The feature must preserve the current task semantics:

- task runs remain independent sessions by default,
- a busy runner should not accept chat in the MVP,
- cloud remains the UI and persistence layer,
- runner remains the local owner of the agent CLI process and workspace.

## 2. Current System

### 2.1 Cloud Task Model

The Django runner app already owns these task entities:

- `Runner`: enrolled local daemon, scoped to a pod and workspace.
- `RunnerSession`: per-runner long-poll session.
- `AgentRun`: durable task assignment.
- `AgentRunEvent`: append-only task event transcript.
- `ApprovalRequest`: approval rows tied to `AgentRun`.
- `RunnerLiveState`: volatile per-active-run observability snapshot.

Important code:

- `apps/api/pi_dash/runner/models.py`
- `apps/api/pi_dash/runner/views/runs.py`
- `apps/api/pi_dash/runner/views/run_endpoints.py`
- `apps/api/pi_dash/runner/views/sessions.py`
- `apps/api/pi_dash/runner/services/matcher.py`
- `apps/api/pi_dash/runner/services/outbox.py`

The web-facing API is mounted at `/api/runners/`. The runner-facing API is
mounted at `/api/v1/runner/`.

### 2.2 Runner Transport

Current wire protocol is v4 HTTPS long-poll:

- runner opens a `RunnerSession`,
- runner POSTs `/poll` with heartbeat/status/observability,
- cloud responds with queued control messages,
- runner POSTs lifecycle/events through dedicated REST endpoints.

Important code:

- `runner/src/cloud/protocol.rs`
- `runner/src/cloud/http.rs`
- `runner/src/daemon/supervisor.rs`
- `runner/src/daemon/runner_instance.rs`
- `runner/src/daemon/runner_out.rs`

Control messages are currently run-oriented:

- cloud -> runner: `assign`, `cancel`, `decide`, `config_push`, etc.
- runner -> cloud: `accept`, `run_started`, `run_event`, `approval_request`,
  `run_completed`, `run_failed`, etc.

### 2.3 Runner Agent Bridge

The runner has an agent-agnostic bridge surface:

- `AgentBridge::run(payload, cwd)`
- `AgentBridge::next_events(cursor)`
- `AgentBridge::send_approval(...)`
- `AgentBridge::interrupt()`

Codex bridge:

- starts `codex app-server`,
- sends `initialize`,
- sends `thread/start`,
- sends `turn/start`,
- translates app-server notifications into `BridgeEvent`.

Claude bridge:

- starts `claude --print`,
- sends one JSON input,
- closes stdin,
- reads `stream-json` until result.

Important code:

- `runner/src/agent/mod.rs`
- `runner/src/codex/bridge.rs`
- `runner/src/codex/app_server.rs`
- `runner/src/codex/schema.rs`
- `runner/src/claude_code/bridge.rs`

### 2.4 Web UI

The current AI Agents area is the `runners` route:

- `/[workspaceSlug]/runners`
- `/[workspaceSlug]/runners/runs`
- `/[workspaceSlug]/runners/approvals`

Its layout is a top header with tabs. The requested UI changes this route into
an AI Agents shell with a middle panel:

- the global left sidebar still has one "AI Agents" item under Workspace,
- only this route gets a second/middle panel,
- first middle-panel item is `Overview`, selected by default,
- runner rows follow,
- selecting a runner opens that runner's chat page.

Important code:

- `apps/web/app/(all)/[workspaceSlug]/runners/layout.tsx`
- `apps/web/app/(all)/[workspaceSlug]/runners/page.tsx`
- `apps/web/app/(all)/[workspaceSlug]/runners/runs/page.tsx`
- `apps/web/app/(all)/[workspaceSlug]/runners/approvals/page.tsx`
- `packages/services/src/runner/runner.service.ts`
- `packages/types/src/runner.ts`

## 3. Reference Designs

### 3.1 BrowserX

BrowserX's useful pattern is not the implementation language; it is the shape:

- platform-specific channel adapters route user submissions into an agent core,
- submissions carry a `SubmissionContext` with channel/session identity,
- sessions are persistent conversation objects,
- server mode exposes methods such as `chat.send`, `chat.abort`,
  `chat.history`,
- agent events are converted into chat/agent/approval event streams,
- deltas are throttled before they hit the UI.

Relevant files:

- `/home/rich/dev/airepublic/open_source/s1/browserx/docs/ARCHITECTURE.md`
- `/home/rich/dev/airepublic/open_source/s1/browserx/src/core/Session.ts`
- `/home/rich/dev/airepublic/open_source/s1/browserx/src/core/RepublicAgent.ts`
- `/home/rich/dev/airepublic/open_source/s1/browserx/src/core/engine/RepublicAgentEngine.ts`
- `/home/rich/dev/airepublic/open_source/s1/browserx/src/core/channels/ChannelManager.ts`
- `/home/rich/dev/airepublic/open_source/s1/browserx/src/server/handlers/chat.ts`
- `/home/rich/dev/airepublic/open_source/s1/browserx/src/server/streaming/chat-stream.ts`
- `/home/rich/dev/airepublic/open_source/s1/browserx/src/server/channels/ServerChannel.ts`

What we borrow:

- explicit chat session separate from task runs,
- operation/event vocabulary,
- event fan-out to clients,
- delta throttling,
- channel/session identity in every command.

What we do not transplant:

- BrowserX's TypeScript agent core,
- its WebSocket server protocol,
- its Node ownership of model/tool execution.

Pi Dash's runner is Rust and wraps external CLIs; BrowserX's core owns the
model/tool loop directly.

### 3.2 Codex

Codex app-server is the stronger local-agent reference because it already
implements the exact remote UI primitives:

- `Thread`: conversation session.
- `Turn`: one user message and agent response cycle.
- `Item`: user message, agent message, reasoning, shell command, file edit,
  approval, etc.

Relevant files:

- `/home/rich/dev/study/codex/codex-rs/app-server/README.md`
- `/home/rich/dev/study/codex/codex-rs/app-server/src/request_processors/thread_processor.rs`
- `/home/rich/dev/study/codex/codex-rs/app-server/src/request_processors/turn_processor.rs`
- `/home/rich/dev/study/codex/codex-rs/app-server/src/bespoke_event_handling.rs`
- `/home/rich/dev/study/codex/codex-rs/app-server-protocol/src/protocol/v2/thread.rs`
- `/home/rich/dev/study/codex/codex-rs/app-server-protocol/src/protocol/v2/turn.rs`
- `/home/rich/dev/study/codex/codex-rs/core/src/session/mod.rs`

Codex API mapping:

| Pi Dash chat action                              | Codex app-server action                                     |
| ------------------------------------------------ | ----------------------------------------------------------- |
| start chat with runner                           | `thread/start`                                              |
| send user message to idle chat                   | `turn/start`                                                |
| stream assistant output                          | `item/agentMessage/delta`, `item/started`, `item/completed` |
| stop current response                            | `turn/interrupt`                                            |
| inject message into active response, later phase | `turn/steer`                                                |
| reopen existing chat on same runner              | `thread/resume`                                             |

Important Codex behavior:

- `turn/start` submits `Op::UserInput` into a bounded core queue.
- `turn/steer` only succeeds when there is an active regular turn and the
  caller supplies the expected turn id.
- `turn/interrupt` validates active turn id and submits `Op::Interrupt`.
- app-server already emits `turn/started`, `item/*`, `turn/completed`,
  `turn/diff/updated`, `turn/plan/updated`, approval requests, and errors.

## 4. Goals

1. Add a direct chat mode for runners.
2. Keep task mode unchanged and independent.
3. Make runner busy state explicit: chat is disabled when a task or chat turn is
   active.
4. Persist chat sessions/messages/events in cloud DB.
5. Stream chat output to the web UI.
6. Reuse existing runner long-poll/outbox for MVP runner control messages.
7. Design the protocol so runner SSE can replace/augment long-poll later
   without changing the chat domain model.
8. Make Codex chat first-class using app-server thread/turn semantics.
9. Support Claude Code chat through resumable one-message subprocesses.
10. Keep the web UI consistent with the current route, service, type, and i18n
    structure.

## 5. Non-Goals

- Do not queue chat messages behind active task runs in the MVP.
- Do not merge task conversation history into chat sessions.
- Do not make chat sessions authoritative task records.
- Do not replace `AgentRun`.
- Do not replace the existing approval system for task runs; chat gets parallel
  approval rows so task run state remains clean.
- Do not copy BrowserX implementation code into Pi Dash.

## 6. Product Behavior

### 6.1 AI Agents UI

When the user clicks `AI Agents` in the left workspace sidebar:

- render an AI Agents route shell,
- show the middle panel,
- select `Overview` by default,
- render the current AI Agents overview in the main content area.

Middle panel:

1. `Overview`
2. runner rows

Runner row data:

- name,
- pod/project hint,
- status badge,
- last heartbeat,
- busy/idle indication from `status` and `live_state.observed_run_id`.

Routes:

```
/:workspaceSlug/runners
/:workspaceSlug/runners/runs
/:workspaceSlug/runners/approvals
/:workspaceSlug/runners/chat/:runnerId
```

The shell owns the middle panel for all `/runners/*` routes. `Overview` should
be active for `/runners`, `/runners/runs`, and `/runners/approvals`; a runner
row should be active for `/runners/chat/:runnerId`.

Overview content should preserve existing functionality:

- connected runners,
- pods,
- runs,
- approvals.

Implementation can keep `runs` and `approvals` as nested pages under the
overview content or fold them into tabs inside the overview page. The middle
panel must remain stable across all of these views.

### 6.2 Chat Page

Selecting a runner opens:

```
/:workspaceSlug/runners/chat/:runnerId
```

The chat page:

- loads the runner detail,
- loads the latest open chat session for current user + runner that has at
  least one message, or stays in an empty "new chat" state until the first
  send,
- displays prior messages,
- opens an event stream for live events,
- sends user messages through the web API,
- disables composer if runner is offline/revoked/busy.

Composer disabled cases:

| Condition                          | UI state              | Server response           |
| ---------------------------------- | --------------------- | ------------------------- |
| runner offline                     | disabled              | `409 runner_unavailable`  |
| runner revoked                     | disabled              | `409 runner_unavailable`  |
| runner busy with task              | disabled              | `409 runner_busy`         |
| runner busy with another chat turn | disabled or stop-only | `409 chat_turn_active`    |
| session closed                     | disabled              | `409 chat_session_closed` |

MVP behavior for user sends while active response is still streaming:

- server rejects with `409 chat_turn_active`,
- UI keeps composer disabled until `chat_turn_completed`,
- no queue.

Later behavior:

- use Codex `turn/steer` when we intentionally support mid-turn steering.

## 7. Domain Model

Add new Django models.

### 7.1 `AgentChatSession`

```python
class AgentChatSession(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    workspace = models.ForeignKey("db.Workspace", on_delete=models.CASCADE, related_name="agent_chat_sessions")
    runner = models.ForeignKey("runner.Runner", on_delete=models.CASCADE, related_name="chat_sessions")
    created_by = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.PROTECT, related_name="agent_chat_sessions")
    pod = models.ForeignKey("runner.Pod", on_delete=models.PROTECT, related_name="agent_chat_sessions")

    status = models.CharField(max_length=24, choices=AgentChatSessionStatus.choices, default="open", db_index=True)
    agent_kind = models.CharField(max_length=24, blank=True, default="")

    local_thread_id = models.CharField(max_length=128, blank=True, default="")
    local_session_id = models.CharField(max_length=128, blank=True, default="")
    cwd = models.TextField(blank=True, default="")
    model = models.CharField(max_length=128, blank=True, default="")

    active_turn_id = models.CharField(max_length=128, blank=True, default="")
    active_message_id = models.UUIDField(null=True, blank=True)
    last_message_at = models.DateTimeField(null=True, blank=True)
    closed_at = models.DateTimeField(null=True, blank=True)
    error = models.TextField(blank=True, default="")

    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)
```

Statuses:

- `open`
- `closed`
- `failed`

The session status is only lifecycle state. It does not encode turn runtime
state. Runtime state belongs to:

- `active_turn_id`: non-empty while a local agent turn is in flight,
- `active_message_id`: user message currently being answered,
- `AgentChatMessage.status`: queued/sent/streaming/completed/failed/cancelled,
- pending `AgentChatApprovalRequest` rows.

This avoids a second source of truth such as `status="active"` with an empty
`active_turn_id`.

Notes:

- `local_thread_id` is Codex `thread.id`.
- `local_session_id` is Claude Code `session_id` when available.
- `active_turn_id` is Codex `turn.id`.
- `active_message_id` points to the user message being answered.
- The row is not an `AgentRun` and does not participate in the task matcher.

Indexes:

- `(workspace, runner, status)`
- `(created_by, runner, status)`
- `(runner, status)`
- `(last_message_at)`

### 7.2 `AgentChatMessage`

```python
class AgentChatMessage(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    session = models.ForeignKey("runner.AgentChatSession", on_delete=models.CASCADE, related_name="messages")
    role = models.CharField(max_length=16, choices=AgentChatMessageRole.choices, db_index=True)
    content = models.TextField(blank=True, default="")
    content_parts = models.JSONField(default=list, blank=True)
    status = models.CharField(max_length=24, choices=AgentChatMessageStatus.choices, default="completed", db_index=True)
    local_item_id = models.CharField(max_length=128, blank=True, default="")
    local_turn_id = models.CharField(max_length=128, blank=True, default="")
    seq = models.PositiveIntegerField()
    created_at = models.DateTimeField(auto_now_add=True)
    completed_at = models.DateTimeField(null=True, blank=True)
```

Roles:

- `user`
- `assistant`
- `tool`
- `system`

Statuses:

- `queued`
- `sent`
- `streaming`
- `completed`
- `failed`
- `cancelled`

Constraints:

- unique `(session, seq)`
- index `(session, created_at)`
- index `(session, local_turn_id)`
- index `(session, local_item_id)`

### 7.3 `AgentChatEvent`

```python
class AgentChatEvent(models.Model):
    id = models.BigAutoField(primary_key=True)
    session = models.ForeignKey("runner.AgentChatSession", on_delete=models.CASCADE, related_name="events")
    message = models.ForeignKey("runner.AgentChatMessage", null=True, blank=True, on_delete=models.SET_NULL, related_name="events")
    seq = models.PositiveIntegerField()
    source_key = models.CharField(max_length=160, blank=True, default="")
    kind = models.CharField(max_length=64)
    payload = models.JSONField(default=dict, blank=True)
    created_at = models.DateTimeField(auto_now_add=True)
```

Constraints:

- unique `(session, seq)`
- conditional unique `(session, source_key)` where `source_key != ""`
- index `(session, created_at)`

Event kinds:

- `chat_started`
- `turn_started`
- `assistant_delta`
- `assistant_message`
- `item_started`
- `item_completed`
- `tool_output_delta`
- `approval_requested`
- `approval_decided`
- `turn_completed`
- `chat_failed`
- `chat_closed`
- `raw`

### 7.4 `AgentChatApprovalRequest`

Chat approvals should not reuse `ApprovalRequest` because that table requires
`agent_run`. Add a parallel model:

```python
class AgentChatApprovalRequest(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    session = models.ForeignKey("runner.AgentChatSession", on_delete=models.CASCADE, related_name="approvals")
    local_approval_id = models.CharField(max_length=160)
    kind = models.CharField(max_length=24, choices=ApprovalKind.choices)
    payload = models.JSONField(default=dict, blank=True)
    reason = models.TextField(blank=True, default="")
    status = models.CharField(max_length=16, choices=ApprovalStatus.choices, default=ApprovalStatus.PENDING, db_index=True)
    decision_source = models.CharField(max_length=16, blank=True, default="")
    decided_by = models.ForeignKey(settings.AUTH_USER_MODEL, null=True, blank=True, on_delete=models.SET_NULL, related_name="runner_chat_approvals_decided")
    requested_at = models.DateTimeField(auto_now_add=True)
    expires_at = models.DateTimeField(null=True, blank=True)
    decided_at = models.DateTimeField(null=True, blank=True)
```

The runner receives approval decisions through the same cloud control channel,
but the control message must identify whether it targets a run or chat session.

Approval IDs have two layers:

- `id` is the cloud UUID used by web APIs and browser actions.
- `local_approval_id` is the opaque Codex/Claude/bridge approval id sent by
  the local agent. It is unique per chat session and is the value passed back to
  the runner bridge when an operator decides the approval.

Constraint:

- unique `(session, local_approval_id)`

## 8. Runner Availability Rules

The MVP is single-active-operation per runner:

- an assigned/running task occupies the runner,
- an active chat turn occupies the runner,
- no queue between task and chat,
- no chat while task is active,
- no task assignment while chat turn is active.

### 8.1 Cloud Busy Check

Add a helper:

```python
def runner_has_active_task(runner: Runner) -> bool:
    return AgentRun.objects.filter(
        runner=runner,
        status__in=matcher.BUSY_STATUSES,
    ).exists()

def runner_has_active_chat(runner: Runner) -> bool:
    return (
        AgentChatSession.objects
        .filter(runner=runner, status="open")
        .filter(Q(active_message_id__isnull=False) | ~Q(active_turn_id=""))
        .exists()
    )
```

Use it in:

- chat session/message create endpoint,
- matcher runner selection.

`matcher.select_runner_in_pod`, `drain_pod`, and `drain_for_runner` must exclude
runners with active chat turns. This prevents a task `Assign` landing while the
runner is answering chat.

The matcher hot path should avoid per-runner duplicate EXISTS queries. Prefer
one query shape that excludes both busy task runs and active chat sessions, for
example with `Exists` subqueries or a scoped helper that is evaluated while the
candidate `Runner` rows are already locked.

The chat send path must lock the same runner row with `select_for_update()`
before setting `active_message_id`. The matcher already selects runners under
row-level lock; chat send must use the same lock discipline so task assignment
and chat turn start cannot both claim an idle runner. `active_message_id` is the
cloud-side claim while the message is queued/starting; `active_turn_id` is
filled once the runner reports the local turn id.

### 8.2 Runner Busy Check

RunnerLoop already has:

```rust
current_run: Option<CurrentRun>
```

Add:

```rust
current_chat: Option<CurrentChat>
```

Inbound behavior:

- `Assign` while `current_chat.is_some()` logs and rejects/ignores.
- `ChatUserMessage` while `current_run.is_some()` returns `ChatFailed { code:
"runner_busy" }`.
- `ChatUserMessage` while the session has an active turn returns `ChatFailed {
code: "chat_turn_active" }`.

The cloud should prevent these cases first; runner enforcement is the safety
net.

## 9. Cloud API

Add web-facing endpoints under `/api/runners/chat/`.

### 9.1 Routes

```python
path("chat/sessions/", AgentChatSessionListEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/", AgentChatSessionDetailEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/messages/", AgentChatMessageListEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/events/", AgentChatEventStreamEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/cancel/", AgentChatCancelEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/close/", AgentChatCloseEndpoint.as_view())
path("chat/approvals/", AgentChatApprovalListEndpoint.as_view())
path("chat/approvals/<uuid:approval_id>/decide/", AgentChatApprovalDecideEndpoint.as_view())
```

The event stream route should be implemented as an ASGI endpoint, not a normal
sync DRF `APIView`. Use a Channels `AsyncHttpConsumer` or an async Django view
with `redis.asyncio` pub/sub. Keep authentication and permission checks aligned
with the DRF session-auth endpoints, but keep the stream itself fully async.

### 9.2 Session Create/Get

`POST /api/runners/chat/sessions/`

Request:

```json
{
  "workspace": "workspace_uuid",
  "runner": "runner_uuid",
  "model": "optional",
  "cwd": "optional"
}
```

Behavior:

1. authenticate user session,
2. verify workspace membership,
3. verify runner belongs to workspace,
4. reject offline/revoked runner,
5. lock the `Runner` row with `select_for_update()`,
6. return the existing empty open session for `(created_by, runner)` if one
   exists,
7. otherwise create `AgentChatSession(status="open")`,
8. return session.

Session creation is cloud-only. It must not start a local agent process or
mark the runner busy. The runner is contacted by the first message send.

`GET /api/runners/chat/sessions/?workspace=<id>&runner=<id>` lists sessions
the user can see. Default UI should use the latest open session with at least
one message for the selected runner, or stay in the empty new-chat state and
create a session on first send.

To avoid orphan drift, there should be at most one empty open session per
`(created_by, runner)`. Empty open sessions older than 24 hours can be closed
by the chat sweeper without user-visible data loss.

Because "empty" depends on the absence of related messages, do not try to model
this as a simple database unique constraint. Serialize session create by taking
the runner row lock and checking for an empty open session inside the same
transaction. First-send can also skip pre-creation entirely by creating the
session and first message in one transaction.

`cwd` is optional but must be server-validated. MVP should either ignore
user-provided `cwd` or resolve it against the runner's configured workspace /
pod / project root and reject paths outside that root with `400 invalid_cwd`.
The runner should perform the same containment check before spawning the local
agent process.

### 9.3 Send Message

`POST /api/runners/chat/sessions/<id>/messages/`

Request:

```json
{
  "content": "What changed in this repo?",
  "content_parts": []
}
```

Behavior:

1. verify session access,
2. lock session and runner,
3. reject closed/failed session,
4. reject runner unavailable or active task,
5. reject `active_message_id is not null` or `active_turn_id != ""`,
6. create user `AgentChatMessage(status="queued")`,
7. set `active_message_id=<message>`,
8. register a `transaction.on_commit` callback that enqueues
   `chat_user_message` with enough context for the runner to start or resume
   the local agent conversation,
9. return message.

This endpoint is the chat equivalent of `AgentRunListEndpoint.post`, but it
does not call `matcher.drain_pod`.

Do not perform Redis/outbox I/O inside the DB transaction. The on-commit
enqueue helper must be idempotent for the message id. If enqueue fails because
the runner went offline or Redis is unavailable, it must mark the queued user
message `failed`, append a `chat_failed` event, clear
`AgentChatSession.active_message_id` / `active_turn_id`, and publish the failure
to browser SSE subscribers.

### 9.4 Chat Event Stream to Browser

`GET /api/runners/chat/sessions/<id>/events/?after=<seq>`

Use browser-facing SSE:

```text
event: chat.event
id: 42
data: {"seq":42,"kind":"assistant_delta","payload":{"delta":"hello"}}
```

Implementation:

- serve this endpoint through Django's ASGI path, not a blocking WSGI
  `StreamingHttpResponse`; a sync streaming response pins one gunicorn worker
  per open chat tab and will exhaust small worker pools,
- use `redis.asyncio` or the Channels layer from async code; do not call
  blocking `redis-py` pub/sub methods from the event loop,
- authenticate via normal session cookie,
- verify access before stream opens,
- subscribe to `agent_chat_session:{session_id}` first,
- read missed persisted events with `seq > after` after the subscription is
  active,
- dedupe by `seq` between DB replay and pub/sub delivery,
- then forward pub/sub events,
- heartbeat every 15 seconds,
- close when session enters terminal `closed` or `failed`, unless client
  reconnects.

Replay correctness matters: subscribing after the DB replay creates a loss
window where an event can commit and publish between the two operations. The
implementation must subscribe first, replay from DB second, and discard any
pub/sub event whose `seq` was already emitted during replay.

This is separate from runner transport. It gives the browser a chat-app feel
even if runner control is still long-poll.

### 9.5 Runner-Upstream Chat Endpoints

Add runner-facing endpoints under `/api/v1/runner/chat/sessions/<id>/...`.

```python
path("chat/sessions/<uuid:session_id>/started/", ChatStartedEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/events/", ChatEventEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/messages/<uuid:message_id>/started/", ChatMessageStartedEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/messages/<uuid:message_id>/complete/", ChatMessageCompleteEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/approvals/", ChatApprovalEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/failed/", ChatFailedEndpoint.as_view())
path("chat/sessions/<uuid:session_id>/closed/", ChatClosedEndpoint.as_view())
```

All require `RunnerAccessTokenAuthentication` and `session.runner_id ==
request.auth_runner.id`.

Use an idempotency model parallel to `RunMessageDedupe` for runner POSTs that
do not already have a natural unique key:

```python
class ChatMessageDedupe(models.Model):
    session = models.ForeignKey("runner.AgentChatSession", on_delete=models.CASCADE, related_name="message_dedupes")
    message_id = models.CharField(max_length=128)
    created_at = models.DateTimeField(auto_now_add=True)
```

Unique `(session, message_id)`.

Every runner-upstream endpoint must be idempotent:

- `ChatEventEndpoint` requires `Idempotency-Key`; for bridge events, the runner
  should use a stable key such as
  `chat_event:{chat_session_id}:{bridge_seq}`. The cloud stores this key in
  `AgentChatEvent.source_key` and returns the existing event on duplicate.
- `ChatMessageStartedEndpoint` and `ChatMessageCompleteEndpoint` are
  idempotent by `(session, message_id, endpoint_kind)`.
- `ChatStartedEndpoint`, `ChatFailedEndpoint`, and `ChatClosedEndpoint` require
  `Idempotency-Key` and use `ChatMessageDedupe`.
- Approval requests are idempotent by `(session, local_approval_id)` using
  `update_or_create`, not by `ChatMessageDedupe`.

## 10. Runner Cloud Protocol

Extend `runner/src/cloud/protocol.rs`.

### 10.1 Cloud -> Runner

```rust
pub enum ServerMsg {
    // existing...
    ChatUserMessage {
        chat_session_id: Uuid,
        message_id: Uuid,
        content: String,
        content_parts: Vec<serde_json::Value>,
        local_thread_id: Option<String>,
        local_session_id: Option<String>,
        cwd: Option<String>,
        model: Option<String>,
    },
    ChatCancel {
        chat_session_id: Uuid,
        reason: Option<String>,
    },
    ChatClose {
        chat_session_id: Uuid,
        reason: Option<String>,
    },
    ChatDecide {
        chat_session_id: Uuid,
        approval_id: Uuid,
        local_approval_id: String,
        decision: ApprovalDecision,
        decided_by: Option<String>,
    },
}
```

The cloud sends `local_thread_id` / `local_session_id` back to the runner on
each `ChatUserMessage` so the runner can remain mostly stateless across
messages and process restarts. The cloud is the durable conversation index; the
runner owns only the local agent execution for the active turn. This tradeoff
adds a few fields to the control message but avoids requiring a long-lived
runner memory map keyed by `chat_session_id`.

### 10.2 Runner -> Cloud

```rust
pub enum ClientMsg {
    // existing...
    ChatStarted {
        chat_session_id: Uuid,
        local_thread_id: String,
        local_session_id: Option<String>,
        started_at: DateTime<Utc>,
    },
    ChatMessageStarted {
        chat_session_id: Uuid,
        message_id: Uuid,
        turn_id: Option<String>,
        started_at: DateTime<Utc>,
    },
    ChatEvent {
        chat_session_id: Uuid,
        bridge_seq: u64,
        kind: String,
        payload: serde_json::Value,
    },
    ChatApprovalRequest {
        chat_session_id: Uuid,
        local_approval_id: String,
        kind: ApprovalKind,
        payload: serde_json::Value,
        reason: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    },
    ChatMessageCompleted {
        chat_session_id: Uuid,
        message_id: Uuid,
        turn_id: Option<String>,
        assistant_message_id: Option<Uuid>,
        status: String,
        completed_at: DateTime<Utc>,
    },
    ChatFailed {
        chat_session_id: Uuid,
        code: String,
        detail: Option<String>,
        failed_at: DateTime<Utc>,
    },
    ChatClosed {
        chat_session_id: Uuid,
        closed_at: DateTime<Utc>,
    },
}
```

HTTP transport dispatch in `RunnerCloudClient.dispatch_client_msg` must map
these new `ClientMsg` variants to the new runner-upstream chat endpoints.
`bridge_seq` is runner-local ordering metadata only. The cloud must copy it
into the persisted event payload and assign its own `AgentChatEvent.seq` under
the session lock.

### 10.3 Outbox

Update `services/outbox.py`:

- add valid message types:
  - `chat_user_message`
  - `chat_cancel`
  - `chat_close`
  - `chat_decide`
- reject all chat messages while runner offline.

Do not offline-buffer chat messages. Chat is synchronous/live; if the runner is
offline, the API should return a 409 and the UI should show unavailable.

## 11. Runner Implementation

### 11.1 Agent Chat Bridge Interface

Do not stretch `RunPayload` to fit chat. Add chat-specific bridge methods.

```rust
pub enum AgentBridge {
    Codex(crate::codex::bridge::Bridge),
    ClaudeCode(crate::claude_code::bridge::Bridge),
}

pub struct ChatUserMessagePayload {
    pub chat_session_id: Uuid,
    pub message_id: Uuid,
    pub content: String,
    pub content_parts: Vec<serde_json::Value>,
    pub local_thread_id: Option<String>,
    pub local_session_id: Option<String>,
    pub cwd: PathBuf,
    pub model: Option<String>,
}

pub enum ChatBridgeEvent {
    Started { chat_session_id: Uuid, local_thread_id: String, local_session_id: Option<String> },
    MessageStarted { chat_session_id: Uuid, message_id: Uuid, turn_id: Option<String> },
    Raw { chat_session_id: Uuid, bridge_seq: u64, method: String, params: serde_json::Value },
    AssistantDelta { chat_session_id: Uuid, bridge_seq: u64, delta: String, item_id: Option<String>, turn_id: Option<String> },
    AssistantMessage { chat_session_id: Uuid, bridge_seq: u64, text: String, item_id: Option<String>, turn_id: Option<String> },
    ApprovalRequest { chat_session_id: Uuid, local_approval_id: String, kind: ApprovalKind, payload: serde_json::Value, reason: Option<String> },
    TurnCompleted { chat_session_id: Uuid, turn_id: Option<String>, status: String },
    Failed { chat_session_id: Uuid, code: String, detail: Option<String> },
}
```

Add methods:

```rust
impl AgentBridge {
    pub async fn chat_send(&mut self, payload: &ChatUserMessagePayload) -> Result<AgentChatCursor>;
    pub async fn chat_next_events(&mut self, cursor: &mut AgentChatCursor) -> Option<Vec<ChatBridgeEvent>>;
    pub async fn chat_send_approval(&mut self, local_approval_id: &str, decision: ApprovalDecision) -> Result<()>;
    pub async fn chat_cancel(&mut self, cursor: &AgentChatCursor) -> Result<()>;
    pub async fn chat_close(self, grace: Duration) -> Result<()>;
}
```

`AgentCursor` and `AgentChatCursor` should remain separate because task runs
and chat have different lifecycle and terminal conditions.

`chat_send_approval` mirrors the existing task-mode
`AgentBridge::send_approval`; `ServerMsg::ChatDecide` routes through it using
`local_approval_id`. The cloud UUID `approval_id` remains in the control frame
for logging and correlation only. Codex can reuse its approval response wire
format. Claude Code should keep returning a clear error until its non-bypass
approval flow is implemented.

### 11.2 Codex Chat Bridge

Use the same `AppServer` process abstraction.

MVP flow:

1. `chat_send`
   - spawn `codex app-server` for the active chat turn,
   - ensure `initialize` is sent,
   - if `local_thread_id` is empty, send `thread/start`,
   - if `local_thread_id` is present, send `thread/resume`,
   - send `turn/start` with current `thread_id`,
   - store returned `turn.id` on cursor,
   - emit `ChatStarted` when a new/resumed thread is ready,
   - emit `ChatMessageStarted`.
2. `chat_next_events`
   - read app-server notifications,
   - map known item/turn methods to `ChatBridgeEvent`,
   - preserve unknowns as `Raw`.
3. `chat_cancel`
   - send `turn/interrupt` with `thread_id` and active `turn_id`.
4. `chat_close`
   - send shutdown or drop app-server with grace.

MVP may shut down the app-server after each `turn/completed`, then resume by
`thread/resume` on the next chat message. This matches the current
one-worker-per-task runner shape and keeps idle chat sessions from occupying a
runner slot, but it adds cold-start latency to every chat turn. The preferred
MVP behavior is a short keep-warm window, for example 30 seconds after turn
completion, before shutting down the app-server. That keeps common follow-up
messages responsive while still releasing the runner quickly.

Idle timeout:

- runner should close any still-running chat process after 30 minutes of no
  activity,
- cloud may also mark old idle sessions `closed` through a sweeper.

### 11.3 Claude Code Chat Bridge

Claude Code CLI supports:

- `--print`,
- `--output-format stream-json`,
- `--input-format stream-json`,
- `--resume <session-id>`,
- `--continue`.

Current Pi Dash Claude bridge is one subprocess per task. Preserve that style
for MVP chat:

1. first chat message:
   - spawn `claude -p --output-format stream-json`,
   - read `system/init.session_id`,
   - store `local_session_id`,
   - stream result,
   - process exits.
2. next chat message:
   - spawn `claude --resume <local_session_id> -p --output-format stream-json`,
   - stream result,
   - overwrite `local_session_id` with the latest `system/init.session_id` if
     Claude returns one.

This gives persistent conversation semantics without keeping an interactive
Claude process alive. It also matches the current Rust process wrapper better
than trying to manage a live REPL.

### 11.4 RunnerLoop State

Add:

```rust
struct CurrentChat {
    chat_session_id: Uuid,
    cancel: Arc<Notify>,
    done_rx: oneshot::Receiver<()>,
}
```

`RunnerLoop` handles:

- `ServerMsg::ChatUserMessage`
- `ServerMsg::ChatCancel`
- `ServerMsg::ChatClose`
- `ServerMsg::ChatDecide`

Recommended worker:

```rust
struct ChatWorker {
    runner_paths: RunnerPaths,
    runner_config: RunnerConfig,
    state: StateHandle,
    approvals: ApprovalRouter,
    out: RunnerOut,
    cancel: Arc<Notify>,
    bridge: Option<AgentBridge>,
    cursor: Option<AgentChatCursor>,
}
```

For MVP, one active chat turn can be active per runner. `current_chat`
represents that active turn, not an idle open session. When the turn completes,
the worker clears `current_chat`, sets runner status back to idle, and leaves
the cloud `AgentChatSession` open for a future message.

For Codex keep-warm, the app-server process may remain cached after
`current_chat` is cleared. A warmed process must not keep the runner busy by
itself; only an active turn blocks task assignment and new chat sends.

While `current_chat` is set, update the runner `StateHandle` status to
`RunnerStatus::Busy` with `in_flight_run = None`. The existing poll path will
mark the cloud runner row `busy`, which keeps the web UI and task matcher
honest without pretending there is an `AgentRun` in flight.

### 11.5 Local History

Add chat history JSONL separate from run history:

```
$RUNNER_DATA/runners/<runner_id>/chats/<chat_session_id>.jsonl
```

Record:

- header,
- user messages,
- raw agent events,
- assistant deltas/messages,
- approvals,
- terminal close/failure.

This is runner-local debug history only. Cloud DB is the user-visible
transcript.

## 12. Cloud Persistence and Event Fan-Out

Every runner-upstream chat event should:

1. verify runner owns session,
2. idempotency-check `Idempotency-Key` when the endpoint has no natural unique
   domain key,
3. lock the `AgentChatSession` row with `select_for_update()`,
4. assign the next cloud event `seq`,
5. insert/update `AgentChatEvent`,
6. update `AgentChatMessage` and `AgentChatSession`,
7. publish to Redis pub/sub for browser SSE subscribers.

Redis channel:

```
agent_chat_session:{session_id}
```

Payload:

```json
{
  "seq": 12,
  "kind": "assistant_delta",
  "payload": { "delta": "hello" },
  "created_at": "..."
}
```

### 12.1 Seq Assignment

Cloud DB ordering is cloud-owned:

- `AgentChatEvent.seq` is assigned by the cloud while holding
  `select_for_update()` on the `AgentChatSession` row.
- `AgentChatMessage.seq` is also assigned by the cloud under the same session
  lock when creating user/assistant/tool messages.
- Runner-supplied event order must be stored as `bridge_seq` inside
  `AgentChatEvent.payload` when useful for debugging, but it must not be used
  as the DB `seq`.

This prevents `unique(session, seq)` races when multiple runner-upstream POSTs
arrive close together. It also keeps browser replay ordering independent of the
transport batching strategy.

### 12.2 Persistence Rules

Delta handling:

- persist deltas as events for replay,
- do not write `AgentChatMessage.content` on every token delta,
- update assistant message content on `turn/completed`, or checkpoint every N
  deltas / every 1-2 seconds if the UI needs crash recovery mid-stream,
- throttle browser SSE in the API layer if needed.

Turn completion handling:

- set the user message `status="completed"` unless a failure/cancel happened,
- set or complete the assistant message,
- clear `AgentChatSession.active_turn_id`,
- clear `AgentChatSession.active_message_id`,
- set `AgentChatSession.status="open"` so the next message can start another
  turn,
- update `last_message_at`.

BrowserX's `chat-stream.ts` uses 150ms delta throttling. Pi Dash can use the
same threshold for web SSE fan-out to avoid rendering every token as a separate
React update.

### 12.3 Cancel and Close Handling

Cancel is turn-scoped:

1. browser calls `POST /chat/sessions/<id>/cancel/`,
2. cloud locks the session and runner,
3. if no active turn/message exists, return `200 {"ok": true, "noop": true}`,
4. enqueue `chat_cancel` through `transaction.on_commit`,
5. keep `active_message_id` / `active_turn_id` populated until the runner posts
   `ChatMessageCompleted` with cancelled status or `ChatFailed` with
   `code="cancelled"`,
6. on runner acknowledgement, mark the active user and assistant messages
   `cancelled`, append a `turn_completed` or `chat_failed` event with cancelled
   status, clear active ids, and publish to SSE subscribers.

Cancel timeout:

- if the runner does not acknowledge cancellation within the active-turn
  timeout, the active-turn sweeper marks the active messages `failed`, appends
  `chat_failed`, clears active ids, and leaves the session `open`.

Close is session-scoped:

1. reject close only if the user cannot access the session,
2. if an active turn exists, enqueue `chat_cancel` first and mark close as
   pending in the event payload or session error text,
3. set `AgentChatSession.status="closed"` only after active ids are clear,
4. append `chat_closed`,
5. enqueue `chat_close` best-effort to let the runner drop any keep-warm local
   process,
6. make later sends return `409 chat_session_closed`.

## 13. UI Implementation

### 13.1 Routes

Update `apps/web/app/routes/core.ts`:

```ts
layout("./(all)/[workspaceSlug]/runners/layout.tsx", [
  route(":workspaceSlug/runners", "./(all)/[workspaceSlug]/runners/page.tsx"),
  route(":workspaceSlug/runners/runs", "./(all)/[workspaceSlug]/runners/runs/page.tsx"),
  route(":workspaceSlug/runners/approvals", "./(all)/[workspaceSlug]/runners/approvals/page.tsx"),
  route(":workspaceSlug/runners/chat/:runnerId", "./(all)/[workspaceSlug]/runners/chat/[runnerId]/page.tsx"),
]);
```

### 13.2 Layout

Replace the top-tab layout in `runners/layout.tsx` with:

```tsx
<div className="flex h-full w-full overflow-hidden">
  <AIAgentsMiddlePanel />
  <main className="min-w-0 flex-1 overflow-auto">
    <Outlet />
  </main>
</div>
```

Middle panel responsibilities:

- fetch runners by current workspace,
- render `Overview`,
- render runners,
- highlight active route,
- show online/busy/offline indicators,
- keep width stable around 280px,
- no nested cards.

The existing top tabs can move into the overview page.

### 13.3 Chat Page Components

Add:

```
apps/web/app/(all)/[workspaceSlug]/runners/chat/[runnerId]/page.tsx
apps/web/core/components/runners/chat/agent-chat-panel.tsx
apps/web/core/components/runners/chat/chat-message-list.tsx
apps/web/core/components/runners/chat/chat-composer.tsx
apps/web/core/components/runners/chat/use-agent-chat-events.ts
```

Chat page state:

- `runner`
- `session`
- `messages`
- `events`
- `composerDisabledReason`
- `streamingAssistantMessage`

Use existing `RunnerService` pattern in `packages/services/src/runner`.

Add types in `packages/types/src/runner.ts`:

- `IAgentChatSession`
- `IAgentChatMessage`
- `IAgentChatEvent`
- `IAgentChatApprovalRequest`

Add service methods:

- `listChatSessions`
- `createChatSession`
- `getChatSession`
- `listChatMessages`
- `sendChatMessage`
- `cancelChat`
- `closeChat`
- `streamChatEvents` helper or hook-level EventSource URL builder.

### 13.4 UX Details

Chat body:

- assistant/user bubbles or compact Slack-style rows,
- tool events as compact expandable rows,
- approval request as an inline action block,
- failed messages show error state,
- completed assistant message replaces accumulated delta rendering.

Composer:

- send button icon,
- enter sends, shift-enter newline,
- disabled when runner unavailable/busy,
- stop button while active turn streams.

The chat UI should not contain explanatory marketing copy. It should behave
like an operational desktop chat surface.

## 14. Permissions

Read chat:

- workspace member can list runners,
- session creator can read their session,
- workspace admin can read all chat sessions in workspace.

Create/send chat:

- workspace member can chat with a runner in the workspace,
- runner must be online and not revoked,
- future policy may restrict to runner owner/admin.

Approval decisions:

- session creator or workspace admin.

Use 404 rather than 403 when revealing session existence across workspaces
would leak information, matching `AgentRunDetailEndpoint`.

## 15. SSE Strategy

There are two independent SSE paths.

### 15.1 Browser SSE: Required for Chat UX

Browser -> cloud SSE should be part of the first implementation because it is
the simplest way to deliver chat deltas to the React UI.

```
browser -> cloud: GET /api/runners/chat/sessions/<id>/events/?after=<seq>
```

This does not affect runner transport.

### 15.2 Runner SSE: Transport Upgrade

Runner -> cloud can later open an outbound SSE control stream:

```
runner -> cloud: GET /api/v1/runner/runners/<rid>/sessions/<sid>/events
cloud -> runner: event: control
runner -> cloud: POST acknowledgements/results/events
```

Runner SSE can replace or supplement long-poll:

- same `ServerMsg` payloads,
- same ack model,
- same outbox source,
- lower latency.

Do not make direct chat depend on runner SSE in phase 1. Existing long-poll is
already NAT-safe and compatible with per-runner sessions.

## 16. Failure Modes

### 16.1 Runner Goes Offline Mid-Chat

Cloud:

- session remains `open`, but `active_message_id` / `active_turn_id` may stay
  populated until a sweeper marks the active message failed,
- UI shows runner offline,
- user can close session,
- no automatic retry.

Runner:

- on restart, chat sessions are not automatically resumed in MVP,
- future phase can resume Codex by `thread/resume` if `local_thread_id` is
  present and runner local Codex state still exists.

### 16.2 Browser Disconnects

SSE reconnects with `Last-Event-ID` or `?after=<seq>`.

Persisted `AgentChatEvent` is authoritative for replay.

### 16.3 Duplicate Runner Posts

Use `ChatMessageDedupe`, same idea as `RunMessageDedupe`, only for endpoints
without a natural unique key. Persisted event `seq` is cloud-owned and must not
be used as a retry key. Use `AgentChatEvent.source_key` for event
`Idempotency-Key` values, `(session, local_approval_id)` for chat approval
requests, and `(session, message_id, endpoint_kind)` for message
started/completed endpoints.

### 16.4 Orphaned Active Turn

If the runner crashes mid-turn, the cloud may have `active_message_id` or
`active_turn_id` populated forever. Add a periodic sweeper:

- find open sessions with active state whose runner is offline or whose
  `updated_at` is older than the active-turn timeout,
- mark the active user/assistant messages `failed`,
- append a `chat_failed` event,
- clear `active_message_id` and `active_turn_id`,
- leave the session `open` when the runner is back online, or set `failed` when
  the session itself is unrecoverable.

### 16.5 Assign Arrives During Chat

Cloud matcher should exclude active-chat runners.

Runner safety net:

- if `Assign` arrives while chat active, send `RunFailed` with `Internal` and
  detail `"runner busy with chat"` or ignore if run is not yet accepted.

Preferred behavior is preventing assignment in matcher so this path is rare.

### 16.6 Chat Message Arrives During Task

Cloud API rejects before enqueue with `409 runner_busy`.

Runner safety net emits `ChatFailed`.

### 16.7 Single-Runner Fleet UX

In a one-runner pod, direct chat and task work block each other because MVP has
no queue between modes. This is intentional for correctness, but the UI should
make the reason visible: "Runner is busy with chat" or "Runner is busy with
task." Operators with small runner fleets may need to close/stop chat before
task assignment can proceed.

### 16.8 Cost and Rate Limits

Direct chat can drive model spend outside issue automation. Phase 1 should add
basic request throttling per user/session and leave full token-budget policy as
explicitly out of scope for this track unless product requirements change.

## 17. Implementation Phases

### Phase 1a: Cloud Models and Command API

- Add migrations for:
  - `AgentChatSession`
  - `AgentChatMessage`
  - `AgentChatEvent`
  - `AgentChatApprovalRequest`
  - `ChatMessageDedupe`
- Add serializers.
- Add web-facing chat endpoints.
- Add runner-facing chat lifecycle endpoints.
- Add matcher exclusion for active chat turns.
- Add active-turn sweeper.
- Add basic per-user/session send throttling.
- Update stale developer docs such as `CLAUDE.md` so the runner plane describes
  v4 HTTPS long-poll instead of the retired WebSocket protocol.
- Add tests for permissions, busy rejection, idempotency, active-turn cleanup,
  empty-session reuse, send throttling, seq assignment under concurrent runner
  posts, on-commit enqueue failure cleanup, `cwd` containment, and matcher
  exclusion.

End state: cloud can create sessions/messages, persist events, reject busy
runners, and recover orphaned active state even before runner support lands.

### Phase 1b: Browser SSE and Event Fan-Out

- Add Redis pub/sub helper for chat event fan-out.
- Add ASGI browser SSE endpoint.
- Implement subscribe-first replay correctness and `seq` dedupe.
- Keep SSE implementation fully async (`AsyncHttpConsumer` or async Django view
  plus `redis.asyncio`).
- Add tests for replay from `after`, reconnect, duplicate delivery, and terminal
  stream close.

End state: browser clients can stream persisted and live chat events without
pinning WSGI workers.

### Phase 2: Runner Protocol and Codex Chat

- Extend `ClientMsg` and `ServerMsg`.
- Extend outbox valid message types.
- Map HTTP `ClientMsg` dispatch to chat endpoints.
- Add chat bridge API in `agent/mod.rs`.
- Implement Codex chat:
  - initialize once,
  - `thread/start` for first message,
  - `thread/resume` for later messages,
  - `turn/start`,
  - stream notifications,
  - `turn/interrupt`,
  - 30-second keep-warm after the active turn,
  - process shutdown after keep-warm or explicit close.
- Add `RunnerLoop` chat worker and `current_chat`.
- Add runner tests with fake Codex app-server:
  - `chat_user_message`,
  - first-message `thread/start`,
  - later-message `thread/resume`,
  - delta event,
  - turn complete,
  - cancel,
  - approval decision,
  - keep-warm shutdown after idle timeout.

End state: Codex runner can complete direct chat end to end through cloud
long-poll and runner-upstream REST.

### Phase 3: Web AI Agents Middle Panel and Chat UI

- Replace runners layout with route-scoped AI Agents shell.
- Add `AIAgentsMiddlePanel`.
- Preserve overview/runs/approvals routes.
- Add chat route/page.
- Add chat service methods/types.
- Add browser SSE hook.
- Add composer, message list, stop/close controls.
- Add tests for route selection and disabled composer state.

End state: user can click AI Agents, select a runner, chat, see streaming
assistant output, and stop/close.

### Phase 4: Claude Code Chat

- Add Claude chat bridge using resumable per-message subprocesses.
- Store Claude `session_id` in `AgentChatSession.local_session_id`.
- Add fake Claude bridge tests for first message and resumed second message.

End state: both supported runner agent kinds can chat.

### Phase 5: Runner SSE Transport

- Add runner-facing SSE control endpoint.
- Add Rust SSE client task per runner session.
- Feed SSE control frames into the same `RunnerLoop` mailbox as long-poll.
- Keep long-poll fallback.
- Add reconnection and ack behavior.

End state: chat/control latency improves without changing domain logic.

## 18. Testing Plan

### Django

- model constraints and serializers,
- create session permission checks,
- send message busy checks,
- empty open session cap per `(created_by, runner)`,
- event POST idempotency and approval-id idempotency,
- send on-commit failure marks message failed and clears active ids,
- `cwd` outside workspace root returns `400 invalid_cwd`,
- concurrent runner posts assign unique cloud `seq` values,
- SSE replay from `after` with subscribe-first gap coverage,
- cancel/close status transitions and cancel timeout,
- active-turn sweeper clears orphaned busy state,
- basic per-user/session send throttling,
- approval create/decide,
- matcher excludes active chat.

### Rust Runner

- protocol roundtrips for new messages,
- fake Codex first-message and resumed-message send/delta/complete,
- cancel maps to `turn/interrupt`,
- approval decision maps to `chat_send_approval(local_approval_id, decision)`,
- busy task rejects chat,
- active chat blocks assign,
- HTTP dispatch maps chat `ClientMsg` to correct endpoint.

### Web

- middle panel renders overview and runner rows,
- active route highlights correct item,
- chat page disables composer for offline/busy runner,
- send message appends optimistic user message,
- SSE delta appends to streaming assistant message,
- terminal event completes assistant message.

### End-to-End

1. enroll runner,
2. open `/runners`,
3. select runner,
4. send "What repo are you in?",
5. cloud creates `AgentChatSession` and user message,
6. runner receives `chat_user_message`,
7. Codex creates thread and turn,
8. runner posts deltas/events,
9. browser SSE renders response,
10. turn completes and composer re-enables.

Busy E2E:

1. start an `AgentRun`,
2. open runner chat,
3. composer disabled,
4. direct POST returns `409 runner_busy`.

## 19. Open Decisions

1. Default chat working directory:
   - recommended: runner's resolved workspace path for its pod/project when
     available; otherwise runner configured cwd. User-provided paths must stay
     inside that root and should be ignored in MVP unless there is a product
     requirement for manual `cwd`.
2. Visibility:
   - recommended MVP: creator + workspace admins can read a chat session.
3. Session reuse:
   - recommended MVP: reuse latest open session for current user + runner;
     provide "New chat" later.
4. Task assignment during idle open chat:
   - recommended MVP: only active chat turns block tasks. An open but idle chat
     session does not block task matching.
5. Chat transcript retention:
   - recommended MVP: no special retention policy; follows workspace data
     retention.

## 20. Success Criteria

The track is complete when:

- AI Agents has a route-scoped middle panel with Overview and runner rows.
- Selecting a runner opens a chat page.
- A user can create a chat session and send messages to an idle Codex runner.
- Assistant output streams into the browser.
- Chat messages/events persist and replay after refresh.
- Runner busy state disables chat and is enforced server-side.
- Task runs still work and still use independent agent sessions.
- Existing runner runs/approvals overview remains accessible.
- Codex direct chat works end to end.
- Claude Code direct chat works through resumable sessions, or is explicitly
  hidden/disabled until Phase 4 lands.
