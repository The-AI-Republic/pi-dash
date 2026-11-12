# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pytest driver for write-replay (PIDASHCONV-823).

One test per corpus session. Needs the live topology from
``replay/README.md`` (two Djangos + Rust + brokers + snapshot); without
``REPLAY_DJANGO_URL`` the module skips with a pointer instead of
failing, so bare ``pytest`` runs elsewhere stay green. The
``rust-api-replay.yml`` CI job provides the topology and runs every
session for real.
"""

import os

import pytest

from .corpus import ALL_SESSIONS
from .runner import Divergence, ReplayConfig, Runner, Sinks

pytestmark = pytest.mark.contract

NEEDS_TOPOLOGY = os.environ.get("REPLAY_DJANGO_URL", "") == ""
SKIP_REASON = (
    "write-replay needs the live two-backend topology "
    "(see rust-api/contract-tests/replay/README.md)"
)


@pytest.fixture(scope="module")
def replay_config():
    if NEEDS_TOPOLOGY:
        pytest.skip(SKIP_REASON)
    return ReplayConfig.from_env()


@pytest.fixture(scope="module")
def replay_sinks(replay_config):
    from _harness.sinks import WorkerSmtpSink, WorkerWebhookSink

    django_smtp = WorkerSmtpSink()
    rust_smtp = WorkerSmtpSink()
    django_webhook = WorkerWebhookSink()
    rust_webhook = WorkerWebhookSink()
    django_smtp.start("127.0.0.1", int(os.environ["REPLAY_DJANGO_SMTP_PORT"]))
    rust_smtp.start("127.0.0.1", int(os.environ["REPLAY_RUST_SMTP_PORT"]))
    django_webhook.start(os.environ["REPLAY_DJANGO_WEBHOOK_BASE"])
    rust_webhook.start(os.environ["REPLAY_RUST_WEBHOOK_BASE"])
    yield Sinks(
        django_smtp=django_smtp,
        rust_smtp=rust_smtp,
        django_webhook=django_webhook,
        rust_webhook=rust_webhook,
    )
    django_smtp.stop()
    rust_smtp.stop()
    django_webhook.stop()
    rust_webhook.stop()


@pytest.fixture(scope="module")
def runner(replay_config, replay_sinks):
    return Runner(replay_config, replay_sinks)


@pytest.mark.parametrize("session", ALL_SESSIONS, ids=[s.name for s in ALL_SESSIONS])
def test_replay_session(runner, session, capsys):
    try:
        report = runner.run_session(session)
    except Divergence:
        raise
    with capsys.disabled():
        print(f"\n== {report.session}: {len(report.requests)} requests, all equal ==")
        for item in report.requests:
            print(
                f"  [{item.index}] {item.method} {item.route} "
                f"-> {item.django_status}/{item.rust_status} "
                f"tables={item.tables_changed or '-'} "
                f"jobs={item.jobs} mail={item.mail} hooks={item.hooks}"
            )
