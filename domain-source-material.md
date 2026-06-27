# Specialty Insurance Market — Domain Source Material

*Unopinionated domain knowledge mined from the `rins` project docs (`market-mechanics.md`, `phenomena.md`, `calibration.md`). This is reference material for a ground-up redesign. It records what the domain **is** — institutional facts, real-world anchors, target phenomena, and the points where a model must make a choice. It deliberately omits `rins`-specific implementation, code locations, and the project's own design critiques. Where a number comes from the real world it is marked as an anchor; where it was a `rins` modelling choice it is flagged as such.*

---

## 1. Domain glossary (ubiquitous language)

**Lloyd's of London** — a subscription insurance market (not a single insurer). Specialist **syndicates** compete to cover risks brought to them by **brokers** on behalf of clients. Risks are placed on a **subscription** basis: a lead sets terms, followers subscribe.

**Insured / Asset owner** — the party seeking coverage. Owns one or more **assets** with economic value; requests coverage (typically annually); evaluates quotes against a private willingness-to-pay.

**Asset** — a unit of economic value owned by an insured. Characterised by: `sum_insured` (total replacement value; ceiling on any single-occurrence physical loss), `territory` (geographic/peril zone), and the set of perils it is exposed to.

**Sum insured (SI)** — total replacement value of an asset; the maximum physical loss a single occurrence can inflict on it.

**Total Insured Value (TIV)** — sum of `sum_insured` across all insureds; the market's total exposure base.

**Peril** — a hazard category. Two structurally different classes:
- **Attritional** — many small, statistically *independent* occurrences (minor fire, water damage, equipment failure). Predictable in aggregate; aggregate variance compresses as the pool grows.
- **Catastrophe (cat)** — rare, large occurrences (hurricane, earthquake, flood). A single physical event strikes *all* exposed assets in a territory simultaneously → losses are *correlated by construction*. Dominant source of year-to-year capital volatility.

**Occurrence** — a single physical event of a peril, with an intensity expressed as a **damage fraction** ∈ [0, 1].

**Ground-up loss (GUL)** — physical damage independent of any insurance: `GUL = damage_fraction × sum_insured`. Hard invariant: `GUL ≤ sum_insured`.

