# Cloud scratch-stack Django settings (NEWFRONT-436).
#
# The cloud twin of parity_scratch_settings.py: imports the cloud local
# settings and widens ONLY the default anonymous throttle. The cloud
# settings inherit the production 30/minute anon budget unchanged, and
# every parity scenario shares one IP bucket with the frontend's own
# loader calls, so that budget saturates minutes into an oracle run
# (loader 429s serve route-error shells and fail scenarios that prove
# unrelated behavior). The authentication throttle (scope
# "authentication", 30/minute on email-check / magic / forgot) is
# untouched, so rate-limit oracles (AUTH-006) still trip it.
from pi_dash_cloud.settings.local import *  # noqa: F401,F403

REST_FRAMEWORK = {
    **REST_FRAMEWORK,
    "DEFAULT_THROTTLE_RATES": {
        **REST_FRAMEWORK["DEFAULT_THROTTLE_RATES"],
        "anon": "600/minute",
    },
}
