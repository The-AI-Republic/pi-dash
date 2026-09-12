{% if parent or children or related %}
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
Required reading before you implement: for every work item listed above, run `pidash issue get <ID>` and `pidash comment list <ID>`, and fold their framing, acceptance criteria, design decisions and research findings into your workpad. Use the ancestors to judge whether this issue is ready to implement, the children to understand what has already been scoped out of it, and the related items for adjacent context. Do this once, early in analyze-and-scope, and record what you learn so continuation runs do not re-fetch.
{% endif %}
