# rinsanity — Ubiquitous Language

The shared vocabulary of the `rinsanity` model. Definitions only — no design, no implementation. The domain's institutional terms (insured, asset, peril, occurrence, ground-up loss, layer, syndicate, panel, capital, …) are catalogued in [`domain-source-material.md`](./domain-source-material.md) §1; this file holds the terms whose meaning is project-specific or has been deliberately sharpened, and flags ambiguities.

## Language

**Diagnostic invariant**:
A check that the model's substrate is physically correct — e.g. the loss-settlement invariants, or risk pooling (aggregate attritional CV falling ~1/√N while the catastrophe component does not compress). An instrument reading that confirms the engine works, not a research finding.
_Avoid_: calling these "phenomena".

**Phenomenon**:
An emergent macro behaviour of the market that the model aims to reproduce *without hardcoding it* (the underwriting cycle, capital crises, herding, …). A research target, distinct from a diagnostic invariant.
_Avoid_: using "phenomenon" for substrate-correctness checks.

**Cat process**:
The true, ground-truth generator of catastrophe occurrences (frequency, severity, zone correlation), owned by the substrate. No agent observes it directly.
_Avoid_: conflating with a syndicate's cat model.

**Cat model**:
A syndicate's *belief* about the cat process, used to compute its catastrophe ELF and its portfolio tail measure. An estimate of the cat process that may be systematically wrong.
_Avoid_: treating as ground truth.

**Headroom**:
A syndicate's free exposure budget relative to its capital — how much more risk it can write before hitting its exposure limits. The local state that drives its AvT multiplier.

**Genome**:
A syndicate's vector of selectable per-syndicate parameters (AvT responsiveness, share-appetite, herding susceptibility, hurdle rate, credibility `k`, payout rule, reserving bias, cat-model parameters) on which market selection acts.

**Share-appetite**:
The target win-rate a syndicate's placement-feedback loop seeks: it lifts its AvT multiplier when it wins more than this share and cuts it when it wins less. A selectable genome trait — high appetite chases volume and runs soft, low appetite holds margin.

**Market yield**:
The exogenous, mean-reverting rate at which a syndicate's capital and premium float earn investment return. Macroeconomic and genuinely outside the market — a scenario input the market responds to, never something the market generates.
_Avoid_: treating it as a cycle-phase signal; nothing reads it to decide the market is hard or soft.

**Total-return signal**:
The profitability a syndicate acts on: its underwriting result **plus** its investment income. It is what distributions are released against and what fresh capital reads off the supply curve. Each syndicate's own, computed from its own book and balance — never a market-wide broadcast.
_Avoid_: "return" unqualified when the underwriting result alone is meant.

**IBNR reserve**:
A syndicate's *estimate* of the outstanding liability on a claim that has occurred but not yet run off — the incurred-but-not-reported half of the claim, the near-term settled part being the other. A belief, not a fact: the substrate's true outstanding is the ground truth it develops toward.
_Avoid_: treating the booked reserve as the amount actually owed.

**Reserving bias**:
A syndicate's systematic fractional error in the IBNR reserve it books against its true outstanding liability. Negative is optimistic (under-reserved), positive conservative. The agent's estimation error, a genome trait selection acts on — never a property of the loss.
_Avoid_: calling favourable or adverse development itself a "bias"; the bias is the estimate, the development is the correction.

**Reserve development**:
The movement of a booked IBNR reserve toward the true ultimate as an underwriting year ages. Positive movement is **adverse development** (strengthening, a debit to capital); negative is **favourable development** (a release, a credit). It is the lagged capital impact of a loss that already happened.

**Three-year account**:
An underwriting year's open period: the year of occurrence plus two development years. Its result accumulates as it develops and cannot be distributed while it is open.

**RITC (reinsurance-to-close)**:
The close of an underwriting year at the end of its open period: the remaining estimate crystallises onto the true ultimate and the year's **final distribution** is released. An open year's profit is not distributable — RITC is what gates it.

**Treaty**:
An outward reinsurance contract: an **excess-of-loss layer on a primary's aggregate portfolio loss**, subscribed by a panel of reinsurers. Structurally the same layer-and-panel the substrate already owns, pointed at portfolio loss rather than an asset.
_Avoid_: treating reinsurance as a fixed net-retention fraction; a treaty is a layer with a finite-capital counterparty behind it.

**Reinsurer**:
A syndicate whose inwards risks are other syndicates' aggregate losses. Not a separate agent class — the same finite capital with its zero floor, portfolio tail measure, TP-style pricing, and insolvency-into-runoff lifecycle.

**Retention**:
The primary's attachment on its treaty — the aggregate loss it keeps before the treaty responds. Sized off its own current capital at renewal, ahead of the year's writing.

