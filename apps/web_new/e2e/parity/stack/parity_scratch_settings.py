# Scratch-stack Django settings (NEWFRONT-108).
#
# Imports the stock local settings and widens ONLY the default anonymous
# throttle. Every parity scenario shares one IP bucket with the frontend's
# own loader calls, so the production 30/minute anon budget saturates
# minutes into an oracle run (loader 429s serve route-error shells and fail
# scenarios that prove unrelated behavior). The authentication throttle
# (scope "authentication", 30/minute on email-check / magic / forgot) is
# untouched, so rate-limit oracles (AUTH-006) still trip it.
from pi_dash.settings.local import *  # noqa: F401,F403

REST_FRAMEWORK = {
    **REST_FRAMEWORK,
    "DEFAULT_THROTTLE_RATES": {
        **REST_FRAMEWORK["DEFAULT_THROTTLE_RATES"],
        "anon": "600/minute",
    },
}
