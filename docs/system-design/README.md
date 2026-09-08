# rinsanity — System Design

How we model the Lloyd's of London specialty insurance market. This layer is our opinion about which domain rules (see [`domain-source-material.md`](../../domain-source-material.md)) we carry forward, which we simplify, and what shape the model takes. It is the bridge between the domain and the code, and it describes the design as a whole — the end state — and why it must be that way. It does not track build order or implementation status; the code is the record of what currently exists.

## Purpose

`rinsanity` is an agent-based model of the Lloyd's specialty insurance market. Its purpose is to let the market's macro behaviours **emerge from first-principles agent behaviour** — insureds buying cover, syndicates pricing and bearing risk, brokers routing placements, capital depleting and rebuilding — rather than be hardcoded as response functions. The model is an instrument for *studying* those behaviours, so a behaviour is only interesting to the extent the model did not assume it.

## Two tiers of behaviour: diagnostic invariants vs phenomena

The model is built and trusted **bottom-up**. Two kinds of behaviour are distinguished, and they play different epistemic roles:

- **Foundational behaviours — diagnostic invariants.** Checks that the substrate is *physically correct*: the loss-settlement invariants (ground-up loss never exceeds sum insured, no claim below attachment, panel claims sum to the insured loss) and, above all, **risk pooling** — an insurer's aggregate attritional loss is more predictable than any single insured's, with the aggregate coefficient of variation falling as ~1/√N as the pool grows, *while the catastrophe component's CV does not compress with pool size*. A diagnostic invariant is not a research finding; it is an instrument reading that confirms the engine works.

- **True phenomena.** The actual research targets: the underwriting cycle, catastrophe-amplified capital crises, broker–syndicate herding, post-cat concentration, and the rest of the §5 catalogue.

The load-bearing principle: **no phenomenon is a finding unless the substrate's diagnostic invariants hold.** A phenomenon sitting on an incorrect substrate is an artifact, not a result. And — separately — a phenomenon produced by a hardcoded response function (a fixed capacity price uplift, a designer-set cycle amplitude, a fixed herding weight) is tautologically consistent with its own parameters and cannot falsify anything. Both failure modes are disqualifying; the design exists to avoid them.

## Scope: the design realises the whole phenomenon catalogue

The model realises **all** of the §5 phenomena, including the structurally heavy ones (reinsurance contagion, cat-model homogeneity as systemic risk, pricing-rule evolution under selection). Three structural commitments make the heavy ones possible and run through everything below:

