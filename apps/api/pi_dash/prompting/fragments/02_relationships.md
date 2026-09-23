{% if parent or children or related or blocked_by or blocking or other_relations %}
## Work item relationships

This work item is connected to the work items below.
{% if parent %}
Ancestors (up):
{% if lineage %}
- Lineage (current → root): {% for node in lineage %}{{ node.identifier }}{% if loop.first %} (current){% endif %}{% if not loop.last %} → {% endif %}{% endfor %}
{% endif %}
- Parent: {{ parent.identifier }} — {{ parent.title }}{% if parent.state %} ({{ parent.state }}){% endif %}
{% if parent.description %}
{{ parent.description }}
{% else %}
(no description on the parent issue)
{% endif %}
- The parent has {{ parent.comments_count }} comment(s); run `pidash comment list {{ parent.identifier }}` to read them.
{% endif %}
{% if children %}
Children (down):
{% for child in children %}
- {{ child.identifier }}: {{ child.title }}{% if child.state %} ({{ child.state }}){% endif %}
{% endfor %}
{% endif %}
{% if related %}
Related work items (across):
{% for item in related %}
- {{ item.identifier }}: {{ item.title }}{% if item.state %} ({{ item.state }}){% endif %}
{% endfor %}
{% endif %}
{% if blocked_by %}
Blocked by (must be done first):
{% for item in blocked_by %}
- {{ item.identifier }}: {{ item.title }}{% if item.state %} ({{ item.state }}){% endif %}
{% endfor %}
{% if has_open_blockers %}
- Warning: {{ open_blockers | join(", ") }} {% if open_blockers | length == 1 %}is{% else %}are{% endif %} still open — decide whether this work can proceed before you start (see below).
{% endif %}
{% endif %}
{% if blocking %}
Blocking (waiting on this item):
{% for item in blocking %}
- {{ item.identifier }}: {{ item.title }}{% if item.state %} ({{ item.state }}){% endif %}
{% endfor %}
{% endif %}
{% if other_relations %}
Other relations:
{% for item in other_relations %}
- {{ item.relation }} {{ item.identifier }}: {{ item.title }}{% if item.state %} ({{ item.state }}){% endif %}
{% endfor %}
{% endif %}
Required reading before you implement: for every work item listed above, run `pidash issue get <ID>` and `pidash comment list <ID>`, and fold their framing, acceptance criteria, design decisions and research findings into your workpad. Use the ancestors to judge whether this issue is ready to implement, the children to understand what has already been scoped out of it, the related items for adjacent context, and the blocked-by / blocking items to know what this work depends on and who is waiting on it. Do this once, early in analyze-and-scope, and record what you learn so continuation runs do not re-fetch.
{% if has_open_blockers %}
Open blockers ({{ open_blockers | join(", ") }}) are information, not a hard stop — you decide whether to proceed. For each open blocker, read it (`pidash issue get <ID>`, `pidash comment list <ID>`, `pidash workpad get <ID>`) and judge whether this work can proceed safely: is the blocker's interface or data shape settled, and does this issue touch it at all?
- If it can proceed, do the work and record the assumption in your workpad (e.g. "proceeding while X is in review; assumes the field names in X's workpad").
- If it cannot, record in your workpad which blocker you are waiting for and why, then call `pidash issue wait {{ issue.identifier }}` and end the run (`pidash run yield --outcome waiting_on_external`). Post at most one short comment saying what you are waiting for. Do not spend the run investigating unrelated things.

The wait is free: it buys back the tick this run is spending, so a run that ends by waiting costs no net budget. The next cadence tick asks you again — re-read the blockers then and decide afresh, because *you* decide when the wait ends. Nothing on the platform watches those blockers for you: a blocker closing fires no run, and no one reads your workpad. The allowance is one extra pool of waits; past it, `pidash issue wait` reports `wait_cap_reached` and waiting starts spending normal budget, so do not park on it indefinitely — if you are still blocked with the allowance gone, say so to the human in a comment.
{% endif %}
{% endif %}
