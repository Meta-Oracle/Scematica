# The Scematica Equations — Analysis

Companion to [`EQUATIONS.md`](./EQUATIONS.md), which carries the statements alone.
This document derives them, applies the consistency residual to a real fault, and reports
the measured constants behind every number.

Version 1.25.0 · constants measured 2026-08-06

---

## 1. Status of these relations

These relations are **definitional, not derived from first principles.** They assign
precise meaning to quantities the system already measures, and they are deterministic in
the strict sense: given the same inputs they return the same output, with no stochastic
term anywhere on a right-hand side.

Their value is not that they predict the market. It is that equations II and III together
over-determine $\mathrm{AI}$, which converts the third into a *constraint* the running
system either satisfies or violates. A violation is measurable, and its sign localises the
fault. Section 4 shows the constraint detecting a real defect in this codebase after
the fact.

Read them as an instrument, not as a law.

---

## 2. Equation I — The Edge

$$
\mathcal{E} \;=\; \mathbb{E}_{\Sigma}\Big[\, Y \cdot (b - L) \cdot \big(\mathrm{AI} \cdot \varepsilon\big) \Big]
$$

The term $(b - L)$ is net realised PnL and is measured directly:

$$
b - L \;=\; 2.675468 - 0.411967 \;=\; 2.263501 \ \text{SOL}
$$

$Y$ is the capture coefficient — the fraction of available edge the executor actually
realises. It is bounded above by fill rate, so an RPC layer that times out on one buy in
two caps $Y \le 0.5$ regardless of how good selection becomes.

**Selection quality and execution quality multiply; they do not add.** This is the
algebraic reason infrastructure repair precedes selection tuning: no achievable
improvement in the bracketed term compensates for a halved $Y$ in front of it.

---

## 3. Equation II — Agent Capability

$$
\mathrm{AI} \;=\; \mathbb{E}_{\Sigma}\big[\,r\,\big] \cdot \varepsilon \cdot \frac{N}{N_0}
$$

Capability is the product of what the agent expects to earn, how much it still explores,
and how much experience it has accumulated.

The $\varepsilon$ factor is not an error. Capability here is *learning capacity*, not
*immediate performance* — an agent at $\varepsilon = 0$ has stopped acquiring information
about the environment, whatever its current returns. Immediate performance carries the
opposite dependence on $\varepsilon$ and is treated in Section 6, where the two meet and
produce an interior optimum.

---

## 4. Equation III and the consistency constraint

### 4.1 The independent definition of $I$

$$
I \;\equiv\; \frac{\operatorname{Var}_{p \in \Sigma}\big[\,Q^{*}(p)\,\big]}{\big(\mathbb{E}_{p \in \Sigma}\big[\,Q^{*}(p)\,\big]\big)^{2}}
$$

This definition carries the weight of the framework. A model returning the same value for
every input has $\operatorname{Var}[Q^{*}] = 0$ and therefore $I = 0$, **irrespective of
how large or how confident that value is.** Magnitude is not information.

### 4.2 Derivation of the constraint

Equations II and III are not independent. Substituting II into III:

$$
\big(\mathbb{E}_{\Sigma}\,\varepsilon\,\nu\big)^{2} \;=\; I \, q \, \varepsilon
$$

$$
\mathbb{E}_{\Sigma}^{2}\,\varepsilon^{2}\,\nu^{2} \;=\; I \, q \, \varepsilon
$$

Dividing through by $\varepsilon$ and solving:

$$
I \;=\; \frac{\mathbb{E}_{\Sigma}^{2}\,\varepsilon\,\nu^{2}}{q}
$$

$I$ is therefore **predicted** by the other measurables. But §4.1 defines $I$
**independently**, as a measured variance. The system is consistent only where the two
agree, giving the residual

$$
\mathcal{R} \;=\; \underbrace{\frac{\operatorname{Var}_p[Q^{*}]}{\mathbb{E}_p[Q^{*}]^{2}}}_{\text{measured}} \;-\; \underbrace{\frac{\mathbb{E}_{\Sigma}^{2}\,\varepsilon\,\nu^{2}}{q}}_{\text{predicted}}
$$

