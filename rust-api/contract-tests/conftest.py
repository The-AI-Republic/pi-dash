"""Root conftest: make ``_harness`` importable for every domain suite."""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
