"""Redis priming for worker paths that read cache keys.

``send_email_notification`` looks up the per-issue base API URL under the
issue id and silently returns when the key is absent, so mail tests prime
the key before publishing the stack task.
"""

import redis

from . import config


def setex(key: str, value: str, ttl_seconds: int = 600) -> None:
    client = redis.Redis.from_url(config.REDIS_URL)
    client.setex(key, ttl_seconds, value)
