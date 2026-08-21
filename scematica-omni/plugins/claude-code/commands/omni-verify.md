---
description: Re-check a sealed decision record's commitment, and say precisely what that proves
argument-hint: "[record-id]"
allowed-tools: mcp__scema__omni_records, mcp__scema__omni_verify, mcp__scema__omni_explain
---

Verify decision record `$1`. With no id, call `omni_records` and verify every one.

Report per record: `VALID` or `INVALID`, and for an invalid one **name every field that
moved**, not just the first — a report that stops at the first mismatch makes a two-field
edit look like a one-field edit. If `root_only` is set, say so explicitly: every part
verifies and the root does not, which is the signature of a hand edit.

Then state the limits, every time, in your own words:

- It proves **the record was not edited after it was sealed**.
- It does **not** prove the world was as described. Provenance carries that, not the digest.
- It does **not** prove this is the original record. Tamper-**evident**, not tamper-proof,
  until the root is anchored somewhere the author does not control.

A verifier whose reader over-trusts it is worse than no verifier, so do not compress those
three lines into "the record is valid".

If a record could not be **read** at all, that is a third state. Say "unreadable", not
"invalid" — one is a gap and the other is an accusation.