### 4.3 The constraint detecting a real fault

On 2026-08-05 the Deep Q\* agent returned `SELL_PARTIAL` for 25 consecutive pools with
$Q^{*} \approx 26.5$ and negligible spread across inputs. Then:

$$
\operatorname{Var}_p[Q^{*}] \to 0 \quad\Longrightarrow\quad I_{\text{measured}} \to 0
$$

while the predicted branch, with $\varepsilon = 0.06794$, $\nu = 2.05$ and
$\mathbb{E}_\Sigma > 0$, remains strictly positive:

$$
I_{\text{predicted}} \;=\; \frac{\mathbb{E}_{\Sigma}^{2}\,(0.06794)\,(2.05)^{2}}{26.5} \;>\; 0
$$

$$
\Longrightarrow\qquad \mathcal{R} \;<\; 0
$$

The residual is strictly negative, and the sign is diagnostic: **the agent held high value
while carrying no information.**

This is the practical payoff. The veto guard deployed in the codebase tested whether
$Q^{*}$ exceeded the best buy alternative by a relative margin — that is, it tested the
*magnitude* branch only, which is the predicted term. A collapsed policy satisfies a
magnitude test trivially, because collapse produces a large and stable gap across all
inputs. Only the measured branch, $\operatorname{Var}_p[Q^{*}]$, distinguishes conviction
from information. The constraint fails precisely where the guard could not see.

---

## 5. Sign conventions for the residual

| Condition | Interpretation |
|---|---|
| $\mathcal{R} = 0$ | On-manifold; components mutually consistent |
| $\mathcal{R} < 0$ | Measured dispersion below prediction — model confident but uninformative (collapse) |
| $\mathcal{R} > 0$ | Measured dispersion above prediction — value estimates unstable relative to accumulated experience |

---

## 6. Derivation of the exploration sweet spot

Under $\varepsilon$-greedy action selection, immediate expected value is

$$
V(\varepsilon) \;=\; (1 - \varepsilon)\,Q^{*} \;+\; \varepsilon\,\bar{Q}_{\text{rand}}
$$

Writing $\Delta Q = Q^{*} - \bar{Q}_{\text{rand}} > 0$ for the value the policy adds over
random action, this is $V = Q^{*} - \varepsilon\,\Delta Q$. Combining with Equation I,
where the $\varepsilon^2$ arises from the explicit $\varepsilon$ in I and the $\varepsilon$
carried inside $\mathrm{AI}$:

$$
\mathcal{E}(\varepsilon) \;=\; Y\,(b - L)\,\nu\,\mathbb{E}_{\Sigma}\;\varepsilon^{2}\big(Q^{*} - \varepsilon\,\Delta Q\big)
$$

Differentiating the $\varepsilon$-dependent factor:

$$
\frac{\partial}{\partial \varepsilon}\Big(\varepsilon^{2}Q^{*} - \varepsilon^{3}\Delta Q\Big) \;=\; 2\varepsilon Q^{*} - 3\varepsilon^{2}\,\Delta Q
$$

Setting to zero and discarding the trivial root $\varepsilon = 0$:

$$
\varepsilon^{*} \;=\; \frac{2}{3}\cdot\frac{Q^{*}}{\Delta Q}
$$

**Exploration is set by how much the policy beats random, not by a schedule.** The interior
optimum exists only while $\Delta Q > \tfrac{2}{3}Q^{*}$; below that the objective is
monotone over the feasible range and $\varepsilon$ takes its floor.

A collapsed policy has $\Delta Q \to 0$, which sends $\varepsilon^{*} \to \infty$ — the
formal statement that a non-discriminating agent should be exploring, not acting. This
agrees with the $\mathcal{R} < 0$ diagnosis in §4.3 and is an independent route to the
same conclusion.

---

## 7. Derivation of the acceptance sweet spot

Total edge over a window of $n$ candidates at acceptance rate $\alpha$:

$$
\mathcal{E}_{\text{total}}(\alpha) \;=\; n \cdot \alpha \cdot \mathbb{E}\big[\,\text{PnL} \mid \text{admitted at }\alpha\,\big] \cdot Y
$$

