# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Makes _harness importable when running `pytest <domain>` from this dir."""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
