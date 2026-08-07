"""Kept for import compatibility.

``build_package_blueprint`` now lives in :mod:`alchem_link.integration`, next to the map
it embeds. This re-export means ``from alchem_link.core import build_package_blueprint``
keeps working for anyone who wrote it that way.
"""
from __future__ import annotations

from .integration import build_integration_map, build_package_blueprint

__all__ = ["build_package_blueprint", "build_integration_map"]
