# The Scematica Equations

**Statements only.** Derivations, the collapse case study, and the measured constants live
in [`EQUATIONS-ANALYSIS.md`](./EQUATIONS-ANALYSIS.md).

Version 1.15.0 · constants measured 2026-08-06

---

## Symbols

| Symbol | Name | Definition |
|---|---|---|
| $\mathcal{E}$ | Edge | Expected system edge over the evaluated population |
| $\mathrm{AI}$ | Agent capability | Learning capacity of the Deep Q\* agent |
| $\varepsilon$ | Exploration rate | DQN epsilon-greedy parameter |
| $N$ | Episodes | Completed training episodes |
| $N_0$ | Reference episodes | Normalisation constant, $10{,}000$ |
| $\nu$ | Normalised episodes | $\nu = N / N_0$ |
| $b$ | Gross benefit | Sum of winning trade PnL |
| $L$ | Gross loss | Absolute sum of losing trade PnL |
| $Y$ | Yield coefficient | Realised fraction of theoretical edge, $Y \in [0,1]$ |
| $Q^{*}$ | Optimal action value | $\max_a Q(s,a)$ over the population |
| $Q_0$ | Reference value | Normalisation constant, $1$ SOL-equivalent |
| $q$ | Normalised value | $q = Q^{*} / Q_0$ |
| $\Delta Q$ | Policy advantage | $Q^{*} - \bar{Q}_{\text{rand}}$ |
| $I$ | Intelligence ratio | Normalised dispersion of $Q^{*}$ across inputs |
| $\mathcal{R}$ | Residual | Measured $I$ minus predicted $I$ |
| $\alpha$ | Acceptance rate | Candidates admitted / candidates seen |
| $W$ | Win rate | Winning positions / closed positions |
| $R$ | Payoff ratio | Average win / average loss |
| $f^{*}$ | Kelly fraction | Optimal bankroll fraction per position |
| $\Sigma$ | Population | The set of evaluated pools |
| $\mathbb{E}_\Sigma$ | Population expectation | Mean over $\Sigma$ |

All quantities are dimensionless under the normalisations $\nu = N/N_0$ and $q = Q^{*}/Q_0$.

---

## I. The Edge

$$
\mathcal{E} \;=\; \mathbb{E}_{\Sigma}\Big[\, Y \cdot (b - L) \cdot \big(\mathrm{AI} \cdot \varepsilon\big) \Big]
$$

Expanded over $n$ pools:

$$
\mathcal{E} \;=\; \frac{1}{n}\sum_{p \in \Sigma} Y_p \,\big(b_p - L_p\big)\,\mathrm{AI}\,\varepsilon
$$

---

## II. Agent Capability

$$
\mathrm{AI} \;=\; \mathbb{E}_{\Sigma} \cdot \varepsilon \cdot \nu
\qquad\qquad
\nu = \frac{N}{N_0}
$$

---

## III. The Value Identity

$$
\mathrm{AI}^{2} \;=\; I \cdot q \cdot \varepsilon
\qquad\qquad
q = \frac{Q^{*}}{Q_0}
$$

with the intelligence ratio defined independently as

$$
I \;\equiv\; \frac{\operatorname{Var}_{p \in \Sigma}\big[\,Q^{*}(p)\,\big]}{\big(\mathbb{E}_{p \in \Sigma}\big[\,Q^{*}(p)\,\big]\big)^{2}}
$$

---

## IV. Derived — Intelligence Ratio

From II and III:

$$
I \;=\; \frac{\mathbb{E}_{\Sigma}^{2}\,\varepsilon\,\nu^{2}}{q}
$$

---

## V. Derived — Consistency Residual

$$
\mathcal{R} \;=\; \frac{\operatorname{Var}_p[Q^{*}]}{\mathbb{E}_p[Q^{*}]^{2}} \;-\; \frac{\mathbb{E}_{\Sigma}^{2}\,\varepsilon\,\nu^{2}}{q}
$$

$$
\mathcal{R} = 0 \;\Longrightarrow\; \text{consistent}
\qquad
\mathcal{R} \neq 0 \;\Longrightarrow\; \text{a component is misreporting}
$$

---

## VI. Sweet Spot — Exploration

$$
\varepsilon^{*} \;=\; \frac{2}{3}\cdot\frac{Q^{*}}{\Delta Q}
$$

Valid for $\Delta Q > \tfrac{2}{3}Q^{*}$; otherwise the objective is monotone and
$\varepsilon$ takes its floor value.

---

## VII. Sweet Spot — Acceptance

$$
\frac{\partial}{\partial \alpha}\Big(\alpha\,\mathbb{E}[\text{PnL} \mid \alpha]\Big) = 0
\qquad\Longleftrightarrow\qquad
\mathbb{E}\big[\text{PnL} \mid \text{marginal candidate}\big] = 0
$$

---

## VIII. Kelly Bound

$$
f^{*} \;=\; W - \frac{1 - W}{R}
$$

---

## Measured Constants

| Quantity | Value |
|---|---|
| $\varepsilon$ | $0.06794$ |
| $N$ | $20{,}500$ |
| $b$ | $2.675468$ SOL |
| $L$ | $0.411967$ SOL |
| $b - L$ | $2.263501$ SOL |
| $Q^{*}$ | $\approx 26.5$ |
| $\alpha$ | $0.16038$ |
| $W$ | $0.30986$ |
| $R$ | $14.4647$ |
| $f^{*}$ | $0.26215$ |

---

## Rendering notes

Equations use `$…$` inline and `$$…$$` display delimiters — GitHub Flavored Markdown math
syntax, also native to Obsidian, VS Code preview, Jupyter, and MkDocs with
`pymdownx.arithmatex`. Any KaTeX or MathJax renderer will display these.
