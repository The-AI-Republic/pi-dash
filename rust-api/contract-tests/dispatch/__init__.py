"""Task oracle for D-11 dispatch engines (PIDASHCONV-22).

No routes: the contract is the Celery wire format plus the Postgres
before/after diff. Each test publishes a job exactly as Django's
``task.delay()`` would (see ``_harness/celery.py``), lets the live worker
execute it, and asserts the database effect.

Covered tasks (``apps/api/pi_dash`` drift baseline 01a93e17216faea7bfc156b0f864cbbe420d1c52):

- ``cloud_agent.run_agent_run`` (``cloud_agent/tasks.py``)
- ``cloud_agent.scan_queued_runs`` (``cloud_agent/tasks.py``)
- ``cloud_agent.sweep_stale_runs`` (``cloud_agent/tasks.py``)
- ``managed_runner.expire_waiting_runs`` (``managed_runner/tasks.py``)

Side effects are database rows (``agent_run`` + ``agent_run_event``) and
broker messages (dispatch publishes follow-on ``run_agent_run`` jobs).
These tasks emit no mail and no webhooks, so no SMTP/HTTP sinks are needed.

Required environment (see ``../README.md``): ``BASE_URL`` (unused by these
tests beyond the session fixture), ``DATABASE_URL``,
``CELERY_BROKER_URL`` (or ``AMQP_URL``), and the server running with
``CLOUD_AGENT_ENABLED=true``.
"""
