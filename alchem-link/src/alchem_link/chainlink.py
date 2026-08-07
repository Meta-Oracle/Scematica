"""Kept for import compatibility.

``summarize_chainlink_capabilities`` now reports which Chainlink services this toolkit
reads *live* versus merely knows about, so it lives in :mod:`alchem_link.ccip` beside the
router and selector tables it counts.
"""
from __future__ import annotations

from .ccip import summarize_chainlink_capabilities

__all__ = ["summarize_chainlink_capabilities"]