**Gross / net**:
**Gross** is the loss the substrate actually debited a primary; **net** is what remains after reinsurance recoveries *actually received*. The regulatory exposure limits and the portfolio tail measure bind on **net**; the gross figure is tracked alongside, because net is only as good as the reinsurers standing behind it.
_Avoid_: a single blended figure — the distinction is what makes reinsurance contagion expressible.

**Retained shortfall**:
A recovery a primary was **due but never received**, because the reinsurer ran out of capital. It falls on the **ceding primary** — deliberately a different path from primary insolvency, where the shortfall falls on the *insured*. The retained shortfall is the contagion channel.
_Avoid_: calling it a "default loss"; the reinsurer is not in breach, it is exhausted, and liability is several, not joint.

**Ceded premium**:
The reinsurance premium a primary pays out — an explicit deduction in the gross-to-net statement between gross written and net earned premium, never folded into a blended expense ratio.

**Cat-belief population**:
The market-level distribution a syndicate's *cat model* is drawn from, held separately from the general genome population so belief dispersion can be varied with every other trait pinned. It governs founders and entrants alike.
_Avoid_: folding it into the genome spread — that contaminates the experiment.

**Heterogeneity spread**:
The fractional dispersion of cat models across the population: how much syndicates disagree about the cat process. Zero is perfect homogeneity. It scatters belief at an unchanged population average — it is not a knob on how wrong the market is, only on how much it agrees with itself.

**Shared bias**:
The population's common offset of belief *from the true cat process*, signed so negative is optimistic (catastrophes believed rarer, smaller and lighter-tailed than they are). It is applied when a belief is **constructed**, never read at runtime — no agent ever observes the truth. It is what makes homogeneity dangerous rather than merely uniform: homogeneous-and-correct is harmless, homogeneous-and-biased is the systemic-risk condition.
_Avoid_: calling a single syndicate's model error a "shared bias"; the bias is a property of the population.

**Believed annual cat load**:
The expected catastrophe damage fraction per exposed asset per year *under a syndicate's own cat model* — frequency times the believed mean event damage. The one scalar the two knobs move: a shared bias `b` scales it by `(1 + b)³`, while the heterogeneity spread leaves its population average alone.

**Insured population**:
The market-level distribution the cohort of insureds is drawn from: the mean sum insured, the mean risk aversion, and the **population spread** that disperses them. The demand-side counterpart of the cat-belief population — held as its own market parameter so how much the *risks* differ can be varied with everything else pinned. Its draw runs on its own generator, so changing it changes the risks and nothing else about the world they are offered into.
_Avoid_: treating the insureds as a fixture of the market builder; the cohort's size is a fixture, what the risks *are* is a population draw.

**Population spread**:
The fractional half-width of the mean-preserving dispersion of the insured population, on each dimension an insured differs on: how big its asset is, how loss-prone that asset is, and how much it will pay. Zero is a market of carbon copies. Mean-preserving is the point — the spread scatters the population without moving its average, so the market attritional peril stays the population's mean hazard and the spread is a knob distinct from the calibration it disperses around.