**Policy** — an insurance contract covering a defined tranche (**layer**) of ground-up loss for a period (Lloyd's standard: annual).

**Layer [attachment, attachment + limit]** — the band of loss a policy covers:
- `gross = min(GUL, limit)`
- `net (insured loss) = gross − attachment` (0 if `GUL ≤ attachment`)
- The insured retains losses below **attachment** (the deductible) and above **attachment + limit** (uncovered excess).

**Programme / tower** — a vertical stack of consecutive layers, each an independent contract with its own attachment, limit, premium, and panel. Used when required limit exceeds a single placement.

**Rate on line (ROL)** — `premium / limit`. Decreases as attachment height rises (fewer events penetrate higher layers).

**Quote** — a syndicate's offered premium for a risk. May be declined (e.g. exposure limits breached, insolvency).

**Panel** — the set of syndicates subscribing to a single risk, each taking a share (in basis points). Net insured loss is pro-rated across the panel.

**Lead / Follow** — in the subscription market, the **lead** syndicate sets terms and price (quotes blind); **followers** subscribe on those terms, observing the lead quote.

**Premium** — price paid for coverage. Layered as:
- **ATP (Actuarial Technical Price)** — actuarially required premium given expected loss and a target loss ratio.
- **TP (Technical Premium)** — ATP plus profit/cost-of-capital loading; the long-run actuarial floor.
- **AP (Actual Premium)** — market-clearing price; TP scaled by the cycle position.

**Loss ratio (LR)** — claims incurred / premiums earned.
**Expense ratio** — acquisition + management costs as a fraction of premium.
**Combined ratio (CR)** — `loss_ratio + expense_ratio`. < 100% = underwriting profit; > 100% = underwriting loss.

**Capital** — a syndicate's loss-absorbing funds. Premiums credit it; claims debit it. Governs maximum exposure. When exhausted → **insolvent**.

**Exposure limits** — capital-linked caps a syndicate places on what it will write:
- **Per-risk line size** — max retained exposure on any single risk.
- **Cat aggregate (per peril zone)** — max summed exposure across correlated risks, bounded by the requirement that an extreme-return-period loss stays coverable within capital.

**Burning cost** — realised losses / exposure; the empirical loss-cost signal used to update pricing.

**ELF (Expected Loss Fraction)** — expected loss as a fraction of sum insured. Split into an attritional component and a cat component.

**Insolvency / runoff** — when capital is exhausted, a syndicate stops writing new business but continues settling claims on in-force policies until they expire (**runoff**).

---

## 2. Real-world institutional facts (Lloyd's)

These are facts about how the actual market operates — the institutional backdrop any model is approximating.

### Market structure
- **Subscription market**: lead sets terms; followers subscribe. A risk is placed across a **panel** of syndicates, each taking a share.
- **Broker intermediation**: Lloyd's brokers place risks with syndicates. Relationships (built over years) drive routing; switching costs are real.
- **Coverholders / binding authorities**: ~45% of Lloyd's premium is written through coverholders (delegated authority), which carry higher acquisition costs (a ceding commission, a broker fee, and managing-agent overhead all levied on the same premium).

### Capital & solvency framework
- **Funds at Lloyd's (FAL)** — member capital lodged as security for underwriting. Minimum 40% of stamp capacity (max NPI).
- **SCR (Solvency Capital Requirement)** — set at the 99.5th percentile (1-in-200-year) of ultimate total claims (Solvency II).
- **ECA (Economic Capital Assessment)** = `SCR × 1.35` — a 35% buffer above the Solvency II minimum that Lloyd's applies.
- **Solvency ratio target** ≥ 140%; **Lloyd's actual 2024: 206%** (≈ 1.5× excess capital).
- **Central Fund** — a mutual fund financed by annual levies on active syndicates; pays claims an insolvent syndicate in runoff cannot meet. A welfare mechanism, not a cycle mechanism.

### Exposure-management rules (Lloyd's Franchise Guidelines, Market Bulletin Y5375, 2022)
- **Per-risk net line** ≤ 30% of ECA + profit. **Gross line** ≤ min(25% of GWP, £200M).
- **Cat aggregate** bounded so the 1-in-200 loss is coverable within ECA.
- **Tail constraint**: the 1-in-500 loss must not exceed 135% × the 1-in-200 loss (a distribution-shape constraint, preventing thin-tail/cliff-edge books).
- Constraints apply to **net** (post-reinsurance) figures; syndicates can write larger gross lines by ceding outward reinsurance.
- These are regulatory hard floors — exceeding them requires a dispensation.

### Pricing discipline (Lloyd's Minimum Standards MS3 — *Price and Rate Monitoring*, 2021; strengthened 2022)
- Syndicates must track **Actual-vs-Technical premium (AvT)** = AP / TP. AvT < 1.0 means pricing below technical premium and must be justified.
- The **Technical Premium must include modelled cat loading derived from a cat model, not from recent experience** (2022 hard-floor + retrospective-testing rules were added specifically to prevent experience-driven cat-rate erosion during benign periods).
- **Cost-of-capital loading** must be included in the technical price and grounded in actual capital allocation. Representative: ~5% for primary/working layers; up to ~30% for upper/cat-exposed excess layers (which consume far more risk capital per unit of limit).

### Accounting & timing
- **Annual contracts**: 12-month policies; the Lloyd's standard placement cycle. Premiums earned in one year do not carry forward.
- **3-year account**: an underwriting year is held open ~3 years to let IBNR (Incurred But Not Reported) claims emerge before being **reinsured-to-close (RITC)**.
- **Names distributions**: when a year closes, net profit is crystallised and distributable to Names (capital providers), who choose whether to recommit or withdraw. This prevents indefinite capital accumulation.
- **Solvency II distribution constraint**: distributions are prohibited if they would breach the SCR; an eroded syndicate must retain profit to rebuild before distributing.

### Renewal seasonality (quarter-day calendar)
Commercial policies cluster at four standard renewal dates:

| Inception | Approx. share | Primary drivers |
|---|---|---|
| 1 January | ~40% | Reinsurance, European corporate, international programmes |
| 1 April | ~20% | Japan, South Korea, Asia-Pacific, UK mid-market |
| 1 July | ~25% | US cat-exposed (Florida/SE wind), Australia, NZ, mid-year |
| 1 October | ~15% | Fiscal-year-driven accounts, US inland property, residual |

Concentration at 1 January gives that round outsized signalling power; rate moves propagate to later renewals.

### Brokerage / premium flow
Typical brokerage rates: XoL reinsurance ~15%; direct specialty 10–20%; facultative reinsurance 5–10%; large treaty reinsurance 1–5%; coverholder ceding commission 25–35%.

Premium flow: Insured pays gross premium → broker deducts brokerage → net enters the ring-fenced Premium Trust Fund → distributed to syndicates pro-rata → syndicate books full gross as GWP, brokerage appears as an acquisition cost → syndicate pays outward reinsurance premium.

**Net Earned Premium (NEP)** = GWP − outward reinsurance ceded, adjusted for unearned-premium movements; the denominator for Lloyd's ratio calculations.

---

## 3. The annual market lifecycle

A simulated year, at the domain level:

1. **Coverage request** — insureds request coverage for their assets (annually).
2. **Routing** — a broker routes each request to a shortlist of syndicates, favouring established relationships.
3. **Pricing & quoting** — each solicited syndicate prices the risk (its loss-cost estimate, capital position, and read on the market) and either quotes or declines (e.g. exposure limit breached, insolvent).
4. **Placement** — the broker presents the best/assembled quote(s); the insured accepts or rejects based on willingness to pay. If accepted, a **policy is bound** — coverage is live.
5. **Losses occur** — throughout the year: attritional (small, frequent, independent) and occasionally catastrophe (rare, large, correlated across a territory).
6. **Claims settle** — insurers pay; capital shrinks. An insurer that exhausts capital becomes insolvent and goes into runoff.
7. **Year-end learning & adjustment** — insurers update pricing models from experience; brokers adjust relationship scores; profits may be distributed; capital carries over; if the market is profitable, **new capacity enters**; policies expire and are re-underwritten at renewal.

Repeat over many years → emergent macro patterns (Section 5).

---

## 4. Domain mechanics

### 4.1 Risk transfer & layers
Insurance transfers a defined tranche of ground-up loss from insured to market. The three loss layers per occurrence:

| Layer | Meaning | Quantity |
|---|---|---|
| Asset value | Total economic value exposed | `sum_insured` |
| Ground-up loss (GUL) | Physical damage, insurance-independent | `damage_fraction × sum_insured` |
| Insured loss | Market's share after policy terms | `min(GUL, limit) − attachment` |

### 4.2 Attritional vs catastrophe (the central structural distinction)
- **Attritional**: independent draws per policy. Aggregate variance compresses as `1/√N` (Law of Large Numbers). Pooling works.
- **Catastrophe**: a *single shared occurrence* strikes all exposed assets in a territory at once. Adding more risks in the same territory does **not** reduce the CV of the cat component — the dominant variance is event-level severity, perfectly correlated across the exposed pool. Only diversification *across uncorrelated perils and territories* reduces cat variance.

This is why catastrophe-exposed books stay volatile regardless of size, and why cat loading in premium is structurally different from attritional loading.

### 4.3 Pricing
Actuarial loss-cost estimate blended with market signal:
- **ATP** = `E[loss] / target_loss_ratio` (built-in profit margin when target LR < 1).
- Loss cost splits into **attritional ELF** (updated from realised burning cost — high frequency, one year is informative) and **cat ELF** (anchored to a cat model, *not* updated from experience — a quiet decade is a benign sample, not evidence the hazard changed; experience-updating cat ELF causes systematic soft-market rate erosion, the dominant cycle failure mode).
- **Expense loading is multiplicative**, not additive: `gross = pure_premium / (1 − expense_ratio)`. (Additive loading systematically underprices, because each unit of loading must itself be covered.)
- **Credibility weighting**: a syndicate blends its own loss experience with the industry benchmark; weight on own experience rises with volume (low-volume syndicates lean on the benchmark; specialists trust their own data). The actuarially correct form (Bühlmann-Straub) makes credibility a function of information content (within- vs between-syndicate variance), not just years or volume.

### 4.4 Exposure management
Two capital-linked limits, recomputed from current capital at quote time:
- **Per-risk line**: `max_line ≈ line_capacity_fraction × capital`.
- **Cat aggregate**: `max_cat_aggregate = solvency_capital_fraction × capital / pml_damage_fraction_at_1in200`, where the 1-in-200 damage fraction is derived from the cat model.

Because both scale with capital, a post-cat capital drawdown tightens both **simultaneously** — the capacity crunch and the subsequent price hardening are two consequences of the same depletion, reinforcing each other.

### 4.5 Capital & solvency dynamics
- **Persistent capital**: premiums credit, claims debit, year-end balance carries over; no annual re-endowment. Capital floors at zero → insolvency.
- **Distributions**: profit released to capital providers prevents indefinite accumulation; suppressed in loss years and when capital is below a solvency floor (so an impaired syndicate rebuilds rather than distributing into a death spiral).
- **Entry**: elevated returns (AP/TP above a threshold) attract new capacity (new syndicates, ILS, sidecars). Real-world formation lag 12–18 months; new entrants then take 2–3 years to build broker relationships before reaching material panel share (the combined lag sustains hard markets). Capital supply has a **rising supply curve**: easiest capital deploys first; marginal capital needs progressively higher expected returns.
- **Exit**: full class exit is rare (heavy relational + regulatory cost). The real soft-market levers are *gradual*: reducing participation/line fractions, pricing discipline (rates above market, letting business route elsewhere), and terms tightening (higher deductibles, exclusions). Continuous line-fraction reduction, not binary exit, is the realistic withdrawal mechanism.

### 4.6 Loss settlement cascade
`Occurrence → ground-up loss → apply policy terms (attachment/limit) → claim settled → capital debited (insurer pays min(amount, capital); insolvency on first crossing zero)`. Panel claims are pro-rated by share; the sum of settled amounts equals the net insured loss (up to rounding).

Settlement invariants: `GUL ≤ sum_insured`; no claim below attachment; insured loss ≤ limit; settled amounts sum to insured loss; expired policies generate no claims.

### 4.7 Investment income (a cycle co-driver)
Syndicates earn returns on the Premium Trust Fund and FAL. High yields (e.g. 2022–24: 4–5%) let syndicates tolerate CRs above 100% on total return, softening the hard-market signal; near-zero yields (2010–21) force the full cost of capital from underwriting alone, making hard markets sharper and longer. Venezian (1985) and Cummins & Outreville (1987) identify the interest-rate channel as a co-driver of cycle *period* — the empirical 5–10-year cycle aligns with the interest-rate cycle, not purely with cat frequency. Lloyd's 2024 investment return ≈ 4.5% of the balance sheet.

### 4.8 Reserve development / IBNR (a lagged capital shock)
Under the 3-year account, reserves develop after the loss year:
- **Adverse development** (strengthening): additional capital charges 12–24 months post-event — a secondary, lagged shock that sustains hard markets beyond the triggering year (e.g. post-Katrina strengthening through 2007–08).
- **Favourable development** (release): in benign years, releasing excess reserves inflates reported profit and masks deteriorating underwriting quality; when releases exhaust, the true CR steps up abruptly — a hardening trigger with no new catastrophe (e.g. Lloyd's 1988–92; US P&C ~2001).

### 4.9 Reinstatement premiums
After a cat claim exhausts a layer, a **reinstatement premium** (typically 100% of the original layer premium, pro-rated) restores the limit for the rest of the year. Effects: (1) additional premium income in the same year as the loss, partly offsetting capital impact; (2) automatic within-year hardening — a second event costs an additional reinstatement, a non-linear penalty for cat frequency absent from flat annual premiums. Lloyd's quoted ROL includes the reinstatement cost.

### 4.10 Outward reinsurance
The primary tail-risk-management mechanism: a syndicate cedes a tranche of gross exposure for a reinsurance premium, reducing net retained loss in large cat years. Gross vs net is fundamental to capacity accounting (regulatory limits apply to net). Reinsurance is *simultaneously* stabilising (dampens individual-firm first-order cat losses) and destabilising (a reinsurer insolvency removes expected cession recoveries from multiple primaries at once — a correlated secondary shock).

Stances in the ABM literature on modelling reinsurance:
1. **Omit it** — rely on capital constraints + exposure limits as the sole capacity mechanism (a noted gap).
2. **Parametric net retention** — each insurer retains a fixed fraction of each gross loss; reinsurance is a balance-sheet multiplier, no reinsurer agents.
3. **Reinsurers as agents** — finite-capital reinsurers with their own balance sheets, XoL contracts, cat-bond fallback; enables counterparty-contagion study.
4. **Exogenous reinsurance price index** — reinsurance as a time-series price signal feeding primary loss ratios (no reinsurer agents).

---

## 5. Target emergent phenomena

The macro behaviours a first-principles model aims to reproduce *without hardcoding them*. Each is a domain phenomenon with a real-world basis; the "mechanism" column is the agent-level story believed to generate it.

| # | Phenomenon | What it is / why it matters |
|---|---|---|
| 0 | **Risk Pooling (LLN)** | Aggregate attritional losses predictable (CV ~ 1/√N); cat losses do *not* compress with pool size (shared occurrence). Validates the loss architecture. |
| 1 | **Underwriting Cycle** | Multi-year oscillation of premium rates (Lloyd's: 5–10 yr peak-to-peak). Soft (capital abundant, rates → ATP, CR → 100%) → shock (cat/reserve depletes capital) → hard (capacity scarce, rates rise, CR < 100%, capital rebuilds) → capital entry → soft. *The most robust stylised fact in property-cat reinsurance.* Drivers: capital depletion, capital entry lag, investment income, reserve development, reinstatement, demand elasticity, competitive/herding pricing. |
| 2 | **Catastrophe-Amplified Capital Crisis** | A large (or clustered) cat forces simultaneous syndicate losses exceeding normal buffers → wave of insolvencies / capital calls removing market capacity. Tests fat-tailed, non-linear propagation. Driver: cross-syndicate correlation of held risk from shared occurrence. |
| 3 | **Broker-Syndicate Network Herding** | Followers converge on a credible lead quote even when their own estimates differ → clustered pricing, amplified mispricing in both directions. Channel for both information transmission and error propagation; which dominates depends on relationship-network topology. |
| 4 | **Specialist vs Generalist Divergence** | Narrow-specialism syndicates outperform in stable periods but are more exposed to correlated shocks in their line. Tests whether heterogeneity produces realistic performance dispersion vs portfolio convergence. |
| 5 | **Relationship-Driven Placement Stickiness** | Brokers keep routing to established partners despite cheaper/newer capacity available; market share adjusts slowly. Damps competitive adjustment, lengthens cycles. A behavioural friction with empirical counterparts. |
| 6 | **Counter-cyclical Capacity Supply** | After shocks, new syndicates enter (attracted by returns), restoring capacity with a multi-year lag; in soft markets / post-cat, capacity exits. Prevents permanent post-cat oligopolisation. The entry lag is what lets hard markets sustain elevated rates long enough to attract capital. |
| 7 | **Post-Catastrophe Market Concentration Surge** | Simultaneous insolvencies concentrate share among survivors (larger, more diversified, better-capitalised), who temporarily dominate panels and price above normal — until new entrants erode their position. The full recovery arc; validates against #6. |
| 8 | **Geographic / Peril Accumulation Risk** | A single event strikes all syndicates holding exposure in the struck region at once. Routing + specialism produce systematic accumulation of correlated exposure. Effective diversification depends on *spread*, not size. Creates selection pressure toward diversification from capital constraints, not a hardcoded goal. Exists on the demand side too (an insured with many assets in one territory accumulates correlated GUL). |
| 9 | **Experience Rating & Insured Risk Quality** | Underwriters surcharge/restrict/decline insureds by loss history → the insured pool self-selects; chronic loss-generators pushed to specialist markets or out. A shrinking pool in hard markets damps apparent hard-market profitability. |
| 10 | **Layer-Position Premium Gradient** | ROL decreases with attachment height (working layers 15–30%, upper/remote 1–8%), emerging from each layer's expected-loss distribution. Post-cat hardening is non-uniform: primary-layer rates spike more than upper-layer. Adds a vertical (layer) specialism dimension orthogonal to line-of-business. |
| 11 | **Reinsurance Contagion Cascade** | A reinsurer insolvency makes multiple primaries lose expected cession recovery simultaneously → correlated secondary shock, can trigger primary insolvencies even with well-controlled gross exposure. Requires finite-capital reinsurer agents + shared reinsurers. |
| 12 | **Pricing-Rule Evolution Under Selection Pressure** | Over long horizons (200+ yr), syndicates with miscalibrated pricing sensitivities underperform/go insolvent; the surviving parameter distribution converges to a natural equilibrium — making market behaviour a *finding* rather than an assumption. |
| 13 | **Reserve Development / Adverse Loss Reserve** | Reserves develop after the loss year (3-year account): adverse development = lagged capital shock extending hard markets; release exhaustion = abrupt hardening trigger with no new cat. |
| 14 | **Cat-Model Homogeneity as Systemic Risk** | When all syndicates use the same vendor cat model, they misprice the same events in the same direction (e.g. Andrew 1992, Harvey 2017, Ida 2021). Distinct from behavioural herding: independent agents arrive at the same *wrong* answer through shared structural assumptions. |

**Note on emergence vs encoding:** a phenomenon is only a genuine *test* of the model if it emerges from first-principles agent behaviour and can be checked against an independent empirical target. A phenomenon produced by a hardcoded response function (e.g. a fixed capacity price uplift, designer-set cycle amplitude bounds, a fixed herding weight, a fixed pricing memory) is tautologically consistent with its parameters and cannot falsify the assumptions baked into it. The redesign decision of *which* behaviours to let emerge vs encode determines what the model can actually validate.

---

## 6. Calibration anchors (real-world reference numbers)

Anchors for a US-Atlantic-wind / Lloyd's-specialty scope. These are real-world reference points; treat specific simulation parameter values as one prior calibration, not domain truth.

### Peril frequency (US Atlantic hurricane)
- All-category US landfalling hurricanes: **~1.7/yr** (NOAA, 1900–present).
- Major (Cat 3+) US-landfalling: **~0.6/yr** historically.
- NOAA NCEI billion-dollar hurricane events: **~1.5/yr** (>$1B insured); events above ~$5B industry threshold: **~0.3–0.6/yr**.
- Swiss Re sigma 2025: insured nat-cat losses exceeded **$100B for the fifth consecutive year** (2020–24); 2024 alone: Helene (~$16B insured), Milton (~$25B insured).
- A **market-loss-event** rate (one event hits the whole panel at once) of ~0.5/yr sits mid-range of the >$5B-threshold band.

### Peril severity (cat damage fraction)
- GPD shape from peaks-over-threshold on US hurricane damage: empirical ξ = **0.66–0.80** (95% CI) → equivalent Pareto α = **1.25–1.52** (α = 1/ξ). This is a genuinely heavy tail (α near 1.5 ⇒ near-infinite variance).
- Modelling tension: heavier tails (α ≈ 1.5) match empirics but give infinite variance (destabilises EWMA estimators); softer tails (α ≈ 3) buy finite variance at the cost of understating the tail. A **decision point**, not a settled value.
- Hurricane deductibles: **1–5% of TIV** for standard coastal property.
- Per-syndicate capital impact in a major event: roughly **2–20%** for large syndicates (order-of-magnitude, e.g. Hurricane Ian).

### Attritional frequency / severity (large commercial property, ~$50M class)
- Claim frequency ~**0.2/yr per insured** (≈ one per 5 years) consistent with large risks where deductibles/risk-management suppress reported frequency.
- Commercial-property attritional loss ratio at Lloyd's: typically **45–50% of NEP**.
- LogNormal severity (σ≈1.0) standard for property damage; mean partial loss a few % of SI.

### Pricing / ROL
- US commercial property direct: **1–5% ROL** normal market; 3–8% hard market post-major-cat.
- Lloyd's direct property specialist (pre-Ian): **2–4% ROL**; post-Ian renewals **+30–60%**.
- Layered ROL (hard market 2022–24): primary/working **15–30%**; first excess **5–15%**; upper/remote **1–8%**.

### Combined ratios (Lloyd's actuals)
- 2024: **86.9%** (attritional LR 47.1%, major claims 7.8%, expense 34.4%).
- 2023: **84.0%** (attritional LR 48.3%).
- Property reinsurance 2024: ~75% (benefiting from prior-year reserve releases).
- Outlook 2025–26: 90–95% as the market softens.
- Bimodal by design: **75–84% benign years, 110–120% major-cat years** — pooling reserves the cat loading in good years for rare bad ones; long-run average converges to ~89%.

### Expense structure (Lloyd's 2024)
- Brokerage + acquisition: ~**22.6% of NEP**; management expenses: ~**11.8%**; total operating expense ratio ~**34.4%**.
- Lloyd's levies: annual subscription ~0.36% of GWP; Central Fund contribution ~0.35% of GWP.
- Cost-of-capital loading: ~5% primary, up to ~30% upper/cat layers.

### Capital / solvency
- FAL minimum 40% of stamp capacity; ECA = SCR × 1.35; solvency target ≥ 140%; **Lloyd's 2024 actual 206%**.
- Investment return on assets (Lloyd's 2024): ~4.5% of the balance sheet.

### Validation-target metrics (with empirical ranges)
| Metric | Target range |
|---|---|
| Annual expected cat loss / TIV | 1–2% |
| Cat-loss-year std/mean | > 1 (heavy-tailed) |
| Premium / TIV (effective ROL) | 2–6% (direct property) |
| Attritional loss ratio | 20–30% (benign) / ~47% (richer base) |
| Long-run average combined ratio | 85–95% |
| Benign-year combined ratio | 50–70% |
| Cat-year combined ratio | 100–160% |
| Renewal retention rate | ~90–95% |

---

## 7. Modelling decision points (where a choice must be made)

Neutral list of the places where `rins`'s docs note a simplification or open question — i.e. genuine design forks for a redesign, stated without prescribing an answer:

- **Policy terms**: full-value (attachment 0, limit = SI) vs real layer mechanics + programmes/towers.
- **Panel size**: single-insurer vs multi-syndicate subscription panels (and lead/follow pricing modes).
- **Territories / perils**: single peril+territory vs multiple correlated zones and lines of business.
- **Cat event footprint**: whole-portfolio uniform damage vs geographic subset with a shared intensity field; per-occurrence severity correlation.
- **Cat tail shape**: heavy (α≈1.5, infinite variance, empirically faithful) vs softer (finite variance, estimator-stable).
- **Cat ELF**: anchored to a model vs experience-updated (the latter risks soft-market erosion).
- **Pricing emergence**: how much of the cycle (amplitude, memory, herding intensity, capacity response) emerges from agent reasoning vs is encoded as fixed response functions / coordinator broadcasts.
- **Credibility**: linear years-based ramp vs Bühlmann-Straub (information-content) credibility.
- **Capital lifecycle**: persistent capital with/without distributions; capital calls in loss years.
- **Entry/exit**: flat entry trigger vs rising capital supply curve; binary exit (synchronises unrealistically) vs continuous line-fraction reduction.
- **Investment income**: absent vs deterministic-scenario vs stochastic (AR(1)) — material to cycle period/amplitude.
- **Reserve development / IBNR**: immediate settlement vs 3-year account with development factors and RITC.
- **Reinstatement premiums**: absent vs modelled (within-year hardening + post-cat income).
- **Reinsurance**: omit / parametric net retention / reinsurer agents / exogenous price index.
- **Demand side**: inelastic full-SI purchase vs quantity adjustment (limits/deductibles/self-insurance), demand response to loss experience, experience rating.
- **Renewal timing**: calendar year-end expiry vs quarterly renewal seasonality with per-policy inception/expiry.
- **Expense/brokerage**: single expense ratio vs explicit brokerage cash flows, GWP vs NEP distinction.
- **Insolvency handling**: state flag + decline vs managed runoff + Central Fund.
- **Cat-model heterogeneity**: shared model vs per-syndicate model-parameter distribution (systemic-risk dimension).

---

## 8. Literature references (cited in rins docs)

- **Cabral et al. (2024)**, *Exploring the Dynamics of the Specialty Insurance Market Using a Novel Discrete Event Simulation Framework: A Lloyd's of London Case Study*, JASSS 27(2):7. — Directly analogous ABM-DES of Lloyd's.
- **Pearson et al. (arXiv:2307.05581)** — Lloyd's DES proof-of-concept; explicitly omits reinsurance (capital constraints + exposure limits as sole capacity mechanism).
- **Paulson & Staber (2021)**, *A simulation of the insurance industry: the problem of risk-model homogeneity*, JEIC. — Reinsurers as capital-bearing agents; finds reinsurance is simultaneously stabilising and a contagion channel; risk-model homogeneity as systemic fragility.
- **Venezian (1985)** and **Cummins & Outreville (1987)** — underwriting-cycle theory; investment-income / interest-rate channel as cycle co-driver.
- **Meier & Outreville (2006)** — reinsurance as an exogenous time-series price signal feeding primary loss ratios (AR(2)).
- **Lloyd's Minimum Standards MS3** (*Price and Rate Monitoring*, 2021; strengthened 2022) — AvT framework, technical-premium floors.
- **Lloyd's Franchise Guidelines** (Market Bulletin Y5375, 2022) — per-risk line, cat aggregate, tail-risk constraints.

---

*Sources: `rins/docs/market-mechanics.md`, `rins/docs/phenomena.md`, `rins/docs/calibration.md`. Implementation detail, code locations, canonical parameter values, and the project's own design critiques were deliberately excluded to keep this neutral and reusable.*
