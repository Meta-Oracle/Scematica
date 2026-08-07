"""Kept for import compatibility.

``summarize_alchemy_capabilities`` used to return a hardcoded dict of prose. It now
probes the endpoint you are actually pointed at and reports which Enhanced APIs that key
can reach, so it lives in :mod:`alchem_link.enhanced` beside the calls it describes.
"""
from __future__ import annotations

from .enhanced import summarize_alchemy_capabilities

__all__ = ["summarize_alchemy_capabilities"]
