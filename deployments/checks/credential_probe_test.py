# Copyright (c) Pi Dash contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""The community installer's credential probe, against the app's own
client libraries (the versions pinned in apps/api/requirements/base.txt)
and small fake PostgreSQL / RabbitMQ servers that speak just enough of
each protocol to log in. HOSTALIASES points the bundled service names at
the fakes, so the probe runs unchanged.

Run: pip install kombu psycopg[binary] dj-database-url (see base.txt), then
     python -m unittest discover -s deployments/checks -p '*_test.py'
"""

import os
import re
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest

from amqp.serialization import dumps, loads

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
INSTALL_SH = os.path.join(ROOT, "deployments", "cli", "community", "install.sh")


def probe_source():
    with open(INSTALL_SH, encoding="utf-8") as handle:
        text = handle.read()
    match = re.search(r"<<'PROBE'[^\n]*\n(.*?)\nPROBE\n", text, re.S)
    if match is None:
        raise AssertionError("install.sh has no <<'PROBE' heredoc")
    return match.group(1)


def listener():
    try:
        sock = socket.create_server(("", 0), family=socket.AF_INET6, dualstack_ipv6=True)
    except (OSError, ValueError):
        sock = socket.create_server(("127.0.0.1", 0))
    return sock


class FakeServer:
    """Accepts connections on a thread; handle(conn) speaks the protocol."""

    def __init__(self):
        self.sock = listener()
        self.port = self.sock.getsockname()[1]
        self.seen = []
        threading.Thread(target=self._serve, daemon=True).start()

    def _serve(self):
        while True:
            try:
                conn, _ = self.sock.accept()
            except OSError:
                return
            threading.Thread(target=self._handle, args=(conn,), daemon=True).start()

    def _handle(self, conn):
        try:
            with conn:
                conn.settimeout(10)
                self.handle(conn)
        except (OSError, EOFError, struct.error):
            pass

    def close(self):
        self.sock.close()


def recv_exact(conn, n):
    data = b""
    while len(data) < n:
        chunk = conn.recv(n - len(data))
        if not chunk:
            raise EOFError
        data += chunk
    return data


class FakePostgres(FakeServer):
    """Cleartext-password login against {role: password} and a set of databases."""

    def __init__(self, roles, databases, hba_reject=False):
        self.roles, self.databases, self.hba_reject = roles, databases, hba_reject
        super().__init__()

    @staticmethod
    def message(kind, body):
        return kind + struct.pack("!I", len(body) + 4) + body

    def error(self, conn, code, text):
        fields = b"SFATAL\0VFATAL\0C" + code.encode() + b"\0M" + text.encode() + b"\0\0"
        conn.sendall(self.message(b"E", fields))

    def handle(self, conn):
        while True:
            length, code = struct.unpack("!II", recv_exact(conn, 8))
            if code in (80877103, 80877104):  # SSLRequest, GSSENCRequest
                conn.sendall(b"N")
                continue
            break
        raw = recv_exact(conn, length - 8).split(b"\0")
        params = dict(zip(raw[0::2], raw[1::2]))
        user = params.get(b"user", b"").decode()
        database = params.get(b"database", b"").decode()
        self.seen.append((user, database))
        if self.hba_reject:
            return self.error(
                conn, "28000", f'no pg_hba.conf entry for host "192.0.2.5", user "{user}", database "{database}", no encryption'
            )
        if user not in self.roles:
            return self.error(conn, "28000", f'role "{user}" does not exist')
        conn.sendall(self.message(b"R", struct.pack("!I", 3)))
        kind = recv_exact(conn, 1)
        (length,) = struct.unpack("!I", recv_exact(conn, 4))
        password = recv_exact(conn, length - 4).rstrip(b"\0").decode()
        if kind != b"p" or password != self.roles[user]:
            return self.error(conn, "28P01", f'password authentication failed for user "{user}"')
        if database not in self.databases:
            return self.error(conn, "3D000", f'database "{database}" does not exist')
        out = self.message(b"R", struct.pack("!I", 0))
        for key, value in (
            ("server_version", "15.7"),
            ("server_encoding", "UTF8"),
            ("client_encoding", "UTF8"),
            ("DateStyle", "ISO, MDY"),
            ("integer_datetimes", "on"),
            ("standard_conforming_strings", "on"),
        ):
            out += self.message(b"S", key.encode() + b"\0" + value.encode() + b"\0")
        out += self.message(b"K", struct.pack("!II", 1, 2)) + self.message(b"Z", b"I")
        conn.sendall(out)
        while conn.recv(1024):
            pass


class FakeRabbit(FakeServer):
    """AMQP 0-9-1 login against {user: password} and {user: {vhosts}}.

    hang=True accepts the TCP connection and never answers, like a node
    that stopped answering.
    """

    def __init__(self, users, permissions, hang=False):
        self.users, self.permissions, self.hang = users, permissions, hang
        super().__init__()

    @staticmethod
    def frame(class_id, method_id, fmt, values):
        payload = struct.pack("!HH", class_id, method_id) + dumps(fmt, values)
        return struct.pack("!BHI", 1, 0, len(payload)) + payload + b"\xce"

    @staticmethod
    def read_method(conn):
        while True:
            kind, _channel, size = struct.unpack("!BHI", recv_exact(conn, 7))
            payload = recv_exact(conn, size)
            recv_exact(conn, 1)
            if kind == 1:
                class_id, method_id = struct.unpack("!HH", payload[:4])
                return class_id, method_id, payload[4:]

    def close_with(self, conn, code, text):
        conn.sendall(self.frame(10, 50, "BsBB", (code, text, 0, 0)))

    def handle(self, conn):
        recv_exact(conn, 8)  # protocol header
        if self.hang:
            time.sleep(60)
            return
        conn.sendall(self.frame(10, 10, "ooFSS", (0, 9, {}, "AMQPLAIN PLAIN", "en_US")))
        _, _, args = self.read_method(conn)
        (_props, mechanism, response, _locale), _ = loads("FsSs", args, 0)
        if isinstance(response, str):
            response = response.encode("latin-1")
        if mechanism == "AMQPLAIN":
            (table,), _ = loads("F", struct.pack("!I", len(response)) + response, 0)
            user, password = table["LOGIN"], table["PASSWORD"]
        else:
            _, user, password = response.decode().split("\0")
        self.seen.append((user, password))
        if self.users.get(user) != password:
            return self.close_with(conn, 403, "ACCESS_REFUSED - Login was refused using authentication mechanism " + mechanism)
        conn.sendall(self.frame(10, 30, "BlB", (2047, 131072, 0)))
        self.read_method(conn)  # tune-ok
        _, _, args = self.read_method(conn)
        (vhost, _caps, _insist), _ = loads("ssb", args, 0)
        if vhost not in self.permissions.get(user, set()):
            return self.close_with(conn, 530, f"NOT_ALLOWED - access to vhost '{vhost}' refused for user '{user}'")
        conn.sendall(self.frame(10, 41, "s", ("",)))
        while True:
            class_id, method_id, _ = self.read_method(conn)
            if (class_id, method_id) == (10, 50):
                conn.sendall(self.frame(10, 51, "", ()))
                return


def closed_port():
    sock = listener()
    port = sock.getsockname()[1]
    sock.close()
    return port


class ProbeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.aliases = os.path.join(cls.tmp.name, "hosts")
        with open(cls.aliases, "w") as handle:
            handle.write("pi-dash-db localhost\npi-dash-mq localhost\n")
        cls.source = probe_source()

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def run_probe(self, env, services="db mq", seconds="2"):
        full = {
            "PATH": os.environ.get("PATH", ""),
            "HOSTALIASES": self.aliases,
            "CREDENTIAL_CHECK_SERVICES": services,
            "CREDENTIAL_CHECK_SECONDS": seconds,
            "CREDENTIAL_CHECK_INTERVAL": "0.2",
            **env,
        }
        started = time.monotonic()
        result = subprocess.run(
            [sys.executable, "-"], input=self.source, env=full, capture_output=True, text=True, timeout=60
        )
        self.elapsed = time.monotonic() - started
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = [line.split("\t") for line in result.stdout.splitlines() if line.startswith("credential-check\t")]
        return {line[2]: (line[1], line[3]) for line in lines}, result.stdout + result.stderr

    # PostgreSQL

    def pg_url(self, server, auth="pi-dash:pi-dash", db="pi-dash"):
        return f"postgresql://{auth}@pi-dash-db:{server.port}/{db}"

    def test_postgres_accepts_matching_credentials(self):
        pg = FakePostgres({"pi-dash": "pi-dash"}, {"pi-dash"})
        results, out = self.run_probe({"DATABASE_URL": self.pg_url(pg)}, services="db")
        self.assertEqual(results["PostgreSQL"][0], "OK", out)

    def test_postgres_rejects_a_wrong_password_for_an_existing_role(self):
        pg = FakePostgres({"pi-dash": "custom"}, {"pi-dash"})
        results, out = self.run_probe({"DATABASE_URL": self.pg_url(pg)}, services="db")
        self.assertEqual(results["PostgreSQL"][0], "MISMATCH", out)
        self.assertIn("password authentication failed", results["PostgreSQL"][1])

    def test_postgres_rejects_a_volume_created_with_the_old_values(self):
        pg = FakePostgres({"pi dash": "pi dash"}, {"pi dash"})
        results, out = self.run_probe({"DATABASE_URL": self.pg_url(pg)}, services="db")
        self.assertEqual(results["PostgreSQL"][0], "MISMATCH", out)
        self.assertIn('role "pi-dash" does not exist', results["PostgreSQL"][1])

    def test_postgres_accepts_old_values_spelled_out_in_the_url(self):
        pg = FakePostgres({"pi dash": "pi dash"}, {"pi dash"})
        env = {"DATABASE_URL": self.pg_url(pg, "pi%20dash:pi%20dash", "pi%20dash")}
        results, out = self.run_probe(env, services="db")
        self.assertEqual(results["PostgreSQL"][0], "OK", out)

    def test_postgres_decodes_the_url_as_dj_database_url_does(self):
        # A literal "+" and a backslash stay as they are; %2B is "+".
        pg = FakePostgres({"pi-dash": "a+b\\c+"}, {"pi-dash"})
        results, out = self.run_probe({"DATABASE_URL": self.pg_url(pg, "pi-dash:a+b\\c%2B")}, services="db")
        self.assertEqual(results["PostgreSQL"][0], "OK", out)

    def test_postgres_missing_database(self):
        pg = FakePostgres({"pi-dash": "pi-dash"}, {"other"})
        results, out = self.run_probe({"DATABASE_URL": self.pg_url(pg)}, services="db")
        self.assertEqual(results["PostgreSQL"][0], "MISMATCH", out)

    def test_postgres_not_listening_is_not_ready_within_the_deadline(self):
        env = {"DATABASE_URL": f"postgresql://pi-dash:pi-dash@pi-dash-db:{closed_port()}/pi-dash"}
        results, out = self.run_probe(env, services="db")
        self.assertEqual(results["PostgreSQL"][0], "NOTREADY", out)
        self.assertLess(self.elapsed, 20)

    def test_postgres_no_pg_hba_entry_is_a_refusal(self):
        pg = FakePostgres({"pi-dash": "pi-dash"}, {"pi-dash"}, hba_reject=True)
        results, out = self.run_probe({"DATABASE_URL": self.pg_url(pg)}, services="db")
        self.assertEqual(results["PostgreSQL"][0], "MISMATCH", out)
        self.assertIn("no pg_hba.conf entry", results["PostgreSQL"][1])

    def test_postgres_refusal_matches_only_server_login_rejections(self):
        probe = {}
        exec(compile(self.source, "probe", "exec"), probe)
        refusal = probe["postgres_refusal"]
        prefix = 'connection failed: connection to server at "192.0.2.2", port 5432 failed: '
        for text in (
            prefix + 'FATAL:  password authentication failed for user "pi-dash"',
            prefix + 'FATAL:  role "pi-dash" does not exist',
            prefix + 'FATAL:  database "pi-dash" does not exist',
            prefix + 'FATAL:  no pg_hba.conf entry for host "192.0.2.5", user "pi-dash", database "pi-dash", no encryption',
            prefix + 'FATAL:  permission denied for database "pi-dash"\nDETAIL:  User does not have CONNECT privilege.',
        ):
            self.assertTrue(refusal(text), text)
        for text in (
            # Client-side TLS setup: not the volume's credentials.
            prefix + 'root certificate file "/certs/ca.pem" does not exist\nEither provide the file or change sslmode.',
            prefix + 'certificate present, but not private key file "/certs/client.key"',
            prefix + "Connection refused",
            prefix + "FATAL:  the database system is starting up",
        ):
            self.assertFalse(refusal(text), text)

    def test_a_crash_in_one_check_keeps_the_others_results(self):
        # PostgreSQL refuses, then the AMQP URL cannot even be parsed: the
        # PostgreSQL result must still reach the installer.
        pg = FakePostgres({"pi-dash": "custom"}, {"pi-dash"})
        env = {"DATABASE_URL": self.pg_url(pg), "AMQP_URL": "amqp://pi-dash:pi-dash@pi-dash-mq:notaport/pi-dash"}
        results, out = self.run_probe(env)
        self.assertEqual(results["PostgreSQL"][0], "MISMATCH", out)
        self.assertEqual(results["RabbitMQ"][0], "ERROR", out)

    def test_a_check_cut_off_by_the_hard_deadline_does_not_end_the_probe(self):
        # The alarm is the backstop for an attempt that hangs past every
        # client timeout. It must end that service's check only.
        script = (
            "import time\n"
            f"exec(compile({self.source!r}, 'probe', 'exec'))\n"
            "SECONDS, TIMEOUT = 1, 0\n"
            "retry('PostgreSQL', lambda: time.sleep(30))\n"
            "retry('RabbitMQ', lambda: ('OK', ''))\n"
        )
        result = subprocess.run(
            [sys.executable, "-c", script],
            env={"PATH": os.environ.get("PATH", "")},
            capture_output=True,
            text=True,
            timeout=20,
        )
        lines = [line.split("\t") for line in result.stdout.splitlines() if line.startswith("credential-check\t")]
        statuses = {line[2]: line[1] for line in lines}
        self.assertEqual(statuses, {"PostgreSQL": "NOTREADY", "RabbitMQ": "OK"}, result.stdout + result.stderr)

    def test_postgres_query_dbname_overrides_the_path_as_django_does(self):
        # Django: {"dbname": NAME, **OPTIONS}; USER/PASSWORD/HOST/PORT win
        # over OPTIONS.
        pg = FakePostgres({"pi-dash": "pi-dash"}, {"other"})
        url = self.pg_url(pg) + "?dbname=other&user=ignored&host=ignored.invalid"
        results, out = self.run_probe({"DATABASE_URL": url}, services="db")
        self.assertEqual(results["PostgreSQL"][0], "OK", out)
        self.assertIn(("pi-dash", "other"), pg.seen)

    def test_an_unparsable_url_is_reported_without_its_contents(self):
        # An unescaped "/" in the password makes its first part the "port":
        # the parser's message would print it.
        env = {
            "DATABASE_URL": "postgresql://pi-dash:topsecret/rest@pi-dash-db/pi-dash",
            "AMQP_URL": "amqp://pi-dash:mqsecret/rest@pi-dash-mq:5672/pi-dash",
        }
        results, out = self.run_probe(env)
        self.assertEqual(results["PostgreSQL"][0], "ERROR", out)
        self.assertEqual(results["RabbitMQ"][0], "ERROR", out)
        self.assertNotIn("topsecret", out)
        self.assertNotIn("mqsecret", out)
        self.assertIn("DATABASE_URL", results["PostgreSQL"][1])
        self.assertIn("AMQP_URL", results["RabbitMQ"][1])
        self.assertIn("percent-encode", results["PostgreSQL"][1])

    def test_postgres_with_no_host_says_so(self):
        env = {"DATABASE_URL": "", "POSTGRES_DB": "pi-dash", "POSTGRES_USER": "pi-dash", "POSTGRES_PASSWORD": "x"}
        results, out = self.run_probe(env, services="db")
        self.assertEqual(results["PostgreSQL"][0], "SKIP", out)
        self.assertIn("no host", results["PostgreSQL"][1])

    def test_the_hard_deadline_is_not_swallowed_by_a_broad_except(self):
        # The RabbitMQ attempt catches Exception to classify errors; the
        # deadline must still end the check.
        script = (
            "import time\n"
            f"exec(compile({self.source!r}, 'probe', 'exec'))\n"
            "SECONDS, TIMEOUT, INTERVAL = 1, 0, 0.1\n"
            "def attempt():\n"
            "    try:\n"
            "        time.sleep(30)\n"
            "    except Exception:\n"
            "        return ('RETRY', 'swallowed')\n"
            "retry('RabbitMQ', attempt)\n"
        )
        result = subprocess.run(
            [sys.executable, "-c", script], env={"PATH": os.environ.get("PATH", "")}, capture_output=True, text=True, timeout=20
        )
        self.assertIn("credential-check\tNOTREADY\tRabbitMQ\tno answer within 1s", result.stdout, result.stdout + result.stderr)

    def test_postgres_external_host_is_skipped(self):
        results, out = self.run_probe({"DATABASE_URL": "postgresql://app:secret@external-db/app"}, services="db")
        self.assertEqual(results["PostgreSQL"][0], "SKIP", out)

    # RabbitMQ

    def mq_url(self, server, auth="pi-dash:pi-dash", vhost="/pi-dash"):
        return f"amqp://{auth}@pi-dash-mq:{server.port}{vhost}"

    def test_rabbitmq_accepts_matching_credentials(self):
        mq = FakeRabbit({"pi-dash": "pi-dash"}, {"pi-dash": {"pi-dash"}})
        results, out = self.run_probe({"AMQP_URL": self.mq_url(mq)}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "OK", out)

    def test_rabbitmq_rejects_a_wrong_password(self):
        mq = FakeRabbit({"pi-dash": "custom"}, {"pi-dash": {"pi-dash"}})
        results, out = self.run_probe({"AMQP_URL": self.mq_url(mq)}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "MISMATCH", out)
        self.assertIn("ACCESS_REFUSED", results["RabbitMQ"][1])

    def test_rabbitmq_rejects_a_user_without_permissions_on_the_vhost(self):
        mq = FakeRabbit({"pi-dash": "pi-dash"}, {"pi-dash": {"/"}})
        results, out = self.run_probe({"AMQP_URL": self.mq_url(mq)}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "MISMATCH", out)
        self.assertIn("NOT_ALLOWED", results["RabbitMQ"][1])

    def test_rabbitmq_keeps_plus_backslash_and_surrounding_whitespace(self):
        password = " a+b\\c "
        mq = FakeRabbit({"pi-dash": password}, {"pi-dash": {"pi-dash"}})
        results, out = self.run_probe({"AMQP_URL": self.mq_url(mq, "pi-dash:%20a+b\\c%20")}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "OK", out)
        self.assertIn(("pi-dash", password), mq.seen)

    def test_rabbitmq_password_starting_with_a_dash(self):
        mq = FakeRabbit({"pi-dash": "-s3cret"}, {"pi-dash": {"pi-dash"}})
        results, out = self.run_probe({"AMQP_URL": self.mq_url(mq, "pi-dash:-s3cret")}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "OK", out)

    def test_rabbitmq_defaults_the_vhost_to_slash(self):
        mq = FakeRabbit({"pi-dash": "pi-dash"}, {"pi-dash": {"/"}})
        results, out = self.run_probe({"AMQP_URL": self.mq_url(mq, vhost="")}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "OK", out)

    def test_rabbitmq_defaults_a_missing_password_to_guest(self):
        mq = FakeRabbit({"pi-dash": "guest"}, {"pi-dash": {"pi-dash"}})
        results, out = self.run_probe({"AMQP_URL": self.mq_url(mq, "pi-dash")}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "OK", out)

    def test_rabbitmq_that_stops_answering_is_bounded(self):
        mq = FakeRabbit({}, {}, hang=True)
        results, out = self.run_probe({"AMQP_URL": self.mq_url(mq)}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "NOTREADY", out)
        self.assertLess(self.elapsed, 2 + 3 * 5 + 5)

    def test_rabbitmq_external_host_is_skipped(self):
        results, out = self.run_probe({"AMQP_URL": "amqp://app:secret@external-mq:5672/app"}, services="mq")
        self.assertEqual(results["RabbitMQ"][0], "SKIP", out)

    def test_only_the_requested_services_are_checked(self):
        results, out = self.run_probe({}, services="")
        self.assertEqual(results, {}, out)


if __name__ == "__main__":
    unittest.main()