Admitting in descending score order makes $\mathbb{E}[\text{PnL} \mid \alpha]$
monotonically non-increasing in $\alpha$: each widening admits a candidate no better than
the last. The product of an increasing term and a non-increasing one has an interior
maximum, located by

$$
\frac{\partial}{\partial \alpha}\Big(\alpha\,\mathbb{E}[\text{PnL} \mid \alpha]\Big) = 0
\quad\Longleftrightarrow\quad
\mathbb{E}\big[\text{PnL} \mid \text{marginal candidate}\big] = 0
$$

**Admit exactly down to the pool whose expected PnL is zero.** Above that threshold edge is
left unclaimed; below it, fees are paid to lose money.

Note this is a statement about the *marginal* candidate, not the average. A population
whose mean admitted PnL is strongly positive is under-admitting, not performing well.

---

## 8. Measured constants

From 639 closed positions:

| Quantity | Expression | Value |
|---|---|---|
| Win rate | $W = 198/639$ | $0.30986$ |
| Average win | $b / 198$ | $0.013512$ SOL |
| Average loss | $L / 441$ | $0.000934$ SOL |
| Payoff ratio | $R = \bar{w}/\bar{\ell}$ | $14.4647$ |
| Profit factor | $b / L$ | $6.4944$ |
| Expectancy | $W\bar{w} - (1-W)\bar{\ell}$ | $+0.003542$ SOL |

Expectancy cross-checks against the aggregate exactly, which validates the decomposition:

$$
\frac{b - L}{n_{\text{closed}}} \;=\; \frac{2.263501}{639} \;=\; 0.003542 \ \text{SOL}
$$

### 8.1 Kelly bound

$$
f^{*} \;=\; W - \frac{1 - W}{R} \;=\; 0.30986 - \frac{0.69014}{14.4647} \;=\; 0.26215
$$

Full Kelly is **26.2%** of bankroll per position. The edge survives a 31% win rate only
because $R = 14.46$ — **the payoff ratio, not the hit rate, carries this strategy.** Any
change that compresses the right tail destroys the edge even if it raises the win rate,
which is the quantitative form of the standing warning against curtailing the momentum
escalation ladder during loss periods.

---

## 9. Operational consequences

1. **$Y$ multiplies everything.** Execution failure caps total edge no matter how good
   selection becomes. Repair the RPC path before tuning selection; the ordering is forced
   by the algebra, not by preference.

2. **$I$ must be measured, not inferred.** $\mathcal{R} < 0$ is the signature of a
   confident, uninformative model, and it is invisible to any guard testing magnitude
   alone. Instrument $\operatorname{Var}_p[Q^{*}]$ directly and alert on it.

3. **$\varepsilon^{*}$ is a ratio, not a schedule.** Exploration should track $Q^{*}/\Delta Q$.
   A collapsed policy should not be trading at all, and both §4.3 and §6 reach that
   conclusion independently.

4. **Acceptance is bounded by the marginal candidate, not the mean.** A high average
   admitted PnL indicates under-admission.

---

## 10. Limitations

These relations are definitional. They do not forecast returns, and none of the constants
in §8 constitutes a prediction — they are historical summaries of 639 closed positions
under configurations that have since changed.

The residual $\mathcal{R}$ is a consistency check on the system's own reporting, not on
the market. $\mathcal{R} = 0$ means the components agree with each other; it does not mean
they are right.

$\mathbb{E}_\Sigma$ is treated as a scalar throughout. Where the population is strongly
heterogeneous, the substitution in §4.2 requires the expectation to commute with the
squaring operation, which holds exactly only for a degenerate distribution. Treat the
derived $I$ as a first-order approximation over populations with wide score dispersion.

---

## Rendering notes

Equations use `$…$` inline and `$$…$$` display delimiters — GitHub Flavored Markdown math
syntax, also native to Obsidian, VS Code preview, Jupyter, and MkDocs with
`pymdownx.arithmatex`. Any KaTeX or MathJax renderer will display these.
