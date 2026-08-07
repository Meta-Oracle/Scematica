Scematica Profitability: What the Live Data Actually Says

An article-style analysis of 685 completed trades, 2026-05-17 to 2026-05-26. Every figure here comes from the project's own trade log. Reproduce it any time with python tools/deep_analysis.py scematica-trades.jsonl. The operating playbook that follows from this analysis lives in Ideal-Scema-Trading.txt at the repository root.

The one-sentence version

Over six hundred and eighty-five completed round-trip trades, Scematica returned a net realized profit of just over 2.26 SOL while winning fewer than three trades in ten, which is only possible because the average winner is more than fourteen times the size of the average loser. That single fact, a low hit rate paired with an enormous payoff ratio, is the whole story of the bot's profitability, and every operating decision follows from it.

The headline numbers

The bot completed 685 round trips in the window. It won 198 of them, lost 440, and came out flat on 47. That is a win rate of 28.9 percent. A newcomer sees a sub-thirty-percent win rate and assumes the strategy loses money. It does the opposite. The net realized result was positive 2.2636 SOL. The reason is the shape of the wins and losses. The average winning trade made about 13.5 thousandths of a SOL. The average losing trade cost about 0.94 thousandths of a SOL. The winner is roughly 14.4 times the loser. Divide the total won by the total lost and you get the profit factor: 6.50. In plain terms, for every one SOL the bot gave back over this window, it earned six and a half. That is an excellent ratio by any trading standard, and it is not an accident of one lucky trade. It held across hundreds.

Why the profit is so concentrated, and why that is the point

If you sort every trade from best to worst and add up only the top ten percent, you have already captured seventy-eight percent of all the profit the bot made. The single best thirty-three trades account for nearly half of it. Almost all of the money comes from a thin tail of outsized winners, while the great majority of trades are small losses or break-evens that, individually, barely register.

This concentration is not a flaw to be engineered away. It is the signature of the edge. The bot is a lottery-ticket machine with positive expected value: most tickets are near misses that cost a little, and a few are large payouts that more than cover all the misses. The single most dangerous thing an operator can do is react to the long run of small losses by tightening the exit to raise the win rate, because that cuts the winners short and throws away the very tail that produces the profit. The correct posture is the opposite of intuitive: accept the small losses without flinching, and protect the rare big winner at all costs.

What size to trade, in the data's own words

The log records the entry size of every trade, so we can ask directly which sizes make money. The answer is unambiguous. Entries below about four thousandths of a SOL are net negative: across 148 such trades they lost money in aggregate, because fees and the automated-market-maker spread eat a position that small alive. Just above that, the band from roughly seven and a half to fifteen thousandths of a SOL is the profit core of the entire account, responsible on its own for the majority of net gains across two hundred and twenty-seven trades. Larger entries, up to the biggest sizes tested, remained positive, which suggests the edge scales rather than saturating. The practical lesson drove a real configuration change: the smallest rate mode's entry floor was raised so the bot never again trades dust it cannot profit on, and Balanced mode, which places entries right in the profitable core, is the recommended default.

When to hold and when to bail

Sorting by how long each position was held is even more revealing. The first five seconds after entry contain the majority of all profit the bot ever made. Speed, in other words, is not a nice-to-have; it is where the money is. Detection latency and execution latency in those first seconds matter more than any filter tweak, which is why running the bot against a fast, paid node is non-negotiable.

At the other end, there is a graveyard. Positions held between roughly forty-five seconds and two minutes win only about seven percent of the time. These are the tokens that pumped for a moment, stalled, and then bled slowly toward the floor. The velocity-decay and peak-stagnation exits exist specifically to kill these before they rot, and the no-pump-timeout is kept short so capital recycles out of a dead position quickly. Then, far out on the time axis, the rare position held beyond several minutes wins three quarters of the time, because a token that is still climbing after minutes is a genuine runner. That is the tail the escalation logic is designed to ride.

When to trade

The data has a clear opinion about the clock and the calendar. Broken down by hour of the day in coordinated universal time, only two hours across the entire dataset were net negative, and the single most profitable hour of all was one that an earlier, misguided configuration had actually placed on the block list, a mistake that was costing real money until it was corrected using the full data rather than a small recent sample. Broken down by day of the week, Monday and Tuesday together produced almost the entire profit of the run, Friday was roughly break-even, and Sunday was the only losing day of the week. This is precisely why the bot carries a defensive weekend mode that sizes down on Saturday and Sunday and restores the aggressive weekday configuration afterward, and why the honest advice is to run the bot hardest early in the week.

How the losses actually happen

Understanding where the losses come from is what makes the low win rate tolerable. The overwhelming majority of losing trades are not disasters; they are tiny. A position enters a pool that never moves, sits at the automated-market-maker spread, and exits a fraction of a percent down when the no-pump timeout fires. Each of these costs a sliver, and there are many of them, but together they add up to only a small fraction of what the winners bring in. A much smaller number of losses are genuine rug pulls, where the pool's liquidity is pulled before the sell monitor can exit; these are larger but rare, and the deployer-reputation filter and pool scorer exist to reduce how often the bot walks into one. The key insight is that the loss distribution is dominated by cheap, survivable misses rather than account-ending blowups, which is exactly the distribution a positive-expectancy lottery strategy needs.

How much capital it takes

Because the log records every buy and sell with a timestamp and an amount, the capital requirement can be computed rather than guessed. Replaying the trades to track how much SOL was deployed in open positions at any one moment shows that the bot never held more than about one tenth of a SOL in concurrent exposure. Tracking the running equity shows the worst peak-to-trough dip was about one sixth of a SOL. From those two hard facts the bankroll tiers follow directly. Half a SOL is the practical floor, covering the worst observed drawdown roughly three times over plus fees. Seven tenths to one SOL is the recommended amount, giving the percentage-based position sizer room to place the entries in the profitable core while comfortably surviving a bad day. Two to three SOL is the comfortable scaling tier, where the same percentage rule automatically places proportionally larger entries, so the edge compounds with the balance rather than requiring any re-tuning. The token gate's requirement of a quarter-million SCEMA is separate from all of this; it is an access requirement, not trading capital.

The honest caveat

None of this is a promise about the future, and it would be dishonest to present it as one. This is a realized result over one particular nine-and-a-half-day window. Two variables that matter enormously when scaling, the slippage incurred on larger live entries and the competition from other bots racing for the same pools, are exactly the things a historical log cannot fully capture. The claim the data supports, and it is a strong claim, is that Scematica has a real, repeatable, statistically significant edge that showed up clearly across hundreds of trades, not a handful. The way to carry that edge forward is discipline rather than novelty: fund the wallet appropriately, size into the profitable band, run during the productive hours and days on a fast connection, keep the filters moderate so the bot catches tokens early rather than at their peak, and above all take the small losses without interference so the rare large winner is free to run.

Bottom line

The profit factor of 6.50 is the number to remember. It means the strategy is not fragile; it has a wide margin between what it wins and what it loses. The path from here to running the bot at its full potential is not a search for a clever new feature. It is the discipline to run it the way its own history says works, which is described in full in Ideal-Scema-Trading.txt.
