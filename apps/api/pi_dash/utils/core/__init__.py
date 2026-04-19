# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""
Core utilities for Pi Dash database routing and request scoping.
This package contains essential components for managing read replica routing
and request-scoped context in the Pi Dash application.
"""

from .dbrouters import ReadReplicaRouter
from .mixins import ReadReplicaControlMixin
from .request_scope import (
    set_use_read_replica,
    should_use_read_replica,
    clear_read_replica_context,
)

__all__ = [
    "ReadReplicaRouter",
    "ReadReplicaControlMixin",
    "set_use_read_replica",
    "should_use_read_replica",
    "clear_read_replica_context",
]
