---
description: Perceive this project as a world state — counted signals, and what could not be read
argument-hint: "[path]"
allowed-tools: mcp__scema__omni_observe
---

Call `omni_observe` on `$1` (default `.`) and report what came back.

Report it in this order, because the order is the point:

1. **What could not be read.** `blind_spots` first, before any number. An observer that
   hit a permission error knows something the reader needs before they read anything else.
2. **Extent.** If `extent.total` is `null`, say **the walk was capped and the observer does
   not know how much it missed**. Do not present the observed count as a total.
3. **The counted signals**, with their ids. The ids are what `--ground` takes, and they are
   the only thing that can ground a branch, so list them verbatim rather than paraphrasing.
4. **Legibility**, with the caveat that an empty world scores `0.0` because there was
   nothing to read — not because everything was unreadable.

Do not summarise the signals into a health score, a grade, or a percentage of your own.
Every magnitude here was counted from something specific; a number you compute on top of
them is not, and it will be indistinguishable from the ones that were.