**Loss-proneness**:
An asset's **own** propensity to attritional loss, as a multiple of the market attritional peril: `1.0` is exactly the population average, higher is a chronic loss-generator. Substrate truth on the same footing as the cat process — drawn at market construction, fixed for the asset's life, and read by nothing but the peril that strikes the asset. No agent sees it; the **loss record** is its observable trace, and inferring one from the other is what experience rating (#9) is.
_Avoid_: reading it as a "quality" score pricing may consult; the moment an agent can read it, experience rating has nothing left to discover.

**Track record**:
A syndicate's accumulated success: the exponentially-weighted mean of its own total-return signal per unit of opening capital. It is the measure market selection reads — never a single year's result, and never a market-wide figure. A syndicate that has not yet traded a year has no track record at all.
_Avoid_: conflating with a broker relationship score, which is reputation with a counterparty, not profitability.

**Success-weighted inheritance**:
The channel by which new capacity adopts the genome of incumbents that are making money: a parent is drawn from the *profitable* syndicates in proportion to its track record, and the entrant carries that genome. Capital follows what works. When nothing in the market is profitable there is no success to imitate and the entrant falls back to the genome population prior.
_Avoid_: calling it "cloning" — the child is mutated, and the parent keeps trading.

**Mutation**:
The bounded perturbation applied to an inherited genome at entry, so a child resembles its parent without being it. Without it inheritance collapses the population to clones and selection has nothing left to act on.

**Inheritance weighting**:
The market-level knob on how sharply inheritance crowds onto the best performers: zero imitates the profitable pool uniformly, larger values concentrate the draw on the strongest track records. With the mutation rate, the market's two **evolutionary parameters**.

**Float**:
The premium a syndicate holds between writing business and paying claims on it, which earns the market yield while held.

**Loss record**:
A risk's own history of realised attritional losses, **presented by the broker with the submission**. It travels with the risk, so every syndicate quoting it sees the same record and the credibility volume `n` on the experience modifier is a property of the record, not of the quoting syndicate. It is the observable trace of the asset's loss-proneness, never the loss-proneness itself — that stays substrate truth no agent reads.
_Avoid_: conflating with a syndicate's **own book experience**, which is private, per-syndicate, and drives the credibility blend against the benchmark (#4). The loss record rates the *risk* (#9); the book experience rates the *syndicate's confidence in itself*.
_Avoid_: letting catastrophe losses into it — cat loss costs are model-anchored, never experience-updated.

**Own book experience**:
A syndicate's private record of what it has written in the attritional class and what that book has actually burnt: accumulated exposure-years, the benchmark expectation on them, and the realised loss. It is the `own` half of the credibility blend and the volume `n` that weighs it, so it is what turns accumulated exposure into confidence — specialism is **earned** here (#4). No two syndicates hold the same one.
_Avoid_: conflating with the **loss record**, which is the risk's, is presented by the broker, and is identical in every syndicate's hands. This one is the syndicate's own and is never shown to anyone.
_Avoid_: letting catastrophe losses into it, for the same reason as the loss record.

**Acceptance threshold**:
A follower's **reservation price** on a placement: the lowest firm order it will subscribe to, its own technical view relaxed by its herding weight. This — not a quote — is what price herding moves: a follower anchored toward a reputable lead lowers its threshold and writes business it prices as unprofitable. At zero weight it is exactly the syndicate's own price (strict independent underwriting).
_Avoid_: calling herding a "quote blend" in the context of the subscription decision; measuring an anchored quote against the firm order it is anchored to cancels the weight out entirely. The anchored quote survives as a reporting figure only.

**Herding concession**:
The calibration constant setting how far below its own technical view a *fully* herded follower will write. It is the depth of the reservation-price concession, never a probability of following.

**Lead line**:
The share of a layer a syndicate writes **as lead** — its own `target_line` scaled by the market's lead-line multiple, capped at the whole layer. A lead takes the biggest share on the slip: the domain norm, and the share that carries the reputational signal behind the firm order it set. *How much* a syndicate writes is selectable; *that a lead writes more than it would as a follower* is the placement convention.
_Avoid_: reading it as a separate genome trait — it is derived from `target_line`.

**Panel granularity**:
How finely a layer's limit is divided across its subscribers: many small follower lines against a larger lead line, rather than two large ones. It is the precondition for post-cat concentration (#7), which is *defined* on share redistributing across a panel, for price herding (#3), which needs a population of followers to cluster, and for counter-cyclical capacity supply (#6), which routes through how many syndicates can participate in a placement at all. Its cost is that granular books are diversified books: the market absorbs shocks a lumpy panel could not.

**Placed portion**:
How much of a layer's limit its panel actually subscribed. Below one is **partial placement** — the insured restructures or retains the gap. At Lloyd's-scale lines a layer fills only when enough followers subscribe, so a placed portion below one is a standing feature of the market, not an edge case.

**Concessive subscription**:
A subscription bound at a firm order **below the subscriber's own price** — a follower writing at the lead's terms on the strength of who set them. The countable trace of a lead's mispricing propagating across a panel, emitted per year.
_Avoid_: reading it as a discount given to the insured; the insured pays the firm order either way, and it is the follower's margin, not the price, that moves.

**Fast lane / slow lane**:
The two halves of the test suite. The **fast lane** (`cargo test`) is every unit test and **diagnostic invariant** — instrument readings that must be instant, so they can be run continuously. The **slow lane** (`cargo test -- --ignored`) is the **phenomenon experiments** — multi-decade runs read over seed panels, run before opening a PR. The split rule is the tier the test belongs to: a test that steps a market for years and asserts on *emergent statistics* is an experiment; one that checks a function's output, or an invariant that must hold in every year regardless, is an instrument reading.
_Avoid_: reading the slow lane as optional — it is deferred, not skipped, and both lanes must pass.

**Seed panel**:
The set of seeds a phenomenon experiment is read over, **fixed in advance** and counted in full. A phenomenon's claim is about a population of runs, so it is stated distributionally over the panel — a median, a mean, or a count of seeds clearing a bar — never as a threshold one trajectory happens to pass.
_Avoid_: widening or narrowing a panel after seeing the numbers; a claim that survives only on a chosen panel is not a finding.

## Flagged ambiguities

- **"Phenomenon"** was historically used for both substrate checks (e.g. risk pooling, §5 #0) and genuine emergent market behaviours. Resolved: substrate checks are **diagnostic invariants**; only emergent market behaviours are **phenomena**.