1. **Capital and exposure accounting is gross-vs-net aware.** Outward reinsurance reduces net retained loss, and regulatory exposure limits bind on net figures. A model tracking only a single net capital figure could not host finite-capital reinsurer agents or express reinsurance contagion (#11), so the gross/net distinction is intrinsic to the accounting.

2. **Catastrophe beliefs are per-syndicate, not a single shared oracle.** Each syndicate prices cat risk from its own cat-model parameters. A single global cat-ELF would bake homogeneity in as an assumption — precisely the systemic-risk condition (#14) the model studies *as a variable*. Heterogeneous beliefs are the default; homogeneity is a configurable special case.

3. **Syndicate pricing behaviour is parameterised.** A syndicate's pricing sensitivities are explicit parameters of the agent, so a population varies across them and selection acts on them over long horizons (#12). A single hardcoded pricing rule would make market behaviour an assumption rather than a finding.

## Phenomenon coverage map

Each §5 behaviour has a home in the design, and none is hardcoded into existence. #0 is the foundational risk-pooling **diagnostic invariant** (an instrument, not a finding); #1–#14 are **true phenomena** that emerge from the mechanisms below and are read as findings only once the diagnostic invariants hold.

| # | Behaviour | Realising mechanism | Section |
|---|---|---|---|
| 0 | Risk pooling (LLN) | Attritional drawn independently per asset (CV ~ 1/√N); cat a single shared occurrence per zone (CV flat in N) | *Loss architecture*; *Diagnostic invariants* |
| 1 | Underwriting cycle | AvT multiplier driven by local headroom + placement feedback; capacity/cost-of-capital tightening; distributions re-soften | *Placement*; *Exposure management*; *Capital lifecycle* |
| 2 | Cat-amplified capital crisis | Shared cat occurrence correlates losses across exposed syndicates; zero-floor partial settlement; simultaneous limit tightening | *Loss architecture*; *Capital, insolvency, and runoff*; *Exposure management* |
| 3 | Broker–syndicate herding | Followers anchor *price* (not belief) toward a reputable lead with a derived weight `w` | *Lead, follow, and herding* |
| 4 | Specialist vs generalist divergence | Bühlmann-Straub credibility `Z = n/(n+k)` on information content | *Building the technical premium* |
| 5 | Relationship stickiness | Slow, experience-driven broker relationship scores; broker-level inertia | *Broker routing and relationships* |
| 6 | Counter-cyclical capacity supply | Endogenous entry on a rising supply curve + formation lag + relational ramp | *Capital lifecycle*; *Broker routing* |
| 7 | Post-cat concentration surge | Insolvency removes capacity; survivors hold share until lagged entrants erode it | *Capital, insolvency, and runoff*; *Capital lifecycle* |
| 8 | Geographic accumulation | Portfolio tail measure with explicit zone correlation binds the cat aggregate | *Exposure management* |
| 9 | Experience rating & insured quality | Broker-presented per-risk loss record adjusts loss cost, meets WTP exit → pool self-selects | *Demand and clearing* |
| 10 | Layer-position premium gradient | Cost-of-capital derived from marginal tail-capital consumed by each layer | *Cost of capital*; *Layers and towers* |
| 11 | Reinsurance contagion | Finite-capital reinsurers (reusing the syndicate substrate) sharing treaties | *Reinsurance* |
| 12 | Pricing-rule evolution under selection | Insolvency culling + success-weighted inheritance with mutation over the genome | *Selection over the parameter genome* |
| 13 | Reserve development | 3-year account with IBNR developing to ultimate; reserving bias drives masking/step-up | *Reserve development* |
| 14 | Cat-model homogeneity as systemic risk | Truth/belief separation; heterogeneity-spread and shared-bias knobs | *Truth versus belief* |

## Loss architecture (the substrate)

The physics of loss, on which every phenomenon rests. Two structurally different peril classes coexist, and the difference between them *is* the design:

- **Territories.** The market spans **multiple territories** (peril zones), each carrying its own catastrophe process. Multiple zones are required so geographic accumulation (#8) can arise and so the only thing that reduces catastrophe variance — diversification *across* uncorrelated zones — is available to agents. A single-zone world expresses neither.

- **Attritional perils** are drawn **independently per asset**. Their aggregate pools: the coefficient of variation of an insurer's attritional book falls as ~1/√N with the number of risks. This is the heart of the risk-pooling diagnostic invariant.

- **Catastrophe perils** are a **single shared occurrence per event**: one cat event strikes *all* exposed assets in the struck territory simultaneously and applies a **uniform damage fraction** across them. Because the draw is literally shared, adding more risks in the same territory does not reduce the catastrophe component's CV — the dominant variance is event-level severity, perfectly correlated across the exposed pool.

The uniform within-territory footprint is deliberate. A shared intensity field with a per-asset damage response would give high-but-imperfect correlation, but it blurs the very invariant the substrate exists to demonstrate (with uniform damage, "cat CV is flat in N because the draw is shared" is exact) and introduces a second severity law to calibrate. No target phenomenon requires sub-territory spread: accumulation (#8) is a *cross*-territory effect, expressed by holding correlated exposure across zones, not by spread within one.

### Catastrophe tail regime

Catastrophe severity is **heavy-tailed**. The empirical US-hurricane damage tail is genuinely fat (Pareto α ≈ 1.25–1.52, i.e. near-infinite variance), and that fatness is the engine of cat-year capital volatility and the catastrophe-amplified capital crisis (#2). A finite-variance regime (α ≈ 3) would buy well-behaved estimators only by *understating the hazard*, defeating the phenomenon under study.

The heavy tail's worst pathology — divergent variance breaking experience-based estimators — is tamed not by an arbitrary cap but by **two hard domain invariants stacked on each other**:

1. The damage fraction lives in **[0,1]**, so **ground-up loss ≤ sum insured** (the physical cap, per asset).
2. Policy terms cap the **insurer's loss at the layer limit** (× panel share) — a tighter bound than the GUL cap, and the one that governs an insurer's capital exposure.

The severity draw therefore has a heavy *body* but bounded *support*, and from any single insurer's balance-sheet perspective the per-risk loss is doubly bounded. This is why a heavy tail is affordable without the estimator instability unbounded heavy tails cause. It is also one reason cat loss costs are *model-anchored, not experience-updated*: heavy-tailed draws make a benign sample uninformative about the hazard. The tail parameter and severity constants are calibration in the code; the *regime* — heavy, bounded by the domain invariants above — is the design commitment.

### Layers and towers — the unit of placement

The **unit of placement is the layer, not the policy.** A coverage request for an asset is satisfied by a **tower**: a vertical stack of consecutive layers `[0, a₁], [a₁, a₂], …`, each an independent contract with its own attachment, limit, premium, and panel, all written over the same underlying ground-up loss. A single occurrence's GUL flows up the tower deterministically — for each layer, `gross = min(GUL, limit)` and `net = gross − attachment` (zero if `GUL ≤ attachment`).

Towers, rather than single full-value policies, are required because the layer-position premium gradient (#10) is intrinsically vertical: rate-on-line falls as attachment rises, and post-cat hardening is non-uniform — primary layers spike more than upper layers *on the same risk*. That within-risk gradient cannot emerge if each risk is a single undifferentiated policy; it would at best be an artifact of risk heterogeneity. Towers also make **vertical specialism** (writing only upper/remote layers) a real strategy, orthogonal to line-of-business specialism (#4).

The tower's band structure is a **demand-side choice**: the insured selects where its layers attach by trading retention against premium (see *Demand and clearing*), and the substrate consumes whatever bands result — it never decides where layers attach. The band structure is therefore not fixed: a price spike that makes a tranche uneconomic lets the insured restructure its tower, which is the fine-grained form of demand elasticity.

### Panels and the settlement cascade — the horizontal split

Each layer is placed on a **subscription panel**: an ordered set of `(syndicate, share)` entries whose shares sum to the placed portion, with one member designated **lead** and the rest **followers**. Single-syndicate placement is a panel of one, not a separate mode. The placement structure is what three phenomena live on: herding (#3) *is* followers converging on a credible lead quote; post-cat concentration (#7) is the redistribution of panel share toward survivors; and the pro-rata settlement is meaningless without a panel.

The **settlement cascade** is mechanical and invariant-checked: an occurrence produces a GUL per exposed asset; the GUL flows up each affected tower; each penetrated layer yields a net insured loss; that loss is **pro-rated across the panel by share**, and each syndicate's capital is debited its share. The settled amounts across a layer's panel sum to the layer's insured loss (up to rounding), no claim arises below attachment, no layer pays above its limit, and expired layers generate no claims — these are the loss-settlement diagnostic invariants.

This fixes a clean boundary. **The substrate owns the panel *representation* and the *pro-rata settlement* — purely mechanical, fully invariant-checked.** It does **not** own *how a panel is formed or priced*: how the lead prices blind, how followers weight the lead quote, how shares are assembled. That quoting behaviour is agent logic in the pricing layer, where herding emerges, and it is never hardcoded into the substrate.

### Capital, insolvency, and runoff

A syndicate's capital is a **persistent balance**: premiums credit it, claim shares debit it, and the balance carries over between years with **no annual re-endowment**. Persistence is the precondition for every dynamic phenomenon — capital depletion and slow rebuild generate the multi-year cycle (#1) and the post-cat capacity crunch (#2, #6, #7). A capital figure that reset each year would make all of them impossible.

Capital has a **hard floor at zero**. When a claim share would take a syndicate below zero, it pays `min(share, remaining_capital)` and the remainder is an **unrecovered shortfall**. Subscription liability is **several, not joint**: each panel member is liable only for its own share, so an insolvent member's shortfall **falls on the insured**, not on the solvent co-subscribers. This keeps insolvency a *capacity-and-recovery* event localised to the failing syndicate and its cedents, rather than a contagion path between primaries on the same panel.

An insolvent syndicate enters **runoff**: it writes no new business but continues settling its in-force layers (subject to the same zero floor) until they expire. Runoff makes insolvency a gradual withdrawal of capacity rather than an instantaneous disappearance.

The design **excludes the Central Fund** — Lloyd's mutual backstop that would make insureds whole when an insolvent syndicate cannot. It is a welfare mechanism, not a cycle mechanism: the market-dynamics consequences of insolvency (lost capacity, shortfall, runoff) are fully captured without it, and the shortfall falling on insureds is what couples insolvency back into the market.

The substrate guarantees the *accounting* — persistent balance, credit/debit, zero-floor partial settlement, several liability, runoff. The behaviours that *read and write* capital but are not mechanical — profit distributions, capital entry, line-size discipline — are emergent, parameterised agent behaviours and belong to higher layers.

## Time and the market loop

The model runs over many years, but time is **not atomic at the year**. Two commitments:

- **A within-year time axis.** Occurrences fall at points within a year — catastrophe events as a within-year arrival process, attritional losses aggregated per period — rather than at a single annual instant. Pricing, placement, and renewal happen at discrete *dates*, but losses are not collapsed to one moment. This sub-annual resolution is what makes several mechanics representable: **multiple cat events in one year** (the non-linear frequency penalty), **reinstatement premiums** (a second event triggers an extra reinstatement — within-year hardening), partial-year exposure for mid-year placements, and runoff that settles claims until each policy expires.

- **Per-policy inception and expiry dates.** Each layer carries its own annual term with explicit inception and expiry, rather than the whole market renewing at one global boundary. Inceptions follow the **quarter-day renewal calendar** (with 1 January's outsized share giving it signalling power that propagates to later renewals) — a demand-side scheduling behaviour. The substrate honours whatever dates a policy carries; the inception-date distribution is a property of demand, with nothing in the substrate depending on it being uniform.

## Pricing: a computed floor, an emergent cycle

This is the governing law of the model's dynamics, and the line it draws is what lets the underwriting cycle (#1) be a *finding* rather than an assumption.

**The technical premium (TP) is computed. The actual premium (AP), and therefore the cycle, emerges.**

- **TP is a rational reference price, and computing it directly is legitimate** — it is arithmetic, not a phenomenon. Each syndicate estimates its own loss cost, divides by a target loss ratio to get the actuarial technical price (`ATP = E[loss] / target_LR`), and adds a cost-of-capital/profit loading to get TP. TP is the long-run floor the market oscillates *around*; computing it encodes no market behaviour, because TP is not the price anyone pays — it is each syndicate's private notion of the price that just covers expected loss plus the cost of capital at risk.

- **AP is the market-clearing price, and it emerges from supply and demand.** The gap between AP and TP — the Actual-vs-Technical ratio, AvT = AP/TP — is the cycle: AvT < 1 in soft markets (syndicates competing capacity down below the technical floor), AvT > 1 in hard markets (scarce capacity letting syndicates hold out above it). That gap is the *result* of capacity-constrained syndicates competing for placements against insureds with finite willingness-to-pay — never an imposed function.

The design therefore **forbids**, as a standing property: a global "cycle position" or "market phase" variable; any coordinator broadcasting a hardening/softening signal; a fixed capacity price-uplift; designer-set cycle amplitude or period bounds; any expression of AP as `TP × f(t)` for a supplied curve `f`. A cycle from any of these is tautologically consistent with its own parameters and can falsify nothing. The design **permits**: a syndicate setting its *ask* above or below its own TP as a function of its *own* capital and capacity state and its read on the market, and the placement process clearing those asks against demand.

### Building the technical premium

A syndicate's loss cost splits into two components governed by **structurally different update rules** — and that asymmetry is itself load-bearing.

- **Attritional ELF — experience-updated.** Attritional losses are high-frequency, so a single year is informative. The estimate blends the syndicate's **own realised burning cost** with an **industry benchmark** by credibility weighting: `attritional_ELF = Z · own + (1 − Z) · benchmark`.

- **Catastrophe ELF — model-anchored, never experience-updated.** Cat ELF comes from each syndicate's own cat-model parameters (commitment #2) and is *not* pulled down by benign experience. A quiet decade is a benign sample, not evidence the hazard fell; experience-updating cat ELF causes systematic soft-market rate erosion — the dominant cycle-failure mode. Experience-updating the cat ELF is therefore not a supported mode but a **miscalibration the model can represent and select against** (#12): an agent that does it underprices the tail and is punished when the tail arrives.

**Credibility uses the Bühlmann-Straub form**, `Z = n / (n + k)`, where `k` is the ratio of within-syndicate process variance to between-syndicate variance of hypothetical means — credibility as a function of *information content*, not elapsed time. This lets specialist/generalist divergence (#4) **emerge**: a narrow specialist with dense, low-variance data in its line earns high `Z` and trusts itself, while a thin generalist earns low `Z` and leans on the benchmark — straight from the variance structure. A linear years/volume ramp would instead *fit a slope* to imitate that, edging back toward encoding the phenomenon. `k` is a per-syndicate parameter in the selectable genome (#12).

**Two experience channels, deliberately separate.** The credibility blend above rates the **syndicate**: its `own` burning cost is *its own book's* realised experience and `n` is *its own* accumulated exposure, which is what makes specialism earned (#4). Rating the **risk** is a second, independent adjustment — an **experience modifier** on the loss cost for a specific insured, credibility-weighting that risk's realised loss relativity against unity (#9). The two must not be merged: one asks how much a syndicate should trust itself, the other how much a particular risk should be surcharged.

**Expense loading is multiplicative**: `gross = pure_premium / (1 − expense_ratio)`. Additive loading systematically underprices, because each unit of loading must itself be funded — a structural error, not a calibration nuance.

The actuarial technical price is `ATP = loss_cost / target_loss_ratio`, and TP adds the cost-of-capital loading below. The specific values — target loss ratio, expense ratio, benchmark, cat-model constants, `k` — are calibration in the code; the forms are the design commitments.

### Cost of capital — derived, not scheduled

TP for a layer is `ATP(layer) + hurdle_rate × marginal_capital(layer)`. The cost-of-capital loading is the required return on the **risk capital the syndicate must hold against that specific layer** — its `marginal_capital`, the layer's marginal contribution to the syndicate's risk-capital requirement (the portfolio tail measure formalised under *Exposure management*).

The loading is **derived from capital consumption, not read from a per-layer schedule.** Upper, cat-exposed layers consume disproportionate *tail* capital per unit of limit, so their loading comes out high; working layers consume little, so theirs comes out low. The layer-position premium gradient (#10) and its non-uniform post-cat hardening therefore **fall out of capital consumption** rather than being hardcoded — a fixed "5% primary / 30% upper" schedule would encode exactly the phenomenon meant to emerge. It also couples pricing to capital state: post-cat, the same layer consumes a larger fraction of the now-smaller capital, so its loading rises — a structural channel into hard-market pricing that needs no cycle function. The `hurdle_rate` is a per-syndicate genome parameter (#12).

## Exposure management — the portfolio tail measure

A syndicate decides what it can write from a single coherent **portfolio tail measure**, not from independent per-risk caps. The measure is the syndicate's estimate of its **net aggregate loss at a chosen return period** (1-in-200), computed from its *own* cat-model beliefs over its *current net book*, **accounting for zone correlation**: cat losses within a zone are one shared occurrence and add directly, while across zones they diversify.

Independent per-risk fraction caps are insufficient because they are **blind to correlation** — two risks in the same zone accumulate into one shared occurrence, and a per-risk cap cannot see that. Since geographic accumulation (#8) is precisely the effect under study, the binding measure is portfolio-tail-aware.

Two limits derive from it, both computed on a **net (post-reinsurance) basis** and recomputed from *current* capital at quote time:

- **Cat aggregate (per zone):** a risk is acceptable only if the post-addition return-period net loss stays **coverable within capital × a solvency fraction**. Computed on the actual correlated book, an already-loaded zone hits this fast — so accumulation pressure and the incentive to diversify *across* zones emerge from the constraint rather than being a hardcoded goal.
- **Per-risk line:** a simple `net line ≤ line_fraction × capital` cap for single-risk/working-layer exposure. The LLN already tames attritional accumulation, so this needs no tail machinery.

A quote is **declined** when adding the risk would breach either limit, or when the syndicate is insolvent.

Because both limits and the cost-of-capital loading read *current* capital and the *same* tail measure, a post-cat drawdown **tightens capacity and raises price at once** — the capacity crunch and the hardening are two faces of one depletion. This is a core driver of the cycle (#1, #2, #6, #7), emerging from local capital accounting with no market-phase variable.

The exposure measure is defined at a single binding return period (1-in-200 coverability). The design admits further return-period constraints, but the regulatory tail-shape rule (1-in-500 ≤ 135% × 1-in-200) is not imposed: it is a distribution-shape floor that does not bear on any target phenomenon. The return period, solvency fraction, and `line_fraction` are calibration in the code.

## Placement — how AP forms

The actual premium is `AP = TP · AvT`, and the cycle lives in AvT. Two distinct channels move the price, and keeping them separate avoids double-counting scarce capital:

- **TP channel (rational).** Capacity scarcity raises `marginal_capital`, so the cost-of-capital loading and hence TP itself rise post-cat. This is the syndicate correctly charging more for scarcer capital — not the cycle, the floor moving.
- **AvT channel (competitive).** The gap *around* TP. `AvT < 1` is pricing **below** technical premium — the soft-market discipline failure no cost-of-capital story explains; `AvT > 1` is holding out **above** TP because demand exceeds supply. This gap is the cycle.

Each syndicate carries an **AvT multiplier as a slow-moving state variable**, re-set **once a year at renewal** from two purely local inputs — never from any market-phase signal. The annual cadence is what keeps it slow-moving and co-locates it with the year-end capital lifecycle (distributions roll first, so AvT reads the **post-distribution** capital the syndicate will actually carry into next year — a heavy payout leaves less headroom and lifts next year's ask). Within-year *capacity* discipline is not AvT's job: the exposure limits bind live at every quote, so a syndicate that fills its book mid-year is gated immediately; AvT only governs the slow re-*pricing* of its ask. The two inputs:

1. **Capacity headroom** — free exposure budget relative to capital. AvT **relaxes toward a target level** set by headroom: at normal headroom the target is exactly `1` (price at the TP floor the market oscillates around), abundant headroom (idle capital, low opportunity cost of writing) sets it **below 1** to win business, scarce headroom sets it **above 1**. This is the structural core of the cycle: a shared catastrophe collapses *every* exposed syndicate's headroom at once, so their targets jump above 1 together and their AvT multipliers climb *together* — market-wide hardening as the aggregate of correlated local states, with no coordinator. Targeting a *level* (not integrating a per-round nudge) is what anchors AvT: a path-dependent nudge has no rest point and drifts arbitrarily far from the floor, whereas a level target makes TP the standing attractor and the slow relaxation toward it the multi-year hard-market persistence.
2. **Placement feedback** — a local price-discovery loop that nudges AvT **homeostatically toward a target win-rate** (the syndicate's *share-appetite*): winning more than its target lifts AvT to give margin back, winning less cuts it to compete. Each syndicate therefore settles at its own appetite, and the collective scramble of high-appetite syndicates undercutting for share is the soft-market erosion below the floor; the lag of this slow loop gives the cycle its inertia and overshoot. A bare directional nudge (won → up, lost → down, no target) is rejected: it implicitly chases a 50% selection rate no population can collectively hold, hardcoding a target by omission, whereas an explicit appetite is a real, selectable trait.

The two channels combine **additively** into the year's AvT increment — they are independent reads (own capital state vs own price discovery), so summing them double-counts nothing. The **responsiveness** parameters — how sharply AvT reacts to headroom and to feedback — and the **target win-rate (share-appetite)** itself are per-syndicate and **selectable (#12)**: a syndicate that chases share too hard in soft markets prices below the floor and is punished when the tail arrives. The cycle (#1) is whatever the population of these local rules produces.

### Lead, follow, and herding

A placement has a **lead** and **followers**. The broker designates the lead (by relationship, expertise, capacity — see routing). The **lead quotes blind**, from its own TP·AvT, seeing no other quote. Followers then quote *having observed* the lead's quote, and this is where herding (#3) arises.

A follower **anchors its quote toward the lead's**, not its beliefs toward the lead's:

```
follower_quote = (1 − w) · own(TP · AvT) + w · lead_quote
```

The weight `w` is **derived, never a fixed constant** (a fixed herding weight would make #3 tautological). It rises when the follower's *own* estimate is low-confidence — the same information-content logic as credibility, thin own data leaning on the lead — and when the **lead is more reputable** (track record, relationship strength). A per-syndicate **herding susceptibility** scales `w` and is selectable (#12).

The blend moves **price, not belief**. A follower does not overwrite its own cat-model parameters with the lead's. This keeps **pricing herding (#3) orthogonal to cat-model homogeneity (#14)**: #3 is behavioural convergence on a *price*; #14 is independent agents sharing a *model* and reaching the same wrong answer. Folding belief into the herding blend would make it impossible to study either phenomenon without inducing the other.

Followers apply their **own exposure limits and may decline** regardless of the lead — herding moves price, never capacity discipline. With this, #3 emerges: clustered quotes when followers have poor info and a reputable lead, with the relationship-network topology (defined by routing) deciding whether the cluster transmits good information or propagates the lead's error.

The panel forms by **firm-order subscription**. The lead's `TP · AvT` is the layer's **firm order** — the single price the insured pays. A follower **subscribes when the firm order sits at or above its acceptance threshold** — its reservation price, `own(TP · AvT) · (1 − w · concession)` — and its exposure limits permit, taking a share; the panel fills capacity-first up to the layer limit, and an unfilled remainder leaves the layer **partially placed** (the insured restructures or retains the gap). The insured pays one price per layer, never a blend of the panellists' quotes. This is why herding is load-bearing in *formation*, not only in the quote: a follower anchored toward a reputable lead lowers its threshold and **subscribes to a firm order it would otherwise reject** — the channel by which a reputable lead's mispricing propagates across the panel (#3). The weight `w` must move the **threshold**, not a quote that is then measured against the firm order: an anchored quote compared with the lead's own price cancels `w` out of the decision entirely (`(1 − w)·own + w·lead ≤ lead ⟺ own ≤ lead` for every `w < 1`), which is what left the channel inert until #31. Herding is a **reservation-price concession, not a quote blend** — `w` may relax a follower's acceptance, and never touches the firm order, the lead's `TP · AvT`, or anything else the insured pays. The `concession` constant sets how far below its own technical view a fully herded follower will write; the anchored quote itself survives as a reporting figure. Each such subscription — a firm order taken below the subscriber's own price — is counted in the year's emission (`concessive_subscriptions`), so the propagation of a lead's error is readable rather than inferred. A competing-auction alternative — stacking quotes cheapest-first and clearing at the marginal bid — is rejected: it reduces followers to price-takers in an order book, sidelining the subscription-herding the panel exists to express, and is more auction-like than the subscription market it models. Price competition is **across** panels — a syndicate leads one placement and follows another, and the insured takes the cheapest assembled panel within WTP (see *Demand and clearing*) — not an intra-layer auction.

**Line size sets panel granularity.** How much of a layer a syndicate offers is a genome trait (`target_line`), selectable like any other; the placement *convention* on top of it is that a syndicate writes a larger line **as lead** than it would as a follower (`lead_line = target_line × LEAD_LINE_MULTIPLE`, capped at the whole layer) — the domain norm, and the share that carries the reputational signal behind the firm order it set. At Lloyd's-scale lines (followers around 10% of the limit, a lead around 20%) a layer places across a panel of many subscribers rather than a lead and one follower, and the broker's shortlist has to be long enough to fill one with slack for the followers that decline. The granularity is not cosmetic: post-cat concentration (#7) *is* share redistributing across a panel, price herding (#3) is a clustering phenomenon that needs a population of followers, and counter-cyclical capacity supply (#6) routes through how many syndicates can participate in a placement at all. It also has a real cost, and the reference market shows both sides of it: granular books are diversified books, so the market absorbs shocks a lumpy two-member panel could not — the systemic-risk experiment (#14) has to be re-cut thinner on the capital scale to reach the same place — and the year's emission carries `mean_panel_size` and `mean_placed_portion` so panel composition is read rather than inferred.

### Broker routing and relationships

Brokers are **stateful agents**, and there are **several** of them with heterogeneous relationship portfolios — different brokers favour different syndicates. Each broker holds a **relationship score per syndicate**. Persistent relationships are required: stickiness (#5), the herding network topology (#3), and the relational half of the entry lag (#6) all depend on them, so routing is not a stateless function.

For each coverage request the broker **shortlists syndicates capable of the risk** (right line and zone) **weighted by relationship score**, and designates the **lead** as the strongest credible relationship on that shortlist. Routing does relationship-driven *shortlisting and lead designation only* — it does **not** pick the winner on price. Which quote the insured accepts is a demand-side decision. This separation keeps routing stickiness (#5) from being entangled with price selection.

Relationship scores **update slowly from experience** at year-end: raised by competitive quoting, winning placements, and paying claims reliably (staying solvent); eroded by declines, gross mispricing, or insolvency. **Stickiness (#5) emerges from this lag** — relationships trail the competitive landscape, so share adjusts slowly — rather than from a hardcoded stickiness factor; the update is calibrated so renewal retention lands around 90–95%. The **update inertia is a broker-level parameter**, so brokers differ in loyalty and that heterogeneity is itself part of the topology.

**New entrants start with low relationship scores everywhere** and must build them over several years to reach material panel share. This *is* the relational component of the counter-cyclical entry lag (#6): the lag emerges from relationship dynamics, not a separate entry timer.

### Demand and clearing

The insureds are drawn from an **insured population**: a market-level distribution with a mean sum insured, a mean risk aversion, and a **population spread** that disperses both — and, separately, the asset's own **loss-proneness**, a multiplier on the market attritional peril. Every dimension is dispersed mean-preservingly (an evenly spaced ladder, shuffled independently per dimension), so the spread scatters the population without moving its average: the market `AttritionalPeril` remains the population's mean hazard and the size/WTP calibration the market was tuned on is unchanged. Zero spread reproduces a cohort of carbon copies. The dispersion is what several phenomena are *defined on* and could not previously express — experience rating and insured quality (#9) needs chronic loss-generators to find, specialist/generalist divergence (#4) needs risks whose information content differs, and geographic accumulation (#8) binds on exposure that varies in size and not merely in count. Loss-proneness is **substrate truth**: it is drawn at construction, fixed for the asset's life, and read by nothing but the peril that strikes the asset — the pricing, demand and placement path never sees it, which is checked by holding a market's observable population fixed and scattering its hazards, and finding the year's placements and rate index bit-identical while its losses differ.

Insureds are agents holding assets (in zones, exposed to perils), a **loss record**, and a private **willingness-to-pay** equal to their expected loss scaled by a per-insured **risk-aversion loading** (> 1). Demand is **price-elastic in quantity**: facing the presented quotes, an insured structures its tower to maximise value at its WTP — raising retention, lowering limit, or self-insuring a tranche when prices spike, and declining entirely when even minimal cover exceeds WTP. This is a genuine cycle *damper* (#1): hard-market prices shrink the quantity of cover bought, the insured pool contracts, and apparent hard-market profitability is damped. (A purely binary buy-all-or-decline rule would make demand inelastic in quantity and forfeit this damping, so the elastic response *is* the tower band-selection the substrate consumes.)

WTP is a stable per-insured preference (risk-aversion × expected loss); the influence of an insured's own losses enters the market through **experience rating (#9)**, on the supply side, not by perturbing WTP. Experience rating is supply-side pricing on a demand-side attribute: a syndicate's loss-cost estimate for a *specific* risk is adjusted by that insured's own **loss record** (surcharging bad histories, crediting clean ones), so chronic loss-generators are surcharged beyond WTP or declined.

Both channels are **wired into the engine**. Each insured accumulates its loss record year by year — the asset burns whether or not it bought cover, and either way the year is evidence the broker presents next renewal — and the record rides onto the submission every syndicate quotes. The modifier scopes to the **attritional** ELF only: applying an attritionally-derived modifier to the cat ELF would experience-update a model-anchored loss cost, which is the one thing the tail regime forbids. On the other channel each syndicate accumulates its own exposure-years and what that book burnt, so `own` and `n` in the blend are earned rather than assumed.

**Reading the self-selection.** The `cycle` emission carries four readings, and the first thing the diagnosis established is that the obvious one is the wrong one. `insured_declines` counts risks that bought **nothing at all**, and it has a deliberately narrow meaning: because `restructure_tower` sheds the *worst value-for-money* band first, a risk surcharged past its budget shrinks its tower rather than leaving, and that response never reaches the counter. `limit_offered` against `limit_bound` (and their ratio, `limit_bound_share`) is what sees **band-shedding**; `pool_burning_cost` — the burning-cost relativity of the pool taken on, weighted by the attritional exposure each placement carried — is what sees the *composition* of the pool change as chronic risks shed the band their own losses land in and clean ones, credited, buy more. Two further readings measure the structural causes directly: `offered_price_to_wtp` (what the whole offered tower costs as a fraction of the willingness-to-pay it is offered against — the slack a surcharge must climb through) and `attritional_share` (the share of the offered loss cost that is the rateable component at all). Pinning `credibility_k` very high rates nothing and is the control arm: the same market, the same seed, the same population, so no measured gap can be heterogeneity.

**The measured diagnosis, and the decision.** The experiment is the `SelfSelectionArm` sweep: the reference market with two structural knobs — the catastrophe regime (truth and belief scaled together, so the market stays as calibrated as it was and only the *composition* of the loss cost moves) and the insured population's mean risk aversion — each arm run rated against its own unrated control, over a fixed eight-seed panel. Over 300 years per seed:

| arm | rated pool | unrated pool | selection gap | right-signed | price/WTP | attritional share |
|---|---|---|---|---|---|---|
| reference | 0.981 | 0.994 | +0.013 | 5/8 | 0.73 | 0.16 |
| attritional-only loss cost | 1.006 | 0.999 | −0.006 | 3/8 | 0.62 | 1.00 |
| budget tight against price | 0.980 | 0.975 | −0.005 | 3/8 | 1.26 | 0.16 |
| **both released** | **0.844** | **0.991** | **+0.147** | **8/8** | 1.12 | 1.00 |

The three candidate causes rank as follows. **Neither structural cause dominates, because neither is sufficient**: released on its own each leaves the gap inside seed noise and wrong-signed on the majority of the panel. They are not additive but **interacting** — a surcharge has to be *large* (which needs a loss cost that is mostly rateable) **and** land on a price that is *already at the budget ceiling* (which needs no slack), and in the reference market's own calibration relieving one worsens the other: switching the cat regime off drops the price from 0.73 of WTP to 0.62, because the loss cost the price is built on has collapsed. Released together the mechanism is unambiguous — the rated pool burns **15% less** than the population it was offered, on **every seed in the panel**. The mechanism is confirmed; what the reference market lacks is the regime, not the machinery.

The third candidate — priced-out risks restructuring rather than declining — is confirmed as an **instrumentation** failure rather than a behavioural one. In the diagnostic arm, where self-selection is loudest, `insured_declines` is **zero in every year of every seed in both arms** while the pool moves by a sixth, and the rated market ends up buying *more* limit than the unrated one (0.67 against 0.64 of what was offered): rating does not shrink the pool, it changes who holds it. Any reading of #9 taken off `insured_declines` would have reported nothing happening. `restructure_tower`'s sort is correct domain behaviour and is left alone; the observables above are the fix.

**Decision: the reference market does not move.** Reaching the regime where #9 expresses means switching the catastrophe process off *and* collapsing risk aversion onto the price — and the second knob alone leaves a market that binds 39% of the limit it offers and declines a third of its risks every year, while the first would take the cat-driven phenomena (#2, #6, #7, #8, #14) with it, since a cat-amplified capital crisis needs catastrophes. #9 is therefore recorded as **a phenomenon of attritional-dominated classes**: the mechanism is live, correctly signed and demonstrably strong where the loss cost is rateable and price meets the budget, and it is structurally muted at a cat-dominated calibration. That is a finding about the model's regime, not a defect, and the arms above are kept as the standing instrument for it.

The loss record is **broker-presented**: it travels with the risk on the submission, so every syndicate quoting a given risk sees the same record and the credibility volume on the experience modifier is a property of the record, not of the quoting syndicate. This is what makes #9 a statement about the *risk* — a chronic loss-generator is surcharged by the whole market rather than shuffled from the carriers that have seen it to those that have not, so it is genuinely priced out. What still differs between syndicates quoting the same risk is their credibility `k` and the benchmark they bring, which is where specialism (#4) lives. The record holds **realised attritional losses only** — the observable trace of an asset's loss-proneness, never the loss-proneness itself, which is substrate truth no agent reads, and never catastrophe losses, which are model-anchored. The pool then **self-selects** — bad risks pushed to specialist markets or out — as an emergent consequence of per-risk rating meeting the WTP exit decision, not a designed cull.

**Clearing** closes the loop: the broker presents the assembled quote(s); the insured takes the cheapest with `AP ≤ WTP` at its chosen quantity, or declines; on acceptance the layers are bound, the panel is fixed, and coverage is live.

## Capital lifecycle — distributions, entry, and emergent exit

Three forces govern how capital enters, leaves, and turns over between years. Two are dedicated mechanisms; the third emerges from machinery already in place.

**Distributions** release profit to capital providers at year-end, **suppressed in loss years and whenever capital sits below a solvency floor** (an impaired syndicate rebuilds before it distributes). This is the only thing preventing capital from accumulating without bound — without it the market could never re-soften after a hard phase, because abundant recovered capital is exactly what competes AvT back down. Distributions close the loop from hard markets back to soft. The payout rule (the distributable fraction, the floor) is a per-syndicate genome parameter (#12).

**Entry** is endogenous: new capacity responds to **expected returns above a hurdle along a rising supply curve** — the easiest capital deploys first, marginal capital demands progressively higher expected return — subject to a **formation lag**. New capacity instantiates as **new syndicates** with fresh capital, cat-model parameters drawn from the population distribution, and **no broker relationships**. The formation lag plus the relational ramp (from routing) together produce the multi-year delay that lets hard markets sustain elevated rates long enough to attract capital (#6) and that the survivors' temporary dominance (#7) then erodes against. Encoding a capital-*supply curve* is legitimate the same way TP is: it responds to *endogenous* market returns, not a scripted spawn schedule, so #6's counter-cyclical timing emerges from the lag interacting with the cycle.

**Exit is not a dedicated mechanism — it emerges.** Real soft-market withdrawal is gradual: reducing line fractions, pricing above the market, letting business route away. All of that falls out of mechanisms in place — a syndicate with poor prospects runs a high AvT or declines below its floor, loses placements through the feedback loop, and writes less; its exposure caps and headroom govern participation. The only abrupt departure is **insolvency → runoff**. A dedicated binary "leave the market" event is rejected because it would synchronise withdrawals unrealistically — the failure the gradual-exit domain fact warns against.

## Investment income — an exogenous cycle co-driver

Each period, a syndicate's capital and premium float earn an investment return at a **market yield**, crediting capital. Investment income is a documented co-driver of the cycle's *period*: high yields let syndicates tolerate combined ratios above 100% on a total-return basis (softening the hard-market signal), while near-zero yields force the full cost of capital out of underwriting alone (sharper, longer hard markets), and the empirical 5–10-year cycle tracks the interest-rate cycle, not cat frequency alone.

The yield is **exogenous**, and this is correct modelling rather than a simplification: the interest-rate environment is macroeconomic, genuinely outside the insurance market — an input the market responds to, never a phenomenon for the market to generate. It is the one place an external time series is faithful, and it makes a clean controlled study available (hold everything else, shift the yield regime, watch cycle period and amplitude move). The yield follows an exogenous stochastic process (a mean-reverting AR(1) interest-rate process), whose path and parameters are scenario/calibration inputs in the code.

Investment income feeds the **total-return signal** agents act on — the profitability driving distributions, entry, and the AvT feedback loop is underwriting *plus* investment return, not underwriting alone. That is the mechanism by which a high-yield regime softens the hard-market signal.

The signal reaches each mechanism **locally, through the balance sheet**, never as a broadcast. Distributions are released against the syndicate's own total return; entry reads the market's own realised total return off the supply curve; and the AvT loop is touched only because investment income credits capital, capacity headroom is measured against capital, and AvT is re-set off post-distribution headroom. No agent reads the yield to infer a market phase — the yield's only act is to credit or debit a balance.

## Reserve development — the 3-year account

Claims do not all settle at once. At occurrence a claim splits into a **near-term settled** part and an **IBNR reserve** — an *estimate* of ultimate loss that **develops toward the true ultimate** over the underwriting year's ~3-year life. Each development period debits capital (adverse development, strengthening) or credits it (favourable development, release), with a lag. At the end of the open period, **reinsurance-to-close (RITC)** crystallises the remaining liability and closes the underwriting year.

The 3-year account makes capital shocks **lagged** rather than instantaneous, and that lag is itself a phenomenon (#13). Adverse development is a secondary shock arriving 12–24 months after the event, re-tightening exposure limits and cost-of-capital and so **extending hard markets without a fresh catastrophe**. Favourable development releases reserve in benign years.

#13 **emerges from the gap between estimate and realised ultimate, not from a scripted release rule.** Each syndicate carries a **reserving bias** (optimism/conservatism) as a per-syndicate genome parameter (#12). Systematically optimistic reserving releases apparent profit in benign years and masks deteriorating underwriting quality; when realised ultimates catch up and releases exhaust, the correction is an abrupt combined-ratio step-up — a hardening trigger with no new cat. A designer-set "release profit in good years" rule would encode the phenomenon; biased estimation meeting realised losses lets it emerge and lets selection punish the over-optimistic.

This couples to the rest of the design: **RITC-close gates distributions** (an open year's profit cannot be distributed until the account closes), and lagged adverse development feeds the same exposure-limit and cost-of-capital machinery that drives hard-market pricing.

## Reinstatement premiums

A layer carries a **reinstatement count** and a **reinstatement factor** (≈100% of the original layer premium, pro-rated). When a catastrophe claim erodes the layer's limit, the insured pays `factor × original_premium` (pro-rated by amount and time remaining), the panel's capital is **credited in the same year as the loss**, and the **limit is restored**. Once the reinstatements are exhausted the layer stays eroded — no further cover that term.

This is the source of two effects flat annual premiums cannot produce, and both **emerge from the contract mechanics** rather than a pricing rule. First, a **non-linear cat-frequency penalty**: a clustered second event triggers another reinstatement charge, so a bad year costs more than twice a normal year — within-year hardening. Second, **same-year income** that dampens the first-order capital shock of a catastrophe, feeding realistically into the capital lifecycle. The quoted ROL incorporates the expected reinstatement cost, so the layer's price reflects the reinstatement terms it carries. Reinstatement count and factor are contract calibration in the code.

## Reinsurance — reinsurers as syndicates over portfolios

Outward reinsurance is the primary tail-risk-management mechanism, and the gross/net distinction it creates is load-bearing (regulatory exposure limits bind on net). The design's economy is that **a reinsurer reuses the syndicate substrate**: it is a finite-capital agent with the same portfolio-tail exposure measure, the same insolvency/runoff lifecycle, and the same TP-style pricing — the only difference is that **its risks are other syndicates' aggregate losses rather than single assets**. A reinsurance treaty is structurally a *layer on a primary's portfolio loss*, so excess-of-loss reinsurance needs no bespoke agent class, just the existing machinery pointed at portfolio loss. A separate reinsurer class is redundant.

**Reinsurance contagion (#11) emerges** because treaties are **shared**: one reinsurer's insolvency simultaneously strips cession recoveries from every primary it covered — a correlated secondary shock that can topple primaries whose *gross* exposure was well controlled. A parametric net-retention shortcut (each primary simply retaining a fixed fraction of every loss) cannot express this: with no finite-capital counterparty there is nothing to fail. An exogenous reinsurance price index fails for the same reason. Finite-capital, shared reinsurers are therefore intrinsic to the design, not an add-on.

## Truth versus belief — the cat process and cat models

A permanent, model-wide separation: **the substrate owns the true cat process** — the ground-truth generator of occurrences (frequency, heavy-tailed severity, zone correlation) — and **no agent sees it directly**. Each syndicate owns a **cat model**, a *belief* about that process used to compute its cat ELF and its portfolio tail measure. **A cat model is an estimate of the truth and may be systematically wrong.**

This is not only a #14 feature; it is why the rest of the design hangs together. Cat ELF is model-anchored (a syndicate prices off its belief), yet realised losses come from the true process, so the two can diverge — which is precisely how a miscalibrated cat belief gets punished under selection (#12) and why benign experience must not pull the belief toward a quiet sample.

**Cat-model homogeneity as systemic risk (#14)** emerges from shared *error against the truth*. Two market-level knobs govern it: the **heterogeneity spread** of syndicates' model parameters and a possible **shared bias** (a common offset from the true process). Homogeneous *and* biased models make every syndicate underprice the same tail and fail together — correlated insolvency from a structural cause; widely scattered models let individual errors diversify away. #14 is therefore a controlled experiment in the spread/bias knobs.

It stays orthogonal to herding (#3) **by construction**: #3 is convergence on a shared *price* (and the design anchors price, not belief), while #14 is convergence on a shared *model*. Either runs without inducing the other, which is what makes them separately falsifiable.

## Selection over the parameter genome

Throughout the design, the behaviours that could be miscalibrated are **explicit per-syndicate parameters** rather than fixed constants. Their union is the syndicate **genome**:

- AvT responsiveness — to capacity headroom and to placement feedback — and the **target win-rate (share-appetite)** the feedback loop homeostatically seeks
- herding susceptibility — the scaler on the follower weight `w`
- `hurdle_rate` — the cost-of-capital loading rate
- credibility `k` — the Bühlmann-Straub information-content parameter
- the distribution payout rule
- the reserving bias
- the cat-model parameters — which also drive #14

**Pricing-rule evolution under selection (#12)** emerges from acting on this genome through two channels, both required for *convergence* rather than mere pruning:

1. **Insolvency culling** — a miscalibrated syndicate depletes capital and exits via runoff, removing its genome from the pool.
2. **Success-weighted inheritance at entry** — new capacity adopts parameters resembling profitable incumbents (capital follows what works), with **mutation** for variation. Culling alone only prunes; drawing entrants from a fixed prior would let the distribution be replenished from an arbitrary source and never converge. Inheritance plus mutation is what lets the surviving distribution converge to an attractor.

Over long (200+ year) horizons the genome distribution drifts toward a natural equilibrium, and macro behaviour — cycle amplitude, herding intensity, pricing discipline — becomes an *observed attractor* rather than a designer setting. This is the deepest sense in which the model makes behaviour a finding. Mutation rate and inheritance weighting are market-level evolutionary parameters in the code.

**How the two channels are wired.** Success is a syndicate's own **track record**: an exponentially-weighted mean of its total-return signal per unit of opening capital, so inheritance reads sustained performance rather than one lucky year, and a syndicate that has not traded yet has no record to be copied from. When lagged capital comes online, the parent is drawn from the **solvent, profitable** incumbents with weight `exp(inheritance weighting × track record)` — uniform across the profitable pool at zero weighting, crowded onto the best performers as it rises — and the child's genome is the parent's, **mutated** across every selectable trait (fractionally, except the reserving bias, which is a signed error centred near zero and so mutates additively; the cat model mutates through the three quantities it costs risk with, keeping mutation load-neutral). Insolvency culling is the other half and needs no new machinery: a syndicate on the zero floor is in runoff, so it is neither writing business nor eligible to be a parent, and its genome leaves the pool. When *nothing* in the market is profitable there is no success to imitate and entry falls back to the population prior — replenishment is the degenerate case, not the mechanism.

**Controlled experiments switch inheritance off.** Any arm that holds the population fixed while sweeping another knob — the cat-model homogeneity sweep (#14) above especially — runs with entry back on the fixed prior. With inheritance live an entrant's genome is a function of the run's own history, so the belief population the arm is nominally pinning would drift underneath it and the two experiments would contaminate each other.

The per-year emission carries the genome distribution (population means of the hurdle rate, share-appetite, reserving bias and herding susceptibility, plus the width of the hurdle-rate distribution), because #12 closes on a **human read** of where that distribution goes: a converged population holds its mean between successive half-centuries and keeps its width bounded, while a prior-replenished one is pinned to a source no market outcome can move.

## Premium-flow accounting — the GWP→NEP chain

A syndicate's premium is tracked through the **gross-to-net statement**, not collapsed into a single net expense ratio. Gross written premium (GWP) flows through explicit deductions — **brokerage/acquisition cost**, **management expense**, **outward reinsurance ceded** — to arrive at **net earned premium (NEP)**.

The chain is a design commitment because **every validation target is NEP-denominated** (combined ratio, expense ratio, attritional loss ratio). A single blended expense ratio would conflate brokerage, management cost, and reinsurance cession into one figure, making it impossible to check the model against the empirical *decomposition* and putting the headline combined ratio — the diagnostic for whether the cycle has the right bimodality — on the wrong base. Tracking the chain also makes brokerage a real cost the broker relationship spends against, and ties outward reinsurance ceded into the same statement the gross/net exposure limits read, so accounting and risk machinery stay consistent. Finer cash-flow detail (the Premium Trust Fund as a ring-fenced pool, Lloyd's levies, coverholder ceding-commission mechanics) is not modelled — it bears on no target phenomenon. The rates live in the code.

## Diagnostic invariants — the instruments that gate the findings

Before any phenomenon counts as a finding, the substrate must pass its **diagnostic invariants**. These are not phenomena; they are instrument readings confirming the engine is physically correct, and they gate everything above them.

- **Loss-settlement invariants** (mechanical, checked every settlement): ground-up loss ≤ sum insured; no claim below attachment; no layer pays above its limit; a layer's settled amounts sum across its panel to the layer's insured loss (up to rounding); expired layers generate no claims; a syndicate's payments never take it below zero (the excess is the recorded shortfall).
- **Risk pooling (the central diagnostic)**: read on a pool of identical assets *and* on a **heterogeneous** pool (assets differing in size and in loss-proneness), because pooling is a property of independence, not of uniformity. An insurer's aggregate **attritional** loss is more predictable than any single insured's — the aggregate coefficient of variation falls as ~1/√N as the pool grows — **while the catastrophe component's CV does not compress with pool size**, because a cat is one shared occurrence across the zone. If both halves hold, the attritional/catastrophe distinction is physically real in the model; if either fails, the loss architecture is wrong and nothing built on it is trustworthy.

Only once these hold is the macro behaviour above read as a result rather than an artifact. The quantitative reference ranges these and the phenomena are checked against (combined-ratio bimodality, effective ROL, retention, cat-loss/TIV, and so on) are the empirical targets in the domain material; matching them is downstream calibration, but the *form* of each diagnostic is fixed here.

## Two test lanes — the same split, in the test suite

The two tiers of behaviour above are also the split of the test suite, because they want opposite things from a test run. A **diagnostic invariant** is an exact instrument reading and wants to be instant; a **phenomenon experiment** asserts something about the model's emergent behaviour and wants many seeds and long horizons. Forced into one lane, the fast checks stop being fast and the experiments are held to one seed — and single-seed statistics in this model swing hard enough that a threshold passed on one seed is a seed, not a margin.

- **Fast lane** — `cargo test`. Every unit test, every diagnostic invariant, every settlement and risk-pooling instrument reading, and the report/emission plumbing. This is the loop you run continuously while working.
- **Slow lane** — `cargo test -- --ignored`. The multi-decade phenomenon experiments. Run before opening a PR, and whenever a change could plausibly move market behaviour.

**The classification rule**: does the test step a market for years and assert on *emergent statistics*? Slow lane. Does it call a function and check its output, or step a market and check an invariant that must hold in every year regardless? Fast lane. A long-horizon run asserting only that the physics holds — settlement invariants, placed portions in bounds, the market still trading — stays in the fast lane; it is an instrument reading over a long window, not an experiment.

Slow-lane tests carry `#[ignore = "slow lane: phenomenon experiment — cargo test -- --ignored"]`. Both lanes must pass; the slow lane is not optional, only deferred.

Because the experiments are off the fast lane's critical path, they are read over **seed panels** fixed in advance rather than a single trajectory — a panel chosen before the numbers are seen, with every seed in it counted. A claim that only survives on a hand-picked seed is not a finding.
