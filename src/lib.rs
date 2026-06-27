//! rinsanity substrate — the loss architecture and placement/settlement cascade.
//!
//! Perils generate ground-up loss; a coverage request is satisfied by a tower of
//! layers, each placed on a subscription panel; the settlement cascade pro-rates
//! each penetrated layer's insured loss across its panel against the zero floor.
//!
//! See `docs/system-design/README.md` (*Loss architecture*, *Layers and towers*,
//! *Panels and the settlement cascade*, *Capital, insolvency, and runoff*,
//! *Diagnostic invariants*) and `CONTEXT.md` for the vocabulary used here.

/// The ground-up loss of an occurrence on an asset: the physical damage,
/// independent of any insurance. `GUL = damage_fraction × sum_insured`, and
/// the damage fraction lives in `[0, 1]` so `GUL ≤ sum_insured` (the physical
/// cap, per asset).
pub fn ground_up_loss(damage_fraction: f64, sum_insured: f64) -> f64 {
    let capped_fraction = damage_fraction.clamp(0.0, 1.0);
    capped_fraction * sum_insured
}

/// A layer `[attachment, attachment + limit]`: the band of ground-up loss a
/// contract covers. The unit of placement. A full-value policy is the
/// degenerate tower of one layer with `attachment = 0` and `limit = sum_insured`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub attachment: f64,
    pub limit: f64,
}

impl Layer {
    /// A full-value layer over an asset: attachment 0, limit equal to the
    /// asset's sum insured.
    pub fn full_value(sum_insured: f64) -> Self {
        Layer { attachment: 0.0, limit: sum_insured }
    }

    /// The insured loss this layer bears for a given ground-up loss:
    /// `gross = min(GUL, limit)`, `net = gross − attachment` (zero if
    /// `GUL ≤ attachment`). No claim arises below attachment; no layer pays
    /// above its limit.
    pub fn insured_loss(&self, ground_up_loss: f64) -> f64 {
        let gross = ground_up_loss.min(self.attachment + self.limit);
        (gross - self.attachment).max(0.0)
    }
}

/// A tower: a vertical stack of consecutive layers `[0, a₁], [a₁, a₂], …`, each
/// an independent contract over the same underlying ground-up loss. The unit a
/// coverage request is satisfied by. A single occurrence's GUL flows up the
/// tower deterministically — each layer bears `clamp(GUL − attachment, 0, limit)`
/// — so the tower's aggregate payout is `min(GUL, top of tower)` and never
/// exceeds the ground-up loss.
#[derive(Debug, Clone, PartialEq)]
pub struct Tower {
    pub layers: Vec<Layer>,
}

impl Tower {
    /// A tower from a stack of layers, bottom (lowest attachment) first.
    pub fn new(layers: Vec<Layer>) -> Self {
        Tower { layers }
    }

    /// The insured loss each layer bears for a given ground-up loss, in tower
    /// order. Each layer is independent: GUL flows up via [`Layer::insured_loss`].
    pub fn insured_losses(&self, ground_up_loss: f64) -> Vec<f64> {
        self.layers.iter().map(|layer| layer.insured_loss(ground_up_loss)).collect()
    }

    /// The tower's total insured loss across all its layers for a given
    /// ground-up loss. For a stack of consecutive layers this is
    /// `min(GUL, top of tower)`, so it never exceeds the ground-up loss.
    pub fn aggregate_insured_loss(&self, ground_up_loss: f64) -> f64 {
        self.insured_losses(ground_up_loss).iter().sum()
    }
}

/// One draw from a Pareto-style severity law, `x_m · U^(−1/α)` with `U` uniform
/// on `(0, 1]`, clamped into `[0, 1]`. The shared kernel of the true cat
/// process's severity and any cat-model belief's severity: a heavy body with
/// bounded support (`GUL ≤ sum_insured` survives). Consumes one uniform draw.
fn pareto_damage_fraction(min_damage_fraction: f64, tail_alpha: f64, rng: &mut Rng) -> f64 {
    // 1 − uniform() lands in (0, 1], avoiding a divide-by-zero blow-up.
    let u = 1.0 - rng.uniform();
    (min_damage_fraction * u.powf(-1.0 / tail_alpha)).clamp(0.0, 1.0)
}

/// A Poisson count with the given `mean`, drawn by Knuth's algorithm from the
/// in-crate uniform stream. The shared kernel of the true cat process's and a
/// cat-model belief's annual event count.
fn poisson_count(mean: f64, rng: &mut Rng) -> usize {
    let threshold = (-mean).exp();
    let mut count = 0usize;
    let mut product = 1.0;
    loop {
        product *= rng.uniform();
        if product <= threshold {
            return count;
        }
        count += 1;
    }
}

/// A geographic / peril zone. The market spans multiple territories; each
/// carries its own peril processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Territory(pub u32);

/// A unit of economic value owned by an insured: a `sum_insured` (replacement
/// value, the ceiling on any single-occurrence loss) sitting in a `territory`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Asset {
    pub sum_insured: f64,
    pub territory: Territory,
}

/// The attritional peril: many small, statistically independent occurrences.
/// Attritional occurrences are drawn **independently per asset**, so their
/// aggregate pools (the heart of the risk-pooling diagnostic invariant).
#[derive(Debug, Clone, Copy)]
pub struct AttritionalPeril {
    /// Probability that an attritional occurrence strikes a given asset in a period.
    pub occurrence_probability: f64,
    /// Mean damage fraction of an occurrence, given one happens. Severity is
    /// drawn uniformly on `[0, 2 × mean]` (clamped into `[0, 1]`), giving a
    /// finite-variance body.
    pub mean_damage_fraction: f64,
}

impl AttritionalPeril {
    /// Draw this period's ground-up loss for a single asset from an independent
    /// occurrence. Returns 0 when no occurrence strikes the asset. Consumes two
    /// draws (occurrence, then severity) so successive assets get independent
    /// occurrences.
    pub fn strike(&self, asset: &Asset, rng: &mut Rng) -> f64 {
        let occurs = rng.uniform() < self.occurrence_probability;
        let severity = rng.uniform(); // drawn regardless, to keep the stream aligned
        if occurs {
            let damage_fraction = severity * 2.0 * self.mean_damage_fraction;
            ground_up_loss(damage_fraction, asset.sum_insured)
        } else {
            0.0
        }
    }
}

/// The catastrophe peril: a territory's true **cat process** (the ground-truth
/// generator of occurrences — frequency, heavy-tailed severity). A catastrophe
/// is a rare, large event that strikes *all* exposed assets in a territory as a
/// single shared occurrence, so its losses are correlated by construction.
///
/// This is the substrate's truth, not any syndicate's belief (a *cat model*);
/// no agent observes it directly.
#[derive(Debug, Clone, Copy)]
pub struct CatastrophePeril {
    /// Expected number of catastrophe events per year in the territory (the mean
    /// of a Poisson arrival count, so multiple events in one year are possible).
    pub annual_frequency: f64,
    /// The Pareto scale `x_m`: the minimum (and most probable) damage fraction.
    pub min_damage_fraction: f64,
    /// Pareto tail index `α`. Smaller is heavier: the empirical US-hurricane
    /// damage tail sits near `α ≈ 1.25–1.52` (near-infinite variance), the
    /// regime that drives cat-year capital volatility.
    pub tail_alpha: f64,
}

impl CatastrophePeril {
    /// Draw one event's uniform damage fraction from a Pareto-style law,
    /// `x_m · U^(−1/α)` with `U` uniform on `(0, 1]`, clamped into `[0, 1]`. The
    /// result has a heavy body but bounded support, so `GUL ≤ sum_insured`
    /// holds while the tail stays empirically fat.
    pub fn draw_damage_fraction(&self, rng: &mut Rng) -> f64 {
        pareto_damage_fraction(self.min_damage_fraction, self.tail_alpha, rng)
    }

    /// The number of catastrophe events arriving in one year, a Poisson count
    /// with mean [`annual_frequency`](Self::annual_frequency).
    fn annual_event_count(&self, rng: &mut Rng) -> usize {
        poisson_count(self.annual_frequency, rng)
    }

    /// Draw this year's catastrophe events for the territory: a Poisson number
    /// of single shared occurrences, each placed at a uniform within-year time
    /// and carrying its own heavy-tailed damage fraction. Returned in
    /// chronological order.
    pub fn annual_events(&self, rng: &mut Rng) -> Vec<CatastropheEvent> {
        let count = self.annual_event_count(rng);
        let mut events: Vec<CatastropheEvent> = (0..count)
            .map(|_| CatastropheEvent {
                time: rng.uniform(),
                damage_fraction: self.draw_damage_fraction(rng),
            })
            .collect();
        events.sort_by(|a, b| a.time.partial_cmp(&b.time).expect("times are finite"));
        events
    }
}

/// A single catastrophe occurrence in a territory: a point on the within-year
/// time axis carrying the **uniform damage fraction** the event inflicts on
/// every exposed asset in the struck territory at once. The "shared occurrence"
/// of the risk-pooling diagnostic — contrast with attritional, drawn
/// independently per asset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CatastropheEvent {
    /// When in the year the event falls, as a fraction of the year in `[0, 1)`.
    pub time: f64,
    /// The damage fraction applied uniformly to every exposed asset, in `[0, 1]`
    /// so `GUL ≤ sum_insured` per asset.
    pub damage_fraction: f64,
}

/// The insurer's aggregate catastrophe ground-up loss over a pool of assets in
/// one territory across a year's catastrophe events. Each event is a single
/// shared occurrence: its uniform damage fraction strikes every asset, whose
/// ground-up loss flows through a full-value layer. Because the draw is shared,
/// the aggregate is `Σ_events damage_fraction × Σ_assets sum_insured` — the
/// total exposure scales the loss but does not diversify the event severity,
/// which is why the catastrophe-component CV is flat in pool size.
pub fn territory_catastrophe_loss(assets: &[Asset], events: &[CatastropheEvent]) -> f64 {
    events
        .iter()
        .map(|event| {
            assets
                .iter()
                .map(|asset| {
                    let gul = ground_up_loss(event.damage_fraction, asset.sum_insured);
                    Layer::full_value(asset.sum_insured).insured_loss(gul)
                })
                .sum::<f64>()
        })
        .sum()
}

/// Sample the insurer's aggregate catastrophe loss over `trials` independent
/// years for a market of `territories` uncorrelated territories, each holding a
/// fresh pool of `pool_size_per_territory` identical assets and running its own
/// independent cat process. Returns one aggregate-loss figure per year.
///
/// The instrument the catastrophe risk-pooling diagnostic reads: holding
/// `territories = 1` and growing the pool shows the cat CV is flat in pool size
/// (shared draw); holding total exposure fixed and growing `territories` shows
/// it falls ~1/√T (diversification across uncorrelated zones).
pub fn catastrophe_aggregate_samples(
    territories: usize,
    pool_size_per_territory: usize,
    sum_insured: f64,
    peril: &CatastrophePeril,
    trials: usize,
    rng: &mut Rng,
) -> Vec<f64> {
    let zones: Vec<Vec<Asset>> = (0..territories)
        .map(|z| {
            (0..pool_size_per_territory)
                .map(|_| Asset { sum_insured, territory: Territory(z as u32) })
                .collect()
        })
        .collect();
    (0..trials)
        .map(|_| {
            zones
                .iter()
                .map(|assets| {
                    // Each territory draws its own independent catastrophe events.
                    let events = peril.annual_events(rng);
                    territory_catastrophe_loss(assets, &events)
                })
                .sum()
        })
        .collect()
}

/// The insurer's aggregate attritional ground-up loss over a pool of assets in
/// one period: each asset is struck by an independent attritional occurrence,
/// whose ground-up loss flows through a full-value layer (attachment 0,
/// limit = sum insured) — a degenerate tower of one on a panel of one. Because
/// the layer is full-value and `GUL ≤ sum_insured`, each asset's insured loss
/// equals its ground-up loss, and the aggregate is their sum.
pub fn aggregate_attritional_loss(assets: &[Asset], peril: &AttritionalPeril, rng: &mut Rng) -> f64 {
    assets
        .iter()
        .map(|asset| {
            let gul = peril.strike(asset, rng);
            Layer::full_value(asset.sum_insured).insured_loss(gul)
        })
        .sum()
}

/// Run a syndicate's persistent capital through a multi-year horizon of
/// attritional losses on a fixed pool. Each year's aggregate insured loss is
/// settled against capital, which carries over between years with no annual
/// re-endowment. Returns the capital balance at the end of each year.
pub fn run_attritional_horizon(
    syndicate: &mut Syndicate,
    assets: &[Asset],
    peril: &AttritionalPeril,
    years: u32,
    rng: &mut Rng,
) -> Vec<f64> {
    (0..years)
        .map(|_| {
            let annual_loss = aggregate_attritional_loss(assets, peril, rng);
            syndicate.settle(annual_loss);
            syndicate.capital()
        })
        .collect()
}

/// The coefficient of variation of a sample: `stddev / mean` (population
/// standard deviation). The dimensionless dispersion measure the risk-pooling
/// diagnostic is read on. Returns 0 for an empty or zero-mean sample.
pub fn coefficient_of_variation(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let n = samples.len() as f64;
    let mean = samples.iter().sum::<f64>() / n;
    if mean == 0.0 {
        return 0.0;
    }
    let variance = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    variance.sqrt() / mean
}

/// Sample the insurer's aggregate attritional loss over `trials` independent
/// years for a fresh pool of `pool_size` identical assets, returning one
/// aggregate-loss figure per year. The instrument the risk-pooling diagnostic
/// reads: feeding these samples to [`coefficient_of_variation`] gives the
/// aggregate CV at that pool size.
pub fn attritional_aggregate_samples(
    pool_size: usize,
    sum_insured: f64,
    peril: &AttritionalPeril,
    trials: usize,
    rng: &mut Rng,
) -> Vec<f64> {
    let assets: Vec<Asset> = (0..pool_size)
        .map(|_| Asset { sum_insured, territory: Territory(0) })
        .collect();
    (0..trials)
        .map(|_| aggregate_attritional_loss(&assets, peril, rng))
        .collect()
}

/// A small, deterministic, seedable PRNG (SplitMix64). Hand-rolled in-crate so
/// the substrate's stochastic draws are reproducible from a seed and the model
/// carries no external dependencies.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Seed the generator.
    pub fn seeded(seed: u64) -> Self {
        Rng { state: seed }
    }

    /// Next raw 64-bit value (SplitMix64).
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform draw in `[0, 1)`.
    pub fn uniform(&mut self) -> f64 {
        // Use the top 53 bits for a double in [0, 1).
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A syndicate's loss-absorbing capital: a persistent balance. Claim shares
/// debit it and the balance carries over between years with no annual
/// re-endowment. Capital has a hard floor at zero — a settlement pays
/// `min(share, remaining capital)` and any remainder is an unrecovered
/// shortfall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Syndicate {
    capital: f64,
}

/// The outcome of debiting a syndicate's capital for a claim share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settlement {
    /// The amount actually paid (debited from capital).
    pub settled: f64,
    /// The portion of the claim the syndicate could not cover (zero while solvent).
    pub shortfall: f64,
}

impl Syndicate {
    /// A syndicate endowed with an initial capital balance.
    pub fn with_capital(capital: f64) -> Self {
        Syndicate { capital }
    }

    /// Current capital balance.
    pub fn capital(&self) -> f64 {
        self.capital
    }

    /// Whether the syndicate still has loss-absorbing capital.
    pub fn is_solvent(&self) -> bool {
        self.capital > 0.0
    }

    /// Settle a claim share against capital. Debits `min(share, capital)` so
    /// capital never goes below zero; the uncovered remainder is the shortfall.
    pub fn settle(&mut self, claim_share: f64) -> Settlement {
        let settled = claim_share.min(self.capital).max(0.0);
        self.capital -= settled;
        Settlement { settled, shortfall: claim_share - settled }
    }

    /// Credit premium income to capital. The balance has no ceiling; this is the
    /// mirror of [`settle`](Self::settle) used for reinstatement income, which
    /// credits the panel's capital in the same year as the loss it follows.
    pub fn credit(&mut self, amount: f64) {
        self.capital += amount.max(0.0);
    }
}

/// A syndicate's identity within the market's roster — an index into the slice
/// of syndicates a placement is settled against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyndicateId(pub usize);

/// A single subscription on a panel: a syndicate and the fraction of the layer
/// it has taken. Shares across a panel sum to the placed portion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelEntry {
    pub syndicate: SyndicateId,
    pub share: f64,
}

/// A subscription panel: an ordered set of `(syndicate, share)` entries whose
/// shares sum to the placed portion, with the first entry designated **lead**
/// and the rest **followers**. Single-syndicate placement is a panel of one, not
/// a separate mode. The substrate owns the panel representation and the pro-rata
/// settlement; how a panel is formed or priced is agent logic in higher layers.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub entries: Vec<PanelEntry>,
}

impl Panel {
    /// The trivial deterministic placement rule used until pricing exists: split
    /// the `placed_portion` into equal shares over the shortlisted syndicates in
    /// order, designating the first as lead and the rest as followers. The
    /// shortlist order is preserved.
    pub fn subscribe(syndicates: &[SyndicateId], placed_portion: f64) -> Self {
        let n = syndicates.len();
        let share = if n == 0 { 0.0 } else { placed_portion / n as f64 };
        let entries = syndicates
            .iter()
            .map(|&syndicate| PanelEntry { syndicate, share })
            .collect();
        Panel { entries }
    }

    /// The lead — the first entry on the panel.
    pub fn lead(&self) -> &PanelEntry {
        &self.entries[0]
    }

    /// The followers — every entry after the lead.
    pub fn followers(&self) -> &[PanelEntry] {
        &self.entries[1..]
    }

    /// The placed portion of the layer: the sum of the panel's shares.
    pub fn placed_portion(&self) -> f64 {
        self.entries.iter().map(|e| e.share).sum()
    }

    /// Pro-rate a layer's net insured loss across the panel by share, debiting
    /// each subscriber's capital its share (`share × insured_loss`) against the
    /// zero floor. Returns one settlement per entry, in panel order.
    ///
    /// Liability is **several, not joint**: each member settles its own share
    /// independently, so an insolvent member's shortfall falls on the insured and
    /// is never redistributed to co-subscribers. On a fully placed panel the
    /// settled amounts sum to the insured loss (up to rounding).
    pub fn settle(&self, insured_loss: f64, syndicates: &mut [Syndicate]) -> Vec<Settlement> {
        self.entries
            .iter()
            .map(|entry| syndicates[entry.syndicate.0].settle(entry.share * insured_loss))
            .collect()
    }

    /// Credit an amount across the panel by share — the mirror of [`settle`](Self::settle).
    /// Each subscriber's capital is credited `share × amount`; returns one credit
    /// per entry in panel order. Reinstatement income is pro-rated by the same
    /// shares as the loss that triggered it, so the panel's capital is credited in
    /// the same year as the loss, consistent with the settlement cascade.
    pub fn credit(&self, amount: f64, syndicates: &mut [Syndicate]) -> Vec<f64> {
        self.entries
            .iter()
            .map(|entry| {
                let credit = entry.share * amount;
                syndicates[entry.syndicate.0].credit(credit);
                credit
            })
            .collect()
    }
}

/// The syndicates available to write new business: the solvent members of the
/// roster, in roster order. An insolvent syndicate is in **runoff** — it writes
/// no new business — so it is excluded here, while [`settle_placed_tower`] still
/// settles its in-force layers until they expire.
pub fn available_for_new_business(syndicates: &[Syndicate]) -> Vec<SyndicateId> {
    syndicates
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_solvent())
        .map(|(i, _)| SyndicateId(i))
        .collect()
}

/// The per-syndicate **distribution rule** (part of the genome): how a syndicate
/// releases profit to its capital providers at year-end. `payout_fraction` is the
/// share of the year's underwriting profit it pays out; `solvency_floor` is the
/// capital level below which it distributes nothing and rebuilds first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistributionParams {
    /// Fraction of a profitable year's result released to capital providers.
    pub payout_fraction: f64,
    /// Capital level below which distributions are suppressed (rebuild first).
    pub solvency_floor: f64,
}

/// The **year-end distribution**: the amount of capital a syndicate releases to its
/// providers. Profit is released only when the year was profitable and capital sits
/// at or above the [`solvency_floor`](DistributionParams::solvency_floor); the
/// release is the [`payout_fraction`](DistributionParams::payout_fraction) of the
/// year's `year_result`, **capped so it never drives capital below the floor**.
///
/// Distributions are **suppressed in loss years and while impaired** — an impaired
/// syndicate rebuilds before it pays out. This is the only check on unbounded
/// capital accumulation: releasing recovered capital is exactly what competes AvT
/// back down and lets a hard market re-soften, closing the loop from hard to soft.
pub fn distribution(capital: f64, year_result: f64, params: &DistributionParams) -> f64 {
    if year_result <= 0.0 || capital < params.solvency_floor {
        return 0.0;
    }
    let desired = params.payout_fraction * year_result;
    let above_floor = (capital - params.solvency_floor).max(0.0);
    desired.min(above_floor)
}

/// A placed layer: a layer bound to a subscription panel for an annual term with
/// explicit per-policy inception and expiry dates. A tower is a stack of these.
/// The substrate honours whatever dates a policy carries; it never decides them.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLayer {
    pub layer: Layer,
    pub panel: Panel,
    /// When cover incepts, as a within-year fraction.
    pub inception: f64,
    /// When cover expires (exclusive), as a within-year fraction.
    pub expiry: f64,
}

impl PlacedLayer {
    /// Whether the layer is on risk at `date`: `inception ≤ date < expiry`.
    /// Outside this window the layer generates no claims.
    pub fn is_in_force(&self, date: f64) -> bool {
        self.inception <= date && date < self.expiry
    }
}

/// The settlement cascade for a placed tower: flow a ground-up loss up the tower
/// at a `date`, settling each **in-force** layer's net insured loss on its panel
/// against the roster of `syndicates`. Expired (or not-yet-incepted) layers
/// generate no claims and are skipped. Returns the settlements of every panel
/// entry that was debited, in tower-then-panel order.
pub fn settle_placed_tower(
    tower: &[PlacedLayer],
    ground_up_loss: f64,
    date: f64,
    syndicates: &mut [Syndicate],
) -> Vec<Settlement> {
    let mut settlements = Vec::new();
    for placed in tower {
        if !placed.is_in_force(date) {
            continue;
        }
        let insured_loss = placed.layer.insured_loss(ground_up_loss);
        settlements.extend(placed.panel.settle(insured_loss, syndicates));
    }
    settlements
}

/// **Reinstatement terms** on a catastrophe excess-of-loss layer: how many times
/// the eroded limit can be restored within the term (`count`) and the premium
/// `factor` charged per full reinstatement (≈1.0 = 100% of the original layer
/// premium), pro-rated to the fraction of limit reinstated.
///
/// A layer's aggregate cover over its term is therefore `(1 + count) × limit`: the
/// original ("free") limit plus `count` reinstatements. Loss paid above the
/// original limit consumes reinstatements and is charged pro-rata; once the
/// aggregate is exhausted the layer stays eroded. [`none`](Self::none) is a layer
/// with no reinstatement cover — a single limit for the term, no charge ever.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReinstatementTerms {
    pub count: u32,
    pub factor: f64,
}

impl ReinstatementTerms {
    /// A layer with no reinstatement: one limit for the term, no reinstatement
    /// premium ever charged.
    pub fn none() -> Self {
        ReinstatementTerms { count: 0, factor: 0.0 }
    }

    /// The expected reinstatement premium per unit of original premium, given the
    /// expected fraction of the original limit reinstated over a year. The
    /// dimensionless loading the quoted price folds in: `factor × reinstated_fraction`.
    pub fn premium_loading(&self, expected_reinstated_fraction: f64) -> f64 {
        self.factor * expected_reinstated_fraction
    }
}

/// The outcome of a cat XoL layer absorbing one event within its term: the claim
/// settlements across its panel; the `reinstatement_premium` the event triggered
/// (zero while the original limit still covers it); the per-entry capital credits
/// that reinstatement income paid the panel (pro-rata by share, in the same year);
/// and any `uncovered` loss left once the layer's aggregate cover is exhausted.
#[derive(Debug, Clone, PartialEq)]
pub struct EventSettlement {
    pub claim: Vec<Settlement>,
    pub reinstatement_premium: f64,
    pub reinstatement_credits: Vec<f64>,
    pub uncovered: f64,
}

/// A placed catastrophe excess-of-loss layer with **reinstatement cover**, tracking
/// its within-year erosion state. The band sits on a `panel` for a term bounded by
/// `inception`/`expiry`; `terms` carry the reinstatement count and factor; and
/// `premium` is the original layer premium the reinstatement premium is a fraction
/// of. The live `consumed` state is the cumulative layer loss paid this term.
///
/// The aggregate cover is `(1 + count) × limit`. The original limit is free; loss
/// paid above it consumes reinstatements, each charged `factor × premium ×
/// (reinstated / limit)`, **crediting the panel's capital in the same year as the
/// loss** (see [`Panel::credit`]). Once `(1 + count) × limit` is exhausted the
/// layer stays eroded — a later event finds reduced or zero limit. That finite,
/// event-ordered erosion is the source of within-year hardening: a clustered
/// second event triggers a further reinstatement (or finds the layer spent).
#[derive(Debug, Clone, PartialEq)]
pub struct ReinstatementLayer {
    pub layer: Layer,
    pub panel: Panel,
    pub terms: ReinstatementTerms,
    pub premium: f64,
    pub inception: f64,
    pub expiry: f64,
    consumed: f64,
}

impl ReinstatementLayer {
    /// A freshly placed cat XoL layer: full aggregate cover, nothing consumed.
    pub fn new(
        layer: Layer,
        panel: Panel,
        terms: ReinstatementTerms,
        premium: f64,
        inception: f64,
        expiry: f64,
    ) -> Self {
        ReinstatementLayer { layer, panel, terms, premium, inception, expiry, consumed: 0.0 }
    }

    /// Whether the layer is on risk at `date`: `inception ≤ date < expiry`.
    pub fn is_in_force(&self, date: f64) -> bool {
        self.inception <= date && date < self.expiry
    }

    /// The layer's aggregate cover over its term: `(1 + count) × limit`.
    pub fn aggregate_limit(&self) -> f64 {
        (1.0 + self.terms.count as f64) * self.layer.limit
    }

    /// The cover still available this term: `aggregate_limit − consumed`.
    pub fn remaining_limit(&self) -> f64 {
        (self.aggregate_limit() - self.consumed).max(0.0)
    }

    /// Absorb one catastrophe event (its ground-up loss) at `date`, settling the
    /// layer's claim on the panel and crediting any reinstatement premium back to
    /// the panel's capital in the same year. Loss paid above the original limit
    /// consumes reinstatements (charged pro-rata) until the aggregate cover is
    /// exhausted, after which the excess is uncovered. An out-of-force layer
    /// generates nothing.
    pub fn absorb_event(
        &mut self,
        ground_up_loss: f64,
        date: f64,
        syndicates: &mut [Syndicate],
    ) -> EventSettlement {
        if !self.is_in_force(date) {
            return EventSettlement {
                claim: Vec::new(),
                reinstatement_premium: 0.0,
                reinstatement_credits: Vec::new(),
                uncovered: 0.0,
            };
        }
        let demand = self.layer.insured_loss(ground_up_loss);
        let payable = demand.min(self.remaining_limit());

        // The portion of this event's payable loss that falls above the original
        // (free) limit consumes reinstatement and is charged pro-rata to the
        // fraction of limit reinstated.
        let limit = self.layer.limit;
        let before = self.consumed;
        let after = self.consumed + payable;
        let reinstated = (after - limit).max(0.0) - (before - limit).max(0.0);
        let reinstatement_premium = if limit > 0.0 {
            self.terms.factor * self.premium * (reinstated / limit)
        } else {
            0.0
        };
        self.consumed = after;

        let claim = self.panel.settle(payable, syndicates);
        let reinstatement_credits = self.panel.credit(reinstatement_premium, syndicates);
        EventSettlement {
            claim,
            reinstatement_premium,
            reinstatement_credits,
            uncovered: demand - payable,
        }
    }

    /// Absorb a year's catastrophe `events` over a net `exposure`, **in the
    /// chronological order they arrive** (the order [`CatastrophePeril::annual_events`]
    /// returns). Each event's shared occurrence strikes the whole exposure
    /// (`gul = damage_fraction × exposure`) and flows through [`absorb_event`](Self::absorb_event),
    /// so a second event within the year triggers a further reinstatement — or
    /// finds the layer already eroded. Returns one [`EventSettlement`] per event.
    pub fn absorb_year(
        &mut self,
        events: &[CatastropheEvent],
        exposure: f64,
        syndicates: &mut [Syndicate],
    ) -> Vec<EventSettlement> {
        events
            .iter()
            .map(|event| self.absorb_event(event.damage_fraction * exposure, event.time, syndicates))
            .collect()
    }
}

/// A syndicate's **cat model**: its *belief* about the true cat process, used to
/// estimate its catastrophe ELF and its portfolio tail measure. Structurally it
/// mirrors the substrate's [`CatastrophePeril`] (frequency, heavy-tailed
/// severity), but it is a **distinct type** precisely so the truth/belief
/// separation is enforced at compile time: the portfolio tail measure consumes a
/// `CatModel`, never a `CatastrophePeril`, so no agent can read the true process.
/// A cat model is an estimate and may be systematically wrong.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CatModel {
    /// The syndicate's believed expected number of catastrophe events per year
    /// in a zone (mean of a Poisson arrival count).
    pub annual_frequency: f64,
    /// The believed Pareto scale `x_m`: the minimum (most probable) damage fraction.
    pub min_damage_fraction: f64,
    /// The believed Pareto tail index `α`. Smaller is heavier.
    pub tail_alpha: f64,
}

impl CatModel {
    /// Draw one event's believed damage fraction from the model's Pareto law,
    /// `x_m · U^(−1/α)` with `U` uniform on `(0, 1]`, clamped into `[0, 1]`.
    pub fn draw_damage_fraction(&self, rng: &mut Rng) -> f64 {
        pareto_damage_fraction(self.min_damage_fraction, self.tail_alpha, rng)
    }

    /// The believed number of catastrophe events in one year for a zone: a
    /// Poisson count with mean [`annual_frequency`](Self::annual_frequency).
    fn annual_event_count(&self, rng: &mut Rng) -> usize {
        poisson_count(self.annual_frequency, rng)
    }
}

/// A single **net retained line** the syndicate holds: the most it can lose on
/// one risk after outward reinsurance, sitting in a `territory`. For a
/// catastrophe the whole net line is exposed to the zone's shared occurrence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetLine {
    pub territory: Territory,
    pub net_limit: f64,
}

/// A syndicate's **net book**: the net retained lines it currently holds. The
/// object the portfolio tail measure is computed over.
#[derive(Debug, Clone, PartialEq)]
pub struct NetBook {
    pub lines: Vec<NetLine>,
}

impl NetBook {
    /// The distinct territories the book has exposure in, in first-seen order.
    pub fn zones(&self) -> Vec<Territory> {
        let mut zones: Vec<Territory> = Vec::new();
        for line in &self.lines {
            if !zones.contains(&line.territory) {
                zones.push(line.territory);
            }
        }
        zones
    }

    /// The total net retained exposure the book carries in one zone — the sum of
    /// its net lines there. A catastrophe's shared occurrence strikes this whole
    /// aggregate at once, so within-zone exposure adds.
    pub fn zone_exposure(&self, territory: Territory) -> f64 {
        self.lines
            .iter()
            .filter(|l| l.territory == territory)
            .map(|l| l.net_limit)
            .sum()
    }
}

/// The syndicate's **portfolio tail measure**: its estimate of the net aggregate
/// catastrophe loss at a chosen `return_period` (e.g. 200 for 1-in-200) over its
/// current net `book`, computed from its OWN [`CatModel`] belief — never the true
/// process — and accounting for **zone correlation**.
///
/// Within a zone a catastrophe is one shared occurrence, so the year's events
/// each strike the zone's whole aggregate exposure and the losses **add**; across
/// zones the cat processes are independent, so their losses **diversify**. The
/// measure Monte-Carlo simulates `trials` independent years (each zone drawing
/// its own believed events) and returns the `1 − 1/return_period` quantile of the
/// aggregate net loss. Deterministic given a seeded `rng`.
pub fn portfolio_tail_loss(
    book: &NetBook,
    model: &CatModel,
    return_period: f64,
    trials: usize,
    rng: &mut Rng,
) -> f64 {
    let zones: Vec<f64> = book.zones().iter().map(|&z| book.zone_exposure(z)).collect();
    if zones.is_empty() || trials == 0 {
        return 0.0;
    }
    let aggregates: Vec<f64> = (0..trials)
        .map(|_| {
            zones
                .iter()
                .map(|&exposure| {
                    // One year's believed events in this zone. Each event is a
                    // single shared occurrence striking the whole zone aggregate;
                    // within-zone losses add. Net loss per event is capped at the
                    // zone's exposure (damage fraction ≤ 1, per the physical cap).
                    let count = model.annual_event_count(rng);
                    (0..count)
                        .map(|_| model.draw_damage_fraction(rng) * exposure)
                        .sum::<f64>()
                })
                .sum::<f64>() // cross-zone: independent draws diversify
        })
        .collect();
    tail_quantile(aggregates, return_period)
}

/// Why a syndicate **declined** a risk under its exposure limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclineReason {
    /// The syndicate has no loss-absorbing capital (it is in runoff).
    Insolvent,
    /// The net line exceeds the per-risk line limit (`line_fraction × capital`).
    PerRiskLine,
    /// Adding the risk would push the portfolio return-period net loss beyond the
    /// coverable cat aggregate (`solvency_fraction × capital`).
    CatAggregate,
}

/// A syndicate's quote-time underwriting decision under its exposure limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderwritingDecision {
    Accept,
    Decline(DeclineReason),
}

/// A syndicate's **exposure policy**: the calibration of its two capital-linked
/// limits, both recomputed from *current* capital at quote time. Because both
/// scale with capital, a post-cat drawdown tightens them together — the capacity
/// crunch and the hardening are two faces of one depletion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExposurePolicy {
    /// The return period the portfolio tail measure is read at (e.g. 200).
    pub return_period: f64,
    /// The fraction of capital the return-period net loss must stay within (the
    /// cat-aggregate coverability test).
    pub solvency_fraction: f64,
    /// The fraction of capital a single net line may not exceed (per-risk line).
    pub line_fraction: f64,
    /// Monte-Carlo trials used to estimate the portfolio tail measure.
    pub tail_trials: usize,
}

impl ExposurePolicy {
    /// Assess a candidate net line against the syndicate's limits, recomputed
    /// from its *current* capital and its OWN cat-model belief over its current
    /// net `book`. Declines an insolvent syndicate; declines a net line above the
    /// per-risk line limit; declines when the post-addition portfolio
    /// return-period net loss would exceed the coverable cat aggregate. Otherwise
    /// accepts. The substrate's true cat process is never consulted.
    pub fn assess(
        &self,
        syndicate: &Syndicate,
        book: &NetBook,
        model: &CatModel,
        candidate: NetLine,
        rng: &mut Rng,
    ) -> UnderwritingDecision {
        if !syndicate.is_solvent() {
            return UnderwritingDecision::Decline(DeclineReason::Insolvent);
        }
        let capital = syndicate.capital();
        if candidate.net_limit > self.line_fraction * capital {
            return UnderwritingDecision::Decline(DeclineReason::PerRiskLine);
        }
        let mut post_addition = book.clone();
        post_addition.lines.push(candidate);
        let tail = portfolio_tail_loss(&post_addition, model, self.return_period, self.tail_trials, rng);
        if tail > self.solvency_fraction * capital {
            return UnderwritingDecision::Decline(DeclineReason::CatAggregate);
        }
        UnderwritingDecision::Accept
    }

    /// The syndicate's **capacity headroom**: the free cat-aggregate budget as a
    /// fraction of the whole budget, in `[0, 1]`. The budget is the coverable cat
    /// aggregate `solvency_fraction × capital` (the same figure the cat-aggregate
    /// limit binds on in [`assess`](Self::assess)); the consumed portion is the
    /// current book's [`portfolio_tail_loss`] under the syndicate's own cat-model
    /// belief. `1` is an idle book (all budget free, abundant capital); `0` is a
    /// book filled to the limit, or an insolvent syndicate with no budget at all.
    ///
    /// This is the local input the AvT multiplier's headroom channel reads (see
    /// [`headroom_target`]). Because the budget scales with *current* capital, a
    /// post-cat drawdown shrinks it — so a shared catastrophe collapses every
    /// exposed syndicate's headroom together, and their AvT targets harden in step.
    pub fn capacity_headroom(
        &self,
        syndicate: &Syndicate,
        book: &NetBook,
        model: &CatModel,
        rng: &mut Rng,
    ) -> f64 {
        let budget = self.solvency_fraction * syndicate.capital();
        if budget <= 0.0 {
            return 0.0; // no loss-absorbing capital → no capacity to write
        }
        let consumed = portfolio_tail_loss(book, model, self.return_period, self.tail_trials, rng);
        ((budget - consumed) / budget).clamp(0.0, 1.0)
    }
}

/// The **Bühlmann-Straub credibility** weight `Z = n / (n + k)` placed on a
/// syndicate's own experience when blending it with an industry benchmark. `n`
/// is the volume (information content) of the syndicate's own experience —
/// exposure-years or claim count — and `k` is the per-syndicate credibility
/// parameter (the ratio of within-syndicate process variance to between-syndicate
/// variance of hypothetical means), part of the selectable genome. Credibility
/// is a function of *information content*, not elapsed time: a dense specialist
/// (large `n`) earns `Z` near 1 and trusts itself; a thin generalist (small `n`)
/// earns `Z` near 0 and leans on the benchmark. With `n = 0`, `Z = 0`.
pub fn credibility(n: f64, k: f64) -> f64 {
    let denominator = n + k;
    if denominator <= 0.0 {
        return 0.0;
    }
    n / denominator
}

/// The **attritional ELF** (expected loss cost) for the attritional component of
/// a layer: the syndicate's own realised **burning cost** blended with an
/// **industry benchmark** by Bühlmann-Straub [`credibility`],
/// `Z·own + (1 − Z)·benchmark`. Attritional losses are high-frequency, so a year
/// is informative and the estimate is **experience-updated** — the structural
/// opposite of the model-anchored catastrophe ELF. `n` is the volume of the
/// syndicate's own experience and `k` its per-syndicate credibility parameter.
pub fn attritional_elf(own_burning_cost: f64, benchmark: f64, n: f64, k: f64) -> f64 {
    let z = credibility(n, k);
    z * own_burning_cost + (1.0 - z) * benchmark
}

/// The unit being priced: a [`Layer`] sitting over an underlying net `exposure`
/// (the syndicate's net retained sum at risk for the risk) in a `territory`. A
/// catastrophe in the territory strikes the whole exposure as one shared
/// occurrence, and the layer bears `clamp(damage_fraction × exposure − attachment,
/// 0, limit)`. Both the catastrophe ELF and the marginal tail capital are read off
/// this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerExposure {
    pub layer: Layer,
    pub exposure: f64,
    pub territory: Territory,
    /// The reinstatement terms the layer carries. [`ReinstatementTerms::none`] is a
    /// layer with no reinstatement cover; finite terms fold an expected
    /// reinstatement-premium credit into the quoted technical premium (see
    /// [`technical_premium`]).
    pub reinstatement: ReinstatementTerms,
}

/// The **expected reinstatement fraction**: the model-anchored mean fraction of a
/// cat XoL layer's *original limit* that is reinstated in a believed year, over the
/// syndicate's own [`CatModel`]. Each believed year's events strike the whole
/// `exposure` as shared occurrences; their layered losses accumulate, and the
/// portion paid above the original limit (up to the aggregate `(1 + count) × limit`)
/// is what reinstatements restore. The fraction is `E[reinstated] / limit`.
///
/// It is **premium-independent** — a pure property of the layer, its terms, and the
/// belief — so it scales any base premium into the expected reinstatement income
/// the quoted price folds in. Like the cat ELF it is model-anchored, never
/// experience-updated, and deterministic given a seeded `rng`.
pub fn expected_reinstatement_fraction(
    layer: &Layer,
    exposure: f64,
    terms: &ReinstatementTerms,
    model: &CatModel,
    trials: usize,
    rng: &mut Rng,
) -> f64 {
    if trials == 0 || terms.count == 0 || layer.limit <= 0.0 {
        return 0.0;
    }
    let aggregate = (1.0 + terms.count as f64) * layer.limit;
    let total: f64 = (0..trials)
        .map(|_| {
            let count = model.annual_event_count(rng);
            let annual_loss: f64 = (0..count)
                .map(|_| layer.insured_loss(model.draw_damage_fraction(rng) * exposure))
                .sum();
            // The loss paid above the free original limit is what reinstatements
            // restore, capped at the aggregate cover.
            (annual_loss.min(aggregate) - layer.limit).max(0.0)
        })
        .sum();
    (total / trials as f64) / layer.limit
}

/// The **catastrophe ELF** (expected loss cost) for the catastrophe component of
/// a layer: the **model-anchored** expected annual cat loss to the layer over its
/// underlying `exposure`, estimated from the syndicate's own [`CatModel`] belief.
/// Each believed event's damage fraction strikes the whole exposure as one shared
/// occurrence (`gul = damage_fraction × exposure`), and that ground-up loss flows
/// up the layer ([`Layer::insured_loss`]); the ELF is the mean annual layered loss
/// over `trials` believed years.
///
/// It is **never experience-updated**: it consumes only the model (and a seeded
/// `rng`), so a run of benign, loss-free years cannot pull it down — a quiet
/// decade is a benign sample, not evidence the hazard fell. Higher layers are
/// reached only by larger believed events, so the cat ELF falls with attachment.
pub fn catastrophe_elf(
    layer: &Layer,
    exposure: f64,
    model: &CatModel,
    trials: usize,
    rng: &mut Rng,
) -> f64 {
    if trials == 0 {
        return 0.0;
    }
    let total: f64 = (0..trials)
        .map(|_| {
            let count = model.annual_event_count(rng);
            (0..count)
                .map(|_| layer.insured_loss(model.draw_damage_fraction(rng) * exposure))
                .sum::<f64>()
        })
        .sum();
    total / trials as f64
}

/// The **actuarial technical price** of a layer: its loss cost loaded for
/// expenses and profit by dividing by the **target loss ratio**,
/// `ATP = loss_cost / target_loss_ratio`. Division by a target loss ratio below
/// 1 is a **multiplicative** loading (`× 1 / target_loss_ratio > 1`), equivalent
/// to the expense form `gross = pure / (1 − expense_ratio)` with the target loss
/// ratio in the role of `1 − expense_ratio`. The multiplicative form self-funds
/// the loading; additive loading systematically underprices. The cost-of-capital
/// loading is added on top (see [`technical_premium`]).
pub fn actuarial_technical_price(loss_cost: f64, target_loss_ratio: f64) -> f64 {
    loss_cost / target_loss_ratio
}

/// The (1 − 1/return_period) quantile of a sorted sample of annual aggregate
/// losses — the same tail read used by the portfolio tail measure.
fn tail_quantile(mut aggregates: Vec<f64>, return_period: f64) -> f64 {
    if aggregates.is_empty() {
        return 0.0;
    }
    aggregates.sort_by(|a, b| a.partial_cmp(b).expect("losses are finite"));
    let trials = aggregates.len();
    let rank = ((trials as f64) * (1.0 - 1.0 / return_period)).ceil() as usize;
    let index = rank.saturating_sub(1).min(trials - 1);
    aggregates[index]
}

/// The **marginal capital** a layer consumes: its marginal contribution to the
/// syndicate's [`portfolio_tail_loss`] measure — the return-period net loss WITH
/// the `risk` (a [`LayerExposure`]) on the book minus WITHOUT it. The existing
/// `book` is the syndicate's current net retained lines, and the measure is
/// computed from the syndicate's own [`CatModel`] belief with zone correlation,
/// exactly as `portfolio_tail_loss`.
///
/// Both the with- and without-layer aggregates are read off the **same** believed
/// event draws (common random numbers, one simulation), so the difference is the
/// layer's clean marginal contribution rather than Monte-Carlo noise. The measure
/// is **layer-aware**: a higher attachment is penetrated only by larger believed
/// events, so an upper, cat-exposed layer that the tail does reach consumes
/// disproportionate tail capital per unit of limit while a remote layer the tail
/// never reaches consumes almost none. This is the capital the cost-of-capital
/// loading charges for (see [`technical_premium`]).
pub fn marginal_capital(
    book: &NetBook,
    risk: &LayerExposure,
    model: &CatModel,
    return_period: f64,
    trials: usize,
    rng: &mut Rng,
) -> f64 {
    if trials == 0 {
        return 0.0;
    }
    // The zones the simulation walks: the book's zones, plus the candidate's
    // territory if the book has no exposure there yet.
    let mut zones = book.zones();
    if !zones.contains(&risk.territory) {
        zones.push(risk.territory);
    }
    let zone_exposures: Vec<(Territory, f64)> =
        zones.iter().map(|&z| (z, book.zone_exposure(z))).collect();

    let mut base = Vec::with_capacity(trials);
    let mut with_layer = Vec::with_capacity(trials);
    for _ in 0..trials {
        let mut base_year = 0.0;
        let mut with_year = 0.0;
        for &(zone, flat_exposure) in &zone_exposures {
            // One year's believed events in this zone, drawn once and shared by
            // both books (common random numbers).
            let count = model.annual_event_count(rng);
            for _ in 0..count {
                let damage_fraction = model.draw_damage_fraction(rng);
                let flat_loss = damage_fraction * flat_exposure;
                base_year += flat_loss;
                with_year += flat_loss;
                if zone == risk.territory {
                    // The candidate layer is struck by the same shared occurrence.
                    with_year += risk.layer.insured_loss(damage_fraction * risk.exposure);
                }
            }
        }
        base.push(base_year);
        with_layer.push(with_year);
    }
    let marginal = tail_quantile(with_layer, return_period) - tail_quantile(base, return_period);
    marginal.max(0.0)
}

/// A syndicate's realised **attritional experience** for a risk, the input to the
/// experience-updated attritional ELF: its own realised **burning cost**, the
/// **industry benchmark** burning cost, and the **volume** `n` of own experience
/// (exposure-years / claim count) that drives Bühlmann-Straub credibility.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttritionalExperience {
    pub own_burning_cost: f64,
    pub benchmark: f64,
    pub volume: f64,
}

/// The per-syndicate **pricing genome** parameters and pricing calibration that
/// build the technical premium. `hurdle_rate` (the cost-of-capital loading rate)
/// and `credibility_k` (the Bühlmann-Straub information-content parameter) are
/// selectable genome parameters that vary across the population (#12);
/// `target_loss_ratio` is pricing calibration; and `return_period` / `tail_trials`
/// configure the portfolio tail measure the cost-of-capital loading reads. They
/// live with the syndicate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricingParams {
    pub hurdle_rate: f64,
    pub credibility_k: f64,
    pub target_loss_ratio: f64,
    /// The return period the marginal tail capital is read at (e.g. 200).
    pub return_period: f64,
    /// Monte-Carlo trials for the catastrophe ELF and the marginal tail capital.
    pub tail_trials: usize,
}

/// The decomposed **technical premium** (TP) for a layer — the rational price
/// floor, not the market price. Loss cost splits into an experience-updated
/// attritional ELF and a model-anchored catastrophe ELF; the actuarial technical
/// price loads loss cost multiplicatively by the target loss ratio; the
/// cost-of-capital loading is the hurdle rate on the marginal tail capital the
/// layer consumes; and the **expected reinstatement credit** folds the layer's
/// reinstatement terms into the quote.
/// `TP = ATP + cost_of_capital − expected_reinstatement_credit`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TechnicalPremium {
    pub attritional_elf: f64,
    pub catastrophe_elf: f64,
    pub loss_cost: f64,
    pub actuarial_technical_price: f64,
    pub marginal_capital: f64,
    pub cost_of_capital: f64,
    /// The expected reinstatement-premium income the layer's terms will earn,
    /// credited against the quoted price: `factor × E[reinstated_fraction] ×
    /// (ATP + cost_of_capital)`. Zero for a layer with no reinstatement. Because
    /// reinstatement premiums are extra income the layer collects when losses
    /// recur within the year, the quoted base price is lower the more reinstatement
    /// the layer carries — the quoted ROL incorporates the expected reinstatement
    /// cost rather than ignoring the terms.
    pub expected_reinstatement_credit: f64,
    pub technical_premium: f64,
}

/// Compute a syndicate's [`TechnicalPremium`] for a `risk` (a [`LayerExposure`]),
/// given its current net `book`, its own [`CatModel`] belief, its realised
/// attritional [`experience`](AttritionalExperience), and its [`PricingParams`].
/// The rational floor the market's actual premium oscillates around — computing
/// it encodes no market behaviour.
///
/// Loss cost = experience-updated [`attritional_elf`] + model-anchored
/// [`catastrophe_elf`]; the [`actuarial_technical_price`] loads it by the target
/// loss ratio; and the cost-of-capital loading is `hurdle_rate × marginal_capital`,
/// where [`marginal_capital`] is the layer's marginal contribution to the
/// portfolio tail measure. The tail measure's `return_period` and `tail_trials`
/// come from the params; the result is deterministic given a seeded `rng`.
pub fn technical_premium(
    risk: &LayerExposure,
    book: &NetBook,
    model: &CatModel,
    experience: &AttritionalExperience,
    params: &PricingParams,
    rng: &mut Rng,
) -> TechnicalPremium {
    let attritional = attritional_elf(
        experience.own_burning_cost,
        experience.benchmark,
        experience.volume,
        params.credibility_k,
    );
    let catastrophe = catastrophe_elf(&risk.layer, risk.exposure, model, params.tail_trials, rng);
    let loss_cost = attritional + catastrophe;
    let atp = actuarial_technical_price(loss_cost, params.target_loss_ratio);
    let marginal = marginal_capital(book, risk, model, params.return_period, params.tail_trials, rng);
    let cost_of_capital = params.hurdle_rate * marginal;
    // Fold the layer's reinstatement terms into the quote. The expected
    // reinstatement-premium income (a fraction of the base price, model-anchored
    // like the cat ELF) is collected when losses recur within the year, so it
    // credits the base price — a layer carrying reinstatements quotes below an
    // otherwise-identical layer that does not. Skipped when the terms charge
    // nothing, so a no-reinstatement layer prices and draws RNG exactly as before.
    let base_premium = atp + cost_of_capital;
    let expected_reinstatement_credit = {
        let fraction = expected_reinstatement_fraction(
            &risk.layer,
            risk.exposure,
            &risk.reinstatement,
            model,
            params.tail_trials,
            rng,
        );
        risk.reinstatement.premium_loading(fraction) * base_premium
    };
    TechnicalPremium {
        attritional_elf: attritional,
        catastrophe_elf: catastrophe,
        loss_cost,
        actuarial_technical_price: atp,
        marginal_capital: marginal,
        cost_of_capital,
        expected_reinstatement_credit,
        technical_premium: (base_premium - expected_reinstatement_credit).max(0.0),
    }
}

/// A **broker**: a stateful intermediary agent that routes coverage requests to
/// syndicates. It holds a **relationship score per syndicate** (indexed by
/// [`SyndicateId`]) and a **broker-level update inertia** governing how slowly
/// those scores move at year-end. Several brokers coexist with heterogeneous
/// relationship portfolios, so different brokers favour different syndicates;
/// that heterogeneity is part of the network topology herding (#3) and
/// stickiness (#5) live on. Routing is relationship-driven — never price-driven.
#[derive(Debug, Clone, PartialEq)]
pub struct Broker {
    /// Relationship score per syndicate, indexed by `SyndicateId.0`. Higher is a
    /// stronger relationship. New entrants (and syndicates beyond the known
    /// roster) score zero — relationships are earned, not given.
    relationships: Vec<f64>,
    /// Year-end update inertia in `[0, 1]`: the weight kept on the old score when
    /// blending in the year's signal. High inertia → sticky relationships that
    /// trail the competitive landscape, which is where renewal stickiness (#5)
    /// emerges. A per-broker parameter, so brokers differ in loyalty.
    pub inertia: f64,
}

/// A syndicate's year's **relationship outcome** with a broker, the signal the
/// year-end relationship update reads. Trust is raised by competitive quoting
/// and winning placements (and sustained by staying solvent — paying claims
/// reliably) and eroded by declining business or, worst of all, going insolvent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelationshipOutcome {
    /// The syndicate quoted competitively on the broker's business this year.
    pub quoted: bool,
    /// The syndicate won at least one placement from the broker this year.
    pub won: bool,
    /// The syndicate remained solvent (able to pay claims) through the year.
    pub solvent: bool,
}

impl RelationshipOutcome {
    /// The target the relationship score is nudged toward, in `[0, 1]`: insolvency
    /// is the strongest erosion (0); winning is the strongest reinforcement (1);
    /// quoting-but-losing is mildly positive (engaged but unsuccessful); a year
    /// with no engagement erodes toward zero.
    fn signal(&self) -> f64 {
        if !self.solvent {
            0.0
        } else if self.won {
            1.0
        } else if self.quoted {
            0.6
        } else {
            0.1
        }
    }
}

impl Broker {
    /// A broker with the given starting relationship scores and update inertia.
    pub fn new(relationships: Vec<f64>, inertia: f64) -> Self {
        Broker { relationships, inertia }
    }

    /// The broker's relationship score with a syndicate. Syndicates beyond the
    /// known roster score zero (an unknown is a new relationship).
    pub fn relationship(&self, syndicate: SyndicateId) -> f64 {
        self.relationships.get(syndicate.0).copied().unwrap_or(0.0)
    }

    /// Update the broker's relationship with a syndicate at year-end from the
    /// year's [`RelationshipOutcome`], blending the old score toward the outcome's
    /// signal with the broker's inertia: `new = inertia·old + (1 − inertia)·signal`.
    /// High inertia makes the score trail the competitive landscape, which is
    /// where renewal stickiness (#5) emerges — the lag is the mechanism, not a
    /// hardcoded stickiness factor. Grows the roster if it sees a new syndicate.
    pub fn update_relationship(&mut self, syndicate: SyndicateId, outcome: RelationshipOutcome) {
        if syndicate.0 >= self.relationships.len() {
            self.relationships.resize(syndicate.0 + 1, 0.0);
        }
        let signal = outcome.signal();
        let old = self.relationships[syndicate.0];
        self.relationships[syndicate.0] = self.inertia * old + (1.0 - self.inertia) * signal;
    }

    /// A **relationship-weighted shortlist** of up to `size` of the `capable`
    /// syndicates (those already filtered for solvency, line, and zone), ordered
    /// **lead-first**: the lead is the strongest relationship on the shortlist,
    /// the rest are followers in descending relationship order.
    ///
    /// Selection is a weighted sample **without replacement**, each syndicate's
    /// inclusion weight rising with its relationship score. A small floor on the
    /// weight keeps new entrants (relationship zero) shortlisted *rarely but not
    /// never*, so they can build relationships over years — the relational half
    /// of the counter-cyclical entry lag (#6). Crucially, **price is not an
    /// input**: routing does relationship-driven shortlisting and lead
    /// designation only; which quote wins is the insured's separate clearing
    /// decision, so routing stickiness (#5) never entangles with price selection.
    pub fn shortlist(&self, capable: &[SyndicateId], size: usize, rng: &mut Rng) -> Vec<SyndicateId> {
        // A small floor relative to the strongest relationship: a zero-score
        // entrant keeps a slim, non-zero chance of inclusion against incumbents.
        let max_rel = capable.iter().map(|&s| self.relationship(s)).fold(0.0_f64, f64::max);
        let floor = 0.01 * max_rel.max(1.0);

        let mut pool: Vec<(SyndicateId, f64)> =
            capable.iter().map(|&s| (s, self.relationship(s) + floor)).collect();
        let mut chosen: Vec<SyndicateId> = Vec::new();
        while chosen.len() < size && !pool.is_empty() {
            let total: f64 = pool.iter().map(|(_, w)| w).sum();
            let mut target = rng.uniform() * total;
            let mut picked = pool.len() - 1;
            for (i, (_, w)) in pool.iter().enumerate() {
                target -= w;
                if target <= 0.0 {
                    picked = i;
                    break;
                }
            }
            chosen.push(pool.remove(picked).0);
        }
        // Order lead-first by relationship: the strongest credible relationship
        // leads, the rest follow.
        chosen.sort_by(|&a, &b| {
            self.relationship(b)
                .partial_cmp(&self.relationship(a))
                .expect("relationship scores are finite")
        });
        chosen
    }
}

/// The free-budget fraction treated as **normal** capacity utilisation, where the
/// headroom-implied AvT target is exactly `1` — the syndicate prices at the TP
/// floor the cycle oscillates around. Below it capacity is scarce (target above
/// `1`, holding out); above it capacity is abundant (target below `1`, undercutting
/// to win business). A market-level calibration constant, not a genome trait.
pub const NORMAL_HEADROOM: f64 = 0.5;

/// How far the headroom-implied AvT target swings away from `1` per unit of
/// headroom deviation from [`NORMAL_HEADROOM`]. Calibration — it sets the *shape*
/// of the target curve `AvT*(h)`; how fast a syndicate chases that target is its
/// own genome [`AvtParams::headroom_responsiveness`].
pub const HEADROOM_TARGET_SLOPE: f64 = 1.0;

/// The per-syndicate **AvT genome**: how a syndicate re-prices its ask around the
/// technical-premium floor. All three are selectable parameters market selection
/// acts on (#12) — a syndicate that chases share too hard runs soft and is punished
/// when the tail arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvtParams {
    /// Relaxation rate in `[0, 1]`: the fraction of the gap to the headroom-implied
    /// target the AvT multiplier closes each year. Small → slow-moving, which is
    /// what gives the cycle its inertia.
    pub headroom_responsiveness: f64,
    /// Gain on the placement-feedback channel — how sharply AvT reacts to the gap
    /// between realised win-rate and the syndicate's share-appetite.
    pub feedback_responsiveness: f64,
    /// The **share-appetite**: the target win-rate the feedback loop homeostatically
    /// seeks. Winning more than this lifts AvT (giving margin back); winning less
    /// cuts it (to compete). An explicit, selectable target — never an implied 50%.
    pub share_appetite: f64,
}

/// The **headroom-implied AvT target** `AvT*(h)`: the level a syndicate's capacity
/// state alone says it should price at. Anchored so that normal headroom
/// ([`NORMAL_HEADROOM`]) targets exactly `1` (the TP floor); abundant headroom
/// (idle capital, low opportunity cost of writing) targets **below 1** to win
/// business, and scarce headroom targets **above 1** to hold out. Monotone
/// decreasing in headroom. This is the structural core of the cycle: a shared
/// catastrophe collapses every exposed syndicate's headroom at once, lifting their
/// targets above 1 together — market-wide hardening with no coordinator.
pub fn headroom_target(headroom: f64) -> f64 {
    1.0 + HEADROOM_TARGET_SLOPE * (NORMAL_HEADROOM - headroom)
}

/// The annual **AvT update**: a syndicate's slow-moving ask multiplier re-set once
/// a year at renewal from two purely local inputs, combined **additively**:
///
/// 1. **Headroom channel** — AvT *relaxes toward* the [`headroom_target`] level by
///    the genome [`headroom_responsiveness`](AvtParams::headroom_responsiveness)
///    fraction of the gap. Targeting a *level* (not integrating a per-round nudge)
///    anchors AvT to the TP floor: the floor is the standing attractor and the slow
///    relaxation toward it is the multi-year hard-market persistence.
/// 2. **Feedback channel** — a homeostatic nudge toward the syndicate's
///    [`share_appetite`](AvtParams::share_appetite): `feedback_responsiveness ×
///    (realised_win_rate − share_appetite)`. Winning above appetite lifts AvT to
///    give margin back; winning below cuts it to compete.
///
/// Neither input is a market-phase signal — both read the syndicate's own state, so
/// summing them double-counts nothing. The result is floored at `0` (a price can
/// never be negative).
pub fn updated_avt(current: f64, headroom: f64, realised_win_rate: f64, params: &AvtParams) -> f64 {
    let headroom_channel = params.headroom_responsiveness * (headroom_target(headroom) - current);
    let feedback_channel = params.feedback_responsiveness * (realised_win_rate - params.share_appetite);
    (current + headroom_channel + feedback_channel).max(0.0)
}

/// A syndicate's **quote** on a layer: the syndicate and the price it offers.
/// The unit the insured clears on and the lead the followers anchor toward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quote {
    pub syndicate: SyndicateId,
    pub price: f64,
}

/// A syndicate's **blind quote**: its own technical premium scaled by its **ask
/// multiplier** (the AvT lever; 1.0 until the cycle layer moves it), computed
/// from its own TP alone. "Blind" is structural — this sees no other quote — so
/// the lead's quote can never be a reaction to a rival's. Followers also start
/// from their own blind quote before anchoring (see [`anchored_quote`]).
pub fn blind_quote(syndicate: SyndicateId, technical_premium: f64, ask_multiplier: f64) -> Quote {
    Quote { syndicate, price: technical_premium * ask_multiplier }
}

/// The **derived follower weight** `w ∈ [0, 1]` a follower places on the lead's
/// quote when anchoring its own (see [`anchored_quote`]). It is computed, never
/// a fixed constant — a fixed herding weight would make herding (#3)
/// tautological. It rises when the follower's **own estimate is low-confidence**
/// (`1 − own_confidence`, the same information-content logic as credibility:
/// thin own data leans on the lead) and when the **lead is more reputable**
/// (`lead_reputation`), and is scaled by the follower's per-syndicate **herding
/// susceptibility** (a genome parameter). `own_confidence` and `lead_reputation`
/// are expected in `[0, 1]`; the product is clamped into `[0, 1]`.
pub fn follower_weight(own_confidence: f64, lead_reputation: f64, herding_susceptibility: f64) -> f64 {
    (herding_susceptibility * (1.0 - own_confidence) * lead_reputation).clamp(0.0, 1.0)
}

/// A follower's **anchored quote**: its own blind price pulled toward the lead's
/// observed quote by the derived weight `w` (see [`follower_weight`]),
/// `(1 − w)·own_price + w·lead_quote`. The blend moves **price, not belief** — it
/// consumes only prices, so a follower's cat-model parameters are untouched. This
/// is where herding (#3) emerges, and confining it to price keeps it orthogonal
/// to cat-model homogeneity (#14).
pub fn anchored_quote(own_price: f64, lead_quote: f64, w: f64) -> f64 {
    (1.0 - w) * own_price + w * lead_quote
}

/// A follower's response on a placement: either an (anchored) [`Quote`] or a
/// decline carrying the [`DeclineReason`] from its own exposure policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FollowerResponse {
    Quote(Quote),
    Decline(DeclineReason),
}

/// A follower's placement response, composing capacity discipline with herding.
/// The follower first applies its OWN exposure limits (the `exposure` decision,
/// produced by [`ExposurePolicy::assess`]): on a decline it drops off the panel
/// regardless of the lead — herding moves price, never capacity discipline. On
/// accept it quotes its own blind `own_price` **anchored** toward `lead_quote`
/// by the derived weight `w` (see [`anchored_quote`]).
pub fn follower_response(
    follower: SyndicateId,
    own_price: f64,
    lead_quote: f64,
    w: f64,
    exposure: UnderwritingDecision,
) -> FollowerResponse {
    match exposure {
        UnderwritingDecision::Decline(reason) => FollowerResponse::Decline(reason),
        UnderwritingDecision::Accept => FollowerResponse::Quote(Quote {
            syndicate: follower,
            price: anchored_quote(own_price, lead_quote, w),
        }),
    }
}

/// A syndicate's offer to subscribe to a layer under **firm-order subscription**:
/// its (anchored) `quote`, its own exposure `decision`, and the `offered_share` —
/// the fraction of the layer limit it is willing to take. Followers' quotes are
/// already anchored toward the lead (see [`anchored_quote`]); the lead's `quote`
/// is the firm order itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubscriptionOffer {
    pub syndicate: SyndicateId,
    pub quote: f64,
    pub decision: UnderwritingDecision,
    pub offered_share: f64,
}

/// Form a subscription [`Panel`] by **firm-order subscription** (Model B). The
/// `lead`'s quote is the layer's **firm order** — the single price the insured pays
/// — and the lead takes the first share. Each follower offer is considered in
/// order and **subscribes when both** its own exposure limits permit (`decision` is
/// [`Accept`](UnderwritingDecision::Accept)) **and** its anchored `quote` sits **at
/// or below the firm order** (it will write at the lead's terms). This is why
/// herding is load-bearing in *formation*: a follower anchored toward a reputable
/// lead lowers its quote and subscribes to a firm order it would otherwise reject.
///
/// Fill is **capacity-first**: shares accumulate in order up to the full layer, the
/// last needed share trimmed so the panel never exceeds `1.0`; once full, later
/// willing offers are not needed. If willing capacity falls short the layer is left
/// **partially placed** (the panel's [`placed_portion`](Panel::placed_portion) is
/// below `1.0`). A lead that cannot write (declines on exposure) leaves the layer
/// entirely unplaced — an empty panel.
pub fn form_panel(lead: SubscriptionOffer, followers: &[SubscriptionOffer]) -> Panel {
    let mut entries: Vec<PanelEntry> = Vec::new();
    let firm_order = lead.quote;
    let mut placed = 0.0;

    let subscribe = |offer: &SubscriptionOffer, placed: &mut f64, entries: &mut Vec<PanelEntry>| {
        if *placed >= 1.0 {
            return;
        }
        if offer.decision != UnderwritingDecision::Accept {
            return;
        }
        let take = offer.offered_share.min(1.0 - *placed);
        if take <= 0.0 {
            return;
        }
        entries.push(PanelEntry { syndicate: offer.syndicate, share: take });
        *placed += take;
    };

    // The lead sets the firm order and takes the first share. Only a lead that can
    // write founds a panel; a declining lead leaves the layer unplaced.
    subscribe(&lead, &mut placed, &mut entries);
    if entries.is_empty() {
        return Panel { entries };
    }

    for follower in followers {
        if follower.quote <= firm_order {
            subscribe(follower, &mut placed, &mut entries);
        }
    }

    Panel { entries }
}

/// An **insured**: the demand-side agent seeking cover. It carries a private
/// **risk-aversion loading** (`> 1`) and an **expected loss** on the cover it
/// wants; its **willingness-to-pay** is the product. WTP is a stable preference —
/// an insured's own losses enter the market through experience rating on the
/// supply side (see [`experience_modifier`]), not by perturbing WTP.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Insured {
    /// Risk-aversion loading on expected loss, `> 1`.
    pub risk_aversion: f64,
    /// The insured's expected annual loss on the cover sought.
    pub expected_loss: f64,
}

impl Insured {
    /// The insured's **willingness-to-pay**: `risk_aversion × expected_loss`. The
    /// ceiling on any premium it will accept.
    pub fn willingness_to_pay(&self) -> f64 {
        self.risk_aversion * self.expected_loss
    }
}

/// **Clearing**: the insured takes the **cheapest** quote at or below `wtp`, or
/// `None` if every quote exceeds it. Clearing is a demand-side decision distinct
/// from routing — the relationship-designated lead does not win on its status, so
/// routing stickiness (#5) never entangles with price selection.
pub fn clear_cheapest(quotes: &[Quote], wtp: f64) -> Option<Quote> {
    quotes
        .iter()
        .filter(|q| q.price <= wtp)
        .min_by(|a, b| a.price.partial_cmp(&b.price).expect("prices are finite"))
        .copied()
}

/// One band of an insured's desired tower as offered to it: the [`Layer`], the
/// **expected loss** it covers (its value to the insured), and the **cleared
/// price** for that band (the cheapest acceptable quote on it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TowerLayerOffer {
    pub layer: Layer,
    pub expected_loss: f64,
    pub price: f64,
}

/// The outcome of an insured structuring its tower against its WTP budget.
#[derive(Debug, Clone, PartialEq)]
pub enum TowerPurchase {
    /// The bands bound (a subset of those offered when restructured) and the
    /// total premium paid.
    Bound { layers: Vec<Layer>, total_price: f64 },
    /// No band was affordable within WTP — the insured self-insures entirely.
    Declined,
}

/// **Demand-side restructuring**: the insured fits its tower to a `wtp` budget.
/// If the whole offered tower fits, it buys all of it. Otherwise it drops the
/// **worst value-for-money** band (highest price-to-expected-loss ratio) and
/// retries — the fine-grained quantity elasticity that raises retention, lowers
/// limit, or self-insures a tranche when prices spike, damping apparent
/// hard-market profitability (#1). If not even one band fits, it declines.
pub fn restructure_tower(offers: &[TowerLayerOffer], wtp: f64) -> TowerPurchase {
    let mut kept: Vec<&TowerLayerOffer> = offers.iter().collect();
    // Worst value-for-money first, so popping from the end drops it.
    kept.sort_by(|a, b| {
        let la = a.price / a.expected_loss.max(f64::MIN_POSITIVE);
        let lb = b.price / b.expected_loss.max(f64::MIN_POSITIVE);
        la.partial_cmp(&lb).expect("loadings are finite")
    });
    while !kept.is_empty() {
        let total: f64 = kept.iter().map(|o| o.price).sum();
        if total <= wtp {
            let mut layers: Vec<Layer> = kept.iter().map(|o| o.layer).collect();
            // Restore tower order (ascending attachment) for the bound layers.
            layers.sort_by(|a, b| a.attachment.partial_cmp(&b.attachment).expect("attachments are finite"));
            return TowerPurchase::Bound { layers, total_price: total };
        }
        kept.pop(); // drop the worst value-for-money band and retry
    }
    TowerPurchase::Declined
}

/// The **experience-rating modifier** a syndicate applies to its loss-cost
/// estimate for a *specific* risk, given that insured's own **loss history**
/// (#9). It credibility-weights the insured's realised loss relativity
/// (`own_losses / expected_losses`) against unity: `(1 − Z) · 1 + Z · relativity`
/// with `Z = credibility(n, k)` on the volume `n` of the insured's own history.
///
/// A clean history (`own < expected`) earns a **credit** (`< 1`); a chronic one
/// (`own > expected`) earns a **surcharge** (`> 1`); an insured with no own
/// history (`n = 0`) is **unrated** (`1`). Applied as a multiplier on loss cost,
/// it surcharges chronic loss-generators beyond an insured's WTP so the pool
/// self-selects — bad risks priced out — as an emergent consequence, not a cull.
pub fn experience_modifier(own_losses: f64, expected_losses: f64, n: f64, k: f64) -> f64 {
    if expected_losses <= 0.0 {
        return 1.0;
    }
    let relativity = own_losses / expected_losses;
    let z = credibility(n, k);
    (1.0 - z) * 1.0 + z * relativity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bühlmann_straub_credibility_rises_with_information_content() {
        // Z = n / (n + k): credibility as a function of the VOLUME of own
        // experience n against the per-syndicate parameter k. With no own data
        // Z is 0 (lean entirely on the benchmark); as n grows past k, Z passes a
        // half and climbs toward 1 (trust own experience); it never reaches 1.
        let k = 10.0;
        assert_eq!(credibility(0.0, k), 0.0);
        assert!((credibility(10.0, k) - 0.5).abs() < 1e-12); // n == k → exactly a half
        assert!(credibility(90.0, k) > 0.89 && credibility(90.0, k) < 1.0);
        // Strictly increasing in n.
        assert!(credibility(5.0, k) < credibility(50.0, k));
    }

    #[test]
    fn attritional_elf_blends_own_burning_cost_and_benchmark_by_credibility() {
        // The attritional ELF is experience-updated: Z·own + (1−Z)·benchmark.
        // At full credibility (Z→1) it equals own burning cost; at zero
        // credibility (n = 0) it equals the benchmark; in between it interpolates.
        let own = 120.0;
        let benchmark = 80.0;
        let k = 10.0;

        // No own experience → lean entirely on the benchmark.
        assert!((attritional_elf(own, benchmark, 0.0, k) - benchmark).abs() < 1e-9);
        // n == k → Z = 0.5 → the midpoint of own and benchmark.
        assert!((attritional_elf(own, benchmark, 10.0, k) - 100.0).abs() < 1e-9);
        // Dense own experience → close to own burning cost.
        let dense = attritional_elf(own, benchmark, 1_000.0, k);
        assert!(dense > 119.0 && dense < own);
    }

    #[test]
    fn credibility_differs_between_a_dense_specialist_and_a_thin_generalist() {
        // Specialist/generalist divergence (#4) is straight from information
        // content: a narrow specialist with dense, low-variance data in its line
        // earns a high Z and trusts its own burning cost; a thin generalist earns
        // a low Z and leans on the industry benchmark — over the SAME own/benchmark
        // gap, so the only thing differing is the volume of own experience (and the
        // per-syndicate k).
        let own = 150.0; // both have observed the same own burning cost
        let benchmark = 100.0;

        // Dense specialist: many exposure-years of own data, tight k.
        let specialist_n = 200.0;
        let specialist_k = 8.0;
        let z_specialist = credibility(specialist_n, specialist_k);
        let elf_specialist = attritional_elf(own, benchmark, specialist_n, specialist_k);

        // Thin generalist: little own data, looser k.
        let generalist_n = 3.0;
        let generalist_k = 20.0;
        let z_generalist = credibility(generalist_n, generalist_k);
        let elf_generalist = attritional_elf(own, benchmark, generalist_n, generalist_k);

        // The specialist trusts itself far more than the generalist.
        assert!(z_specialist > 0.9, "specialist Z {z_specialist} should be near 1");
        assert!(z_generalist < 0.2, "generalist Z {z_generalist} should be near 0");
        assert!(z_specialist > z_generalist);

        // So the specialist's ELF sits close to its own burning cost, while the
        // generalist's is pulled toward the benchmark.
        assert!((elf_specialist - own).abs() < (elf_generalist - own).abs());
        assert!(elf_specialist > 145.0);
        assert!(elf_generalist < 110.0);
    }

    #[test]
    fn catastrophe_elf_is_layer_aware_and_falls_as_attachment_rises() {
        // The catastrophe ELF is the model-anchored expected annual cat loss to a
        // layer over its underlying exposure: derived from the syndicate's own
        // CatModel by flowing each believed event's damage up the layer. Because a
        // higher layer is reached only by larger events, the cat ELF falls sharply
        // as attachment rises (the loss-cost half of the layer-position gradient).
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let exposure = 1_000.0;
        let limit = 100.0;

        let working = Layer { attachment: 0.0, limit };
        let upper = Layer { attachment: 400.0, limit };

        let mut rng = Rng::seeded(2024);
        let elf_working = catastrophe_elf(&working, exposure, &model, 8_000, &mut rng);
        let mut rng = Rng::seeded(2024);
        let elf_upper = catastrophe_elf(&upper, exposure, &model, 8_000, &mut rng);

        assert!(elf_working > 0.0, "a cat-exposed working layer has positive ELF");
        assert!(elf_upper > 0.0, "a reachable upper layer has positive ELF");
        assert!(
            elf_upper < elf_working,
            "upper-layer cat ELF {elf_upper} should be far below working {elf_working}"
        );
    }

    #[test]
    fn catastrophe_elf_is_model_anchored_and_unmoved_by_benign_experience() {
        // Cat ELF comes from the CatModel belief and is NEVER pulled down by a run
        // of loss-free years. We compute it, then "live through" a decade of benign
        // (zero realised loss) cat experience, and recompute: it is identical,
        // because catastrophe_elf consumes only the model — there is no experience
        // input to it (the truth/belief separation enforced by the type signature).
        let model = CatModel { annual_frequency: 0.5, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let layer = Layer { attachment: 0.0, limit: 100.0 };
        let exposure = 1_000.0;

        let mut rng = Rng::seeded(7);
        let elf_before = catastrophe_elf(&layer, exposure, &model, 8_000, &mut rng);

        // A decade of benign experience: realised cat losses are all zero. This is
        // a benign SAMPLE, not evidence the hazard fell — and it cannot touch the
        // model-anchored estimate.
        let benign_realised_losses = [0.0_f64; 10];
        assert!(benign_realised_losses.iter().all(|&l| l == 0.0));

        let mut rng = Rng::seeded(7);
        let elf_after = catastrophe_elf(&layer, exposure, &model, 8_000, &mut rng);

        assert_eq!(
            elf_before, elf_after,
            "benign experience must not move the model-anchored cat ELF"
        );
        // Contrast: were the cat ELF experience-updated like the attritional one,
        // the benign zero burning cost observed at high credibility (own = 0)
        // would collapse it toward zero against the model as benchmark — the
        // soft-market rate-erosion miscalibration the cat ELF is built to avoid.
        let experience_updated = attritional_elf(0.0, elf_before, 1_000.0, 10.0);
        assert!(experience_updated < 0.5 * elf_before);
    }

    #[test]
    fn the_actuarial_technical_price_loads_loss_cost_multiplicatively() {
        // ATP = loss_cost / target_loss_ratio. Dividing by a target loss ratio
        // below 1 is a MULTIPLICATIVE loading (× 1/target_LR > 1), the form that
        // self-funds the loading — additive loading systematically underprices.
        let loss_cost = 60.0;
        let target_loss_ratio = 0.6;
        let atp = actuarial_technical_price(loss_cost, target_loss_ratio);

        // The defining identity.
        assert!((atp - 100.0).abs() < 1e-9);
        // Multiplicative: the loaded price is a constant factor 1/target_LR above
        // loss cost, so doubling loss cost doubles ATP (a fixed factor, not a fixed
        // additive margin).
        assert!((atp / loss_cost - 1.0 / target_loss_ratio).abs() < 1e-12);
        let doubled = actuarial_technical_price(2.0 * loss_cost, target_loss_ratio);
        assert!((doubled - 2.0 * atp).abs() < 1e-9);
        // Equivalent to the multiplicative expense form gross = pure/(1−expense)
        // when the target loss ratio plays the role of (1 − expense_ratio).
        let expense_ratio = 1.0 - target_loss_ratio;
        assert!((atp - loss_cost / (1.0 - expense_ratio)).abs() < 1e-9);
        // The loading genuinely lifts the price above the pure loss cost.
        assert!(atp > loss_cost);
    }

    #[test]
    fn marginal_capital_is_the_layers_contribution_to_the_portfolio_tail_measure() {
        // marginal_capital(layer) = portfolio tail measure WITH the layer minus
        // WITHOUT it, over the syndicate's own book and cat-model belief. On an
        // empty book it is the standalone return-period loss the layer adds; it is
        // non-negative (a layer can only add exposure) and deterministic.
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let empty = NetBook { lines: vec![] };
        let layer = Layer { attachment: 0.0, limit: 100.0 };
        let exposure = 1_000.0;
        let t0 = Territory(0);

        let risk = LayerExposure { layer, exposure, territory: t0, reinstatement: ReinstatementTerms::none() };
        let mut rng = Rng::seeded(2024);
        let mc = marginal_capital(&empty, &risk, &model, 200.0, 8_000, &mut rng);
        assert!(mc > 0.0, "a cat-exposed working layer consumes tail capital");

        // Deterministic given a seed.
        let mut rng = Rng::seeded(2024);
        let mc_again = marginal_capital(&empty, &risk, &model, 200.0, 8_000, &mut rng);
        assert_eq!(mc, mc_again);

        // Adding the layer to an existing book never lowers the tail (marginal ≥ 0).
        let book = NetBook { lines: vec![NetLine { territory: t0, net_limit: 500.0 }] };
        let mut rng = Rng::seeded(11);
        let mc_on_book = marginal_capital(&book, &risk, &model, 200.0, 8_000, &mut rng);
        assert!(mc_on_book >= 0.0);
    }

    #[test]
    fn marginal_capital_is_layer_aware_so_an_unreachable_upper_layer_consumes_almost_none() {
        // The measure is layer-aware, not a flat limit charge: a layer attaching
        // far above any believable event almost never penetrates, so it adds
        // essentially nothing to the return-period loss — it consumes negligible
        // tail capital, while a working layer over the same exposure consumes much.
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let empty = NetBook { lines: vec![] };
        let exposure = 1_000.0;
        let t0 = Territory(0);

        let working = Layer { attachment: 0.0, limit: 100.0 };
        // Attaches at 980 over exposure 1000: needs a damage fraction above 0.98,
        // far into the believed tail, so the 1-in-200 barely reaches it.
        let remote = Layer { attachment: 980.0, limit: 20.0 };

        let working_risk = LayerExposure { layer: working, exposure, territory: t0, reinstatement: ReinstatementTerms::none() };
        let remote_risk = LayerExposure { layer: remote, exposure, territory: t0, reinstatement: ReinstatementTerms::none() };
        let mut rng = Rng::seeded(2024);
        let mc_working = marginal_capital(&empty, &working_risk, &model, 200.0, 8_000, &mut rng);
        let mut rng = Rng::seeded(2024);
        let mc_remote = marginal_capital(&empty, &remote_risk, &model, 200.0, 8_000, &mut rng);

        assert!(mc_working > 0.0);
        // Per unit of limit the remote layer consumes far less tail capital.
        assert!(
            mc_remote / remote.limit < 0.2 * (mc_working / working.limit),
            "remote/limit {} not far below working/limit {}",
            mc_remote / remote.limit,
            mc_working / working.limit
        );
    }

    fn cat_model() -> CatModel {
        CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 }
    }

    #[test]
    fn the_technical_premium_is_the_actuarial_price_plus_a_cost_of_capital_loading() {
        // TP = ATP + hurdle_rate × marginal_capital(layer). The breakdown is
        // internally consistent: loss cost is attritional ELF + catastrophe ELF,
        // ATP loads it by the target loss ratio, and the cost-of-capital loading is
        // the per-syndicate hurdle rate on the marginal tail capital the layer
        // consumes.
        let model = cat_model();
        let book = NetBook { lines: vec![] };
        let risk = LayerExposure { layer: Layer { attachment: 0.0, limit: 100.0 }, exposure: 1_000.0, territory: Territory(0), reinstatement: ReinstatementTerms::none() };
        let experience = AttritionalExperience { own_burning_cost: 30.0, benchmark: 25.0, volume: 50.0 };
        let params = PricingParams { hurdle_rate: 0.15, credibility_k: 10.0, target_loss_ratio: 0.6, return_period: 200.0, tail_trials: 8_000 };

        let mut rng = Rng::seeded(2024);
        let tp = technical_premium(&risk, &book, &model, &experience, &params, &mut rng);

        // Components reconcile.
        assert!((tp.loss_cost - (tp.attritional_elf + tp.catastrophe_elf)).abs() < 1e-9);
        assert!((tp.actuarial_technical_price - tp.loss_cost / params.target_loss_ratio).abs() < 1e-9);
        assert!((tp.cost_of_capital - params.hurdle_rate * tp.marginal_capital).abs() < 1e-9);
        assert!((tp.technical_premium - (tp.actuarial_technical_price + tp.cost_of_capital)).abs() < 1e-9);

        // Both halves of loss cost are present and positive for a cat-exposed
        // working layer with attritional experience.
        assert!(tp.attritional_elf > 0.0 && tp.catastrophe_elf > 0.0);
        assert!(tp.cost_of_capital > 0.0);

        // A higher hurdle rate raises the technical premium through the loading
        // alone; a zero hurdle collapses TP onto the actuarial technical price.
        let greedier = PricingParams { hurdle_rate: 0.30, ..params };
        let mut rng = Rng::seeded(2024);
        let tp_greedier = technical_premium(&risk, &book, &model, &experience, &greedier, &mut rng);
        assert!(tp_greedier.technical_premium > tp.technical_premium);

        let no_hurdle = PricingParams { hurdle_rate: 0.0, ..params };
        let mut rng = Rng::seeded(2024);
        let tp_floor = technical_premium(&risk, &book, &model, &experience, &no_hurdle, &mut rng);
        assert!((tp_floor.technical_premium - tp_floor.actuarial_technical_price).abs() < 1e-9);
    }

    #[test]
    fn the_layer_position_premium_gradient_emerges_up_a_tower() {
        // Layer-position premium gradient (#10), derived — never scheduled. Pricing
        // a vertical tower of equal-limit catastrophe layers over one exposure:
        //
        //   * rate-on-line (TP per unit limit) FALLS as attachment rises — the
        //     loss cost collapses because higher layers are reached only by rarer,
        //     larger events;
        //   * the cost-of-capital SHARE of the premium RISES as attachment rises —
        //     working/primary layers are loss-cost-dominated, while upper,
        //     cat-exposed layers are capital-dominated: almost all of their thin
        //     premium is the required return on the tail capital they consume.
        //
        // Both fall out of capital consumption and the model-anchored cat ELF, not
        // a hardcoded per-layer schedule.
        let model = cat_model();
        let book = NetBook { lines: vec![] };
        let exposure = 1_000.0;
        let limit = 100.0;
        // Pure catastrophe layers (no attritional component), to read the cat-driven
        // vertical gradient cleanly.
        let no_attritional = AttritionalExperience { own_burning_cost: 0.0, benchmark: 0.0, volume: 0.0 };
        let params = PricingParams { hurdle_rate: 0.15, credibility_k: 10.0, target_loss_ratio: 0.6, return_period: 200.0, tail_trials: 8_000 };

        let attachments = [0.0, 100.0, 200.0, 400.0];
        let priced: Vec<TechnicalPremium> = attachments
            .iter()
            .map(|&attachment| {
                let risk = LayerExposure { layer: Layer { attachment, limit }, exposure, territory: Territory(0), reinstatement: ReinstatementTerms::none() };
                // Common random numbers across layers: same seed isolates the
                // layer-position effect from Monte-Carlo noise.
                let mut rng = Rng::seeded(2024);
                technical_premium(&risk, &book, &model, &no_attritional, &params, &mut rng)
            })
            .collect();

        let rate_on_line: Vec<f64> = priced.iter().map(|p| p.technical_premium / limit).collect();
        let coc_share: Vec<f64> = priced.iter().map(|p| p.cost_of_capital / p.technical_premium).collect();

        // Rate-on-line strictly falls as attachment rises.
        for window in rate_on_line.windows(2) {
            assert!(window[1] < window[0], "rate-on-line did not fall with attachment: {rate_on_line:?}");
        }
        // Cost-of-capital share strictly rises as attachment rises.
        for window in coc_share.windows(2) {
            assert!(window[1] > window[0], "cost-of-capital share did not rise with attachment: {coc_share:?}");
        }

        // The working (primary) layer is loss-cost-dominated; the top layer is
        // capital-dominated — the two faces of the gradient.
        let working = priced.first().unwrap();
        let top = priced.last().unwrap();
        assert!(working.actuarial_technical_price > working.cost_of_capital);
        assert!(top.cost_of_capital > top.actuarial_technical_price);
    }

    #[test]
    fn an_empty_net_book_has_zero_portfolio_tail_loss() {
        // Tracer: a syndicate's cat MODEL (its belief, distinct from the
        // substrate's true cat process) drives the portfolio tail measure over
        // its net book. With nothing on the book there is no exposure, so the
        // return-period net loss is zero regardless of the belief.
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let book = NetBook { lines: vec![] };
        let mut rng = Rng::seeded(2024);
        let tail = portfolio_tail_loss(&book, &model, 200.0, 2_000, &mut rng);
        assert_eq!(tail, 0.0);
    }

    #[test]
    fn the_portfolio_tail_measure_diversifies_when_exposure_is_spread_across_zones() {
        // The portfolio tail measure accounts for zone correlation: the SAME
        // total net exposure concentrated in one zone (one shared occurrence)
        // produces a far heavier 1-in-200 net loss than the same exposure spread
        // across uncorrelated zones, whose independent cat processes diversify.
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let total = 16.0;
        let return_period = 200.0;
        let trials = 8_000;

        let concentrated = NetBook {
            lines: vec![NetLine { territory: Territory(0), net_limit: total }],
        };
        let spread = NetBook {
            lines: (0..16)
                .map(|z| NetLine { territory: Territory(z), net_limit: total / 16.0 })
                .collect(),
        };

        let mut rng = Rng::seeded(7);
        let tail_concentrated = portfolio_tail_loss(&concentrated, &model, return_period, trials, &mut rng);
        let mut rng = Rng::seeded(7);
        let tail_spread = portfolio_tail_loss(&spread, &model, return_period, trials, &mut rng);

        assert!(tail_concentrated > 0.0, "concentrated tail should be positive");
        // Diversification is real and material: spreading the book sharply cuts
        // the return-period loss.
        assert!(
            tail_spread < 0.6 * tail_concentrated,
            "spread tail {tail_spread} not materially below concentrated {tail_concentrated}"
        );
        // The within-zone loss cannot exceed the zone aggregate per event; with a
        // single dominant event the concentrated 1-in-200 stays within total TIV.
        assert!(tail_concentrated <= total, "concentrated tail {tail_concentrated} exceeded total exposure");
    }

    fn lenient_policy() -> ExposurePolicy {
        // A policy with effectively non-binding cat aggregate, to isolate the
        // per-risk line limit.
        ExposurePolicy {
            return_period: 200.0,
            solvency_fraction: 1_000.0, // huge: cat aggregate never binds here
            line_fraction: 0.1,
            tail_trials: 2_000,
        }
    }

    #[test]
    fn a_net_line_above_the_per_risk_line_limit_is_declined() {
        // The per-risk line limit is recomputed from CURRENT capital at quote
        // time: net line ≤ line_fraction × capital. A line above it declines; a
        // line at or below it is accepted (cat aggregate left non-binding here).
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let policy = lenient_policy();
        let syndicate = Syndicate::with_capital(1_000.0); // line cap = 0.1 × 1000 = 100
        let book = NetBook { lines: vec![] };
        let mut rng = Rng::seeded(1);

        let too_big = NetLine { territory: Territory(0), net_limit: 150.0 };
        assert_eq!(
            policy.assess(&syndicate, &book, &model, too_big, &mut rng),
            UnderwritingDecision::Decline(DeclineReason::PerRiskLine)
        );

        let within = NetLine { territory: Territory(0), net_limit: 80.0 };
        assert_eq!(
            policy.assess(&syndicate, &book, &model, within, &mut rng),
            UnderwritingDecision::Accept
        );
    }

    #[test]
    fn an_insolvent_syndicate_declines_every_risk() {
        // An insolvent syndicate is in runoff: it writes no new business, so any
        // candidate declines with Insolvent — even a trivially small line that
        // would clear both limits for a solvent syndicate.
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let policy = lenient_policy();
        let syndicate = Syndicate::with_capital(0.0); // in runoff
        let book = NetBook { lines: vec![] };
        let mut rng = Rng::seeded(1);

        let tiny = NetLine { territory: Territory(0), net_limit: 0.001 };
        assert_eq!(
            policy.assess(&syndicate, &book, &model, tiny, &mut rng),
            UnderwritingDecision::Decline(DeclineReason::Insolvent)
        );
    }

    #[test]
    fn writing_into_one_zone_trips_the_cat_aggregate_while_a_spread_book_does_not() {
        // Geographic accumulation pressure (#8) emerges from the cat-aggregate
        // limit, not a hardcoded goal. Repeatedly writing comparable lines into
        // ONE zone accumulates one shared occurrence, driving the return-period
        // net loss up until further risks there are declined; spreading the same
        // lines across many uncorrelated zones diversifies, so the same total
        // exposure stays under the limit and keeps clearing.
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let policy = ExposurePolicy {
            return_period: 200.0,
            solvency_fraction: 0.5,
            line_fraction: 0.5, // generous per-risk cap, so the cat aggregate binds first
            tail_trials: 4_000,
        };
        let syndicate = Syndicate::with_capital(100.0); // cat budget = 0.5 × 100 = 50
        let line_size = 1.0;
        let max_lines = 300;

        // --- Concentrated: everything into Territory(0) ---
        let mut book = NetBook { lines: vec![] };
        let mut declined_at: Option<usize> = None;
        for i in 0..max_lines {
            let mut rng = Rng::seeded(100 + i as u64);
            let candidate = NetLine { territory: Territory(0), net_limit: line_size };
            match policy.assess(&syndicate, &book, &model, candidate, &mut rng) {
                UnderwritingDecision::Accept => book.lines.push(candidate),
                UnderwritingDecision::Decline(DeclineReason::CatAggregate) => {
                    declined_at = Some(i);
                    break;
                }
                other => panic!("unexpected decline while concentrating: {other:?}"),
            }
        }
        let concentrated_written = book.lines.len();
        assert!(
            declined_at.is_some(),
            "concentrating in one zone never tripped the cat aggregate in {max_lines} lines"
        );

        // --- Spread: the SAME number of lines round-robined across 20 zones ---
        let zones = 20u32;
        let mut spread = NetBook { lines: vec![] };
        for i in 0..concentrated_written {
            let mut rng = Rng::seeded(100 + i as u64);
            let candidate = NetLine { territory: Territory(i as u32 % zones), net_limit: line_size };
            let decision = policy.assess(&syndicate, &spread, &model, candidate, &mut rng);
            assert_eq!(
                decision,
                UnderwritingDecision::Accept,
                "spread book declined at line {i} (total exposure {}) — diversification should keep it under the limit",
                spread.lines.len() as f64 * line_size
            );
            spread.lines.push(candidate);
        }
        // The spread book carries the full comparable total exposure with no
        // cat-aggregate breach.
        assert_eq!(spread.lines.len(), concentrated_written);
    }

    #[test]
    fn the_tail_measure_reads_the_syndicates_cat_model_belief_not_the_true_process() {
        // Truth/belief separation: the portfolio tail measure is computed from a
        // CatModel (the syndicate's belief), never from the substrate's true
        // CatastrophePeril. The substrate's true process below is constructed to
        // be wildly more severe than either belief, yet there is NO API to feed
        // it to the measure — the type system forbids it (portfolio_tail_loss
        // takes &CatModel). The measure therefore tracks belief alone: an
        // optimistic belief yields a lower tail than a pessimistic one over the
        // same book.
        let _true_process = CatastrophePeril {
            annual_frequency: 50.0, // catastrophically worse than any belief
            min_damage_fraction: 0.5,
            tail_alpha: 1.1,
        };

        let book = NetBook {
            lines: vec![NetLine { territory: Territory(0), net_limit: 10.0 }],
        };
        let optimistic = CatModel { annual_frequency: 0.3, min_damage_fraction: 0.02, tail_alpha: 1.6 };
        let pessimistic = CatModel { annual_frequency: 1.2, min_damage_fraction: 0.05, tail_alpha: 1.2 };

        let mut rng = Rng::seeded(2024);
        let tail_optimistic = portfolio_tail_loss(&book, &optimistic, 200.0, 8_000, &mut rng);
        let mut rng = Rng::seeded(2024);
        let tail_pessimistic = portfolio_tail_loss(&book, &pessimistic, 200.0, 8_000, &mut rng);

        // The belief drives the measure: a heavier-tailed, more frequent belief
        // estimates a larger return-period loss than an optimistic one.
        assert!(
            tail_pessimistic > tail_optimistic,
            "pessimistic belief {tail_pessimistic} should exceed optimistic {tail_optimistic}"
        );
        // Both are finite, bounded by the zone exposure per dominant event, and
        // entirely independent of the (far worse) true process, which could not
        // even be passed in.
        assert!(tail_optimistic > 0.0);
    }

    #[test]
    fn ground_up_loss_is_damage_fraction_times_sum_insured() {
        assert_eq!(ground_up_loss(0.25, 1_000.0), 250.0);
    }

    #[test]
    fn ground_up_loss_never_exceeds_sum_insured() {
        // Even a damage fraction beyond 1.0 cannot inflict more than the
        // asset's full replacement value.
        assert_eq!(ground_up_loss(1.5, 1_000.0), 1_000.0);
    }

    #[test]
    fn full_value_layer_settles_the_entire_ground_up_loss() {
        let layer = Layer::full_value(1_000.0);
        assert_eq!(layer.insured_loss(250.0), 250.0);
    }

    #[test]
    fn layer_never_pays_above_its_limit() {
        let layer = Layer { attachment: 0.0, limit: 600.0 };
        assert_eq!(layer.insured_loss(1_000.0), 600.0);
    }

    #[test]
    fn no_claim_arises_below_attachment() {
        let layer = Layer { attachment: 300.0, limit: 700.0 };
        assert_eq!(layer.insured_loss(200.0), 0.0);
        // A loss above attachment pays only the excess over attachment.
        assert_eq!(layer.insured_loss(500.0), 200.0);
    }

    #[test]
    fn settling_a_claim_debits_capital_by_the_settled_amount() {
        let mut syndicate = Syndicate::with_capital(1_000.0);
        let settlement = syndicate.settle(250.0);
        // The settled amount equals the insured loss; capital is debited by it.
        assert_eq!(settlement.settled, 250.0);
        assert_eq!(settlement.shortfall, 0.0);
        assert_eq!(syndicate.capital(), 750.0);
    }

    #[test]
    fn a_full_value_loss_flows_to_a_panel_of_one_and_debits_capital() {
        // End-to-end attritional settlement cascade for a panel of one:
        // occurrence → GUL → full-value layer → settled amount → capital debit.
        let sum_insured = 1_000.0;
        let mut syndicate = Syndicate::with_capital(5_000.0);
        let layer = Layer::full_value(sum_insured);

        let gul = ground_up_loss(0.4, sum_insured);
        let insured_loss = layer.insured_loss(gul);
        let settlement = syndicate.settle(insured_loss);

        assert_eq!(gul, 400.0);
        assert_eq!(insured_loss, gul); // full-value layer: settled == insured loss == GUL
        assert_eq!(settlement.settled, insured_loss);
        assert_eq!(syndicate.capital(), 4_600.0);
    }

    #[test]
    fn coefficient_of_variation_of_a_constant_sample_is_zero() {
        assert_eq!(coefficient_of_variation(&[5.0, 5.0, 5.0]), 0.0);
    }

    #[test]
    fn aggregate_attritional_cv_falls_as_one_over_sqrt_n() {
        // The central risk-pooling diagnostic invariant: as the pool grows, the
        // CV of the insurer's aggregate attritional loss falls as ~1/√N — so
        // quadrupling N roughly halves the CV. We assert the *trend* with a
        // tolerance, not exact values.
        let peril = AttritionalPeril { occurrence_probability: 0.25, mean_damage_fraction: 0.1 };
        let trials = 4_000;
        let mut rng = Rng::seeded(2024);

        // Pool sizes quadrupling each step: CV should roughly halve each step.
        let pool_sizes = [50usize, 200, 800, 3_200];
        let cvs: Vec<f64> = pool_sizes
            .iter()
            .map(|&n| {
                let samples = attritional_aggregate_samples(n, 1_000.0, &peril, trials, &mut rng);
                coefficient_of_variation(&samples)
            })
            .collect();

        // CV strictly decreases with pool size.
        for window in cvs.windows(2) {
            assert!(window[1] < window[0], "CV did not fall as N grew: {cvs:?}");
        }

        // Each quadrupling of N should halve the CV (ratio ≈ 0.5). Allow a
        // generous tolerance band around the 1/√N law.
        for window in cvs.windows(2) {
            let ratio = window[1] / window[0];
            assert!(
                (0.40..0.60).contains(&ratio),
                "CV ratio {ratio} per 4x pool growth not near 0.5 (1/√4); cvs = {cvs:?}"
            );
        }

        // End-to-end: 64x the pool (50 → 3200) should cut the CV ~8-fold (√64).
        let overall = cvs[0] / cvs[3];
        assert!((6.5..9.5).contains(&overall), "overall CV compression {overall} not near 8");
    }

    fn uniform_pool(n: usize, sum_insured: f64) -> Vec<Asset> {
        (0..n).map(|_| Asset { sum_insured, territory: Territory(0) }).collect()
    }

    #[test]
    fn capital_persists_across_years_with_no_re_endowment() {
        let peril = AttritionalPeril { occurrence_probability: 0.2, mean_damage_fraction: 0.1 };
        let assets = uniform_pool(200, 1_000.0);
        let initial = 1_000_000.0;
        let mut syndicate = Syndicate::with_capital(initial);

        let mut rng = Rng::seeded(99);
        let trajectory = run_attritional_horizon(&mut syndicate, &assets, &peril, 5, &mut rng);

        assert_eq!(trajectory.len(), 5);
        // Capital is drawn down each year (losses occur) and never re-endowed,
        // so the balance is monotonically non-increasing and ends below where
        // it started.
        for window in trajectory.windows(2) {
            assert!(window[1] <= window[0], "capital rose between years: {window:?}");
        }
        assert!(*trajectory.last().unwrap() < initial);

        // No re-endowment: ending capital equals initial minus the total settled.
        let total_settled = initial - syndicate.capital();
        assert!(total_settled > 0.0);
        assert!((syndicate.capital() - trajectory[4]).abs() < 1e-9);
    }

    #[test]
    fn attritional_occurrences_are_drawn_independently_per_asset() {
        // Independence shows up as a *spread* of per-asset outcomes within one
        // period: some assets are struck, some are not. (A catastrophe, by
        // contrast, is a single shared occurrence striking all or none.)
        let peril = AttritionalPeril { occurrence_probability: 0.3, mean_damage_fraction: 0.1 };
        let territory = Territory(0);
        let assets: Vec<Asset> =
            (0..1_000).map(|_| Asset { sum_insured: 1_000.0, territory }).collect();

        let mut rng = Rng::seeded(123);
        let losses: Vec<f64> = assets.iter().map(|a| peril.strike(a, &mut rng)).collect();

        let struck = losses.iter().filter(|&&l| l > 0.0).count();
        let unstruck = losses.len() - struck;
        assert!(struck > 0, "expected some assets to be struck");
        assert!(unstruck > 0, "expected some assets to be spared");
        // Roughly the occurrence probability, not 0% or 100% (which a shared
        // occurrence would produce).
        let hit_rate = struck as f64 / losses.len() as f64;
        assert!((0.25..0.35).contains(&hit_rate), "hit rate {hit_rate} not near 0.3");
    }

    #[test]
    fn a_catastrophe_event_is_a_single_shared_occurrence_across_the_territory() {
        // The defining contrast with attritional: one cat event applies the SAME
        // damage fraction to every exposed asset in the struck territory at once.
        // A panel of assets with heterogeneous sums insured each lose that one
        // fraction of their own value — perfectly correlated, not independent.
        let event = CatastropheEvent { time: 0.5, damage_fraction: 0.3 };
        let assets = [
            Asset { sum_insured: 1_000.0, territory: Territory(0) },
            Asset { sum_insured: 4_000.0, territory: Territory(0) },
        ];
        // Each asset loses 0.3 of its own sum insured: 300 + 1200 = 1500.
        let loss = territory_catastrophe_loss(&assets, &[event]);
        assert_eq!(loss, 0.3 * (1_000.0 + 4_000.0));
    }

    #[test]
    fn catastrophe_loss_never_exceeds_total_sum_insured() {
        // The physical cap survives the shared occurrence: even a degenerate
        // damage fraction beyond 1.0 cannot inflict more than each asset's full
        // replacement value (GUL ≤ sum insured, per asset).
        let event = CatastropheEvent { time: 0.1, damage_fraction: 1.5 };
        let assets = [
            Asset { sum_insured: 1_000.0, territory: Territory(0) },
            Asset { sum_insured: 2_000.0, territory: Territory(0) },
        ];
        let loss = territory_catastrophe_loss(&assets, &[event]);
        assert_eq!(loss, 3_000.0);
    }

    #[test]
    fn catastrophe_severity_is_heavy_tailed_and_bounded_to_the_unit_interval() {
        // A Pareto-style severity: a heavy *body* (most events mild, occasional
        // extreme) with *bounded support* — the damage fraction is clamped into
        // [0, 1], the first of the two stacked domain caps. Heavy-tailed shows up
        // as a mean well below the midpoint yet a tail that reaches the cap.
        let peril = CatastrophePeril {
            annual_frequency: 0.5,
            tail_alpha: 1.4,
            min_damage_fraction: 0.02,
        };
        let mut rng = Rng::seeded(2024);
        let draws: Vec<f64> = (0..50_000).map(|_| peril.draw_damage_fraction(&mut rng)).collect();

        // Bounded support: every draw lies in [0, 1].
        for d in &draws {
            assert!((0.0..=1.0).contains(d), "damage fraction {d} outside [0, 1]");
        }

        // Heavy-tailed body: the bulk of events are mild (median well below the
        // mean), yet the tail occasionally reaches the [0,1] cap.
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        let small = draws.iter().filter(|&&d| d < 0.1).count() as f64 / draws.len() as f64;
        let capped = draws.iter().filter(|&&d| d >= 1.0).count();
        assert!(mean < 0.25, "mean {mean} too high for a heavy-tailed body");
        assert!(small > 0.5, "expected most events mild; only {small} below 0.1");
        assert!(capped > 0, "expected a tail reaching the [0,1] cap; none did");
    }

    #[test]
    fn catastrophe_events_fall_on_a_within_year_time_axis_with_multiple_per_year_possible() {
        // Events arrive on a within-year axis (times in [0, 1), chronological),
        // and the Poisson count means a year can carry zero, one, or several
        // events — the precondition for clustered-event within-year hardening.
        let peril = CatastrophePeril {
            annual_frequency: 1.5,
            min_damage_fraction: 0.02,
            tail_alpha: 1.4,
        };
        let mut rng = Rng::seeded(7);

        let mut counts = Vec::new();
        for _ in 0..5_000 {
            let events = peril.annual_events(&mut rng);
            // Times lie within the year and are in chronological order.
            for window in events.windows(2) {
                assert!(window[0].time <= window[1].time, "events not chronological");
            }
            for e in &events {
                assert!((0.0..1.0).contains(&e.time), "time {} outside [0, 1)", e.time);
                assert!((0.0..=1.0).contains(&e.damage_fraction));
            }
            counts.push(events.len());
        }

        // Some years are quiet, some carry multiple events.
        let quiet = counts.iter().filter(|&&c| c == 0).count();
        let multi = counts.iter().filter(|&&c| c >= 2).count();
        assert!(quiet > 0, "expected some catastrophe-free years");
        assert!(multi > 0, "expected some years with multiple events");

        // The mean event count tracks the configured annual frequency.
        let mean = counts.iter().sum::<usize>() as f64 / counts.len() as f64;
        assert!((1.35..1.65).contains(&mean), "mean event count {mean} off 1.5");
    }

    #[test]
    fn catastrophe_cv_is_flat_in_pool_size_but_falls_when_spread_across_territories() {
        // The catastrophe half of the risk-pooling diagnostic invariant, the
        // structural mirror of the attritional 1/√N law:
        //
        //   * Within ONE territory the cat draw is shared, so growing the pool
        //     scales the loss but cannot diversify the event severity — the
        //     catastrophe-component CV is ~flat in pool size.
        //   * Spreading the SAME total exposure across more *uncorrelated*
        //     territories does diversify (independent cat processes), so the CV
        //     falls ~1/√T. This is the only thing that reduces cat variance.
        let peril = CatastrophePeril {
            annual_frequency: 0.6,
            min_damage_fraction: 0.02,
            tail_alpha: 1.4,
        };
        let trials = 12_000;
        let sum_insured = 1_000.0;

        // --- Flat in N within one territory ---
        // Flatness is exact (the pool size cancels in the CV), so modest pools
        // demonstrate it; we grow N 16× and the CV barely moves.
        let mut rng = Rng::seeded(2024);
        let cvs_by_pool: Vec<f64> = [50usize, 100, 200, 800]
            .iter()
            .map(|&n| {
                let samples =
                    catastrophe_aggregate_samples(1, n, sum_insured, &peril, trials, &mut rng);
                coefficient_of_variation(&samples)
            })
            .collect();

        // The CV barely moves as the pool grows 64×: every ratio stays near 1.
        for window in cvs_by_pool.windows(2) {
            let ratio = window[1] / window[0];
            assert!(
                (0.9..1.1).contains(&ratio),
                "cat CV not flat in pool size: {cvs_by_pool:?}"
            );
        }

        // --- Falls ~1/√T as the same total exposure spreads across territories ---
        // Hold total assets at 1024; split across 1, 4, 16, 64 territories.
        let total = 1_024usize;
        let cvs_by_spread: Vec<f64> = [1usize, 4, 16, 64]
            .iter()
            .map(|&t| {
                let per = total / t;
                let samples =
                    catastrophe_aggregate_samples(t, per, sum_insured, &peril, trials, &mut rng);
                coefficient_of_variation(&samples)
            })
            .collect();

        // CV strictly falls as exposure spreads.
        for window in cvs_by_spread.windows(2) {
            assert!(window[1] < window[0], "cat CV did not fall as spread grew: {cvs_by_spread:?}");
        }
        // Each 4× spread roughly halves the CV (1/√4), with a generous band.
        for window in cvs_by_spread.windows(2) {
            let ratio = window[1] / window[0];
            assert!(
                (0.40..0.62).contains(&ratio),
                "cat CV ratio {ratio} per 4× spread not near 0.5; {cvs_by_spread:?}"
            );
        }
    }

    #[test]
    fn catastrophe_losses_settle_against_capital_within_the_zero_floor() {
        // A correlated cat loss flows through the same settlement cascade as any
        // other loss: it debits capital, never takes a syndicate below zero, and
        // any uncovered remainder is recorded as a shortfall. The shared
        // occurrence is exactly what makes a cat able to exhaust capital where a
        // diversified attritional book would not.
        let peril = CatastrophePeril {
            annual_frequency: 5.0, // forced busy so a loss is essentially certain
            min_damage_fraction: 0.2,
            tail_alpha: 1.4,
        };
        let assets: Vec<Asset> =
            (0..500).map(|_| Asset { sum_insured: 1_000.0, territory: Territory(0) }).collect();

        let mut rng = Rng::seeded(2024);
        let events = peril.annual_events(&mut rng);
        let loss = territory_catastrophe_loss(&assets, &events);
        assert!(loss > 0.0, "expected a catastrophe loss in a busy year");

        // Undercapitalised relative to the loss: settlement floors at zero.
        let mut syndicate = Syndicate::with_capital(loss / 2.0);
        let settlement = syndicate.settle(loss);

        assert_eq!(settlement.settled, loss / 2.0);
        assert!((settlement.shortfall - loss / 2.0).abs() < 1e-9);
        assert_eq!(syndicate.capital(), 0.0);
        assert!(!syndicate.is_solvent());

        // The cat loss respects the physical cap: it cannot exceed total TIV.
        let total_tiv: f64 = assets.iter().map(|a| a.sum_insured).sum();
        assert!(loss <= total_tiv * events.len() as f64);
    }

    #[test]
    fn rng_is_reproducible_from_a_seed() {
        let mut a = Rng::seeded(42);
        let mut b = Rng::seeded(42);
        let seq_a: Vec<f64> = (0..5).map(|_| a.uniform()).collect();
        let seq_b: Vec<f64> = (0..5).map(|_| b.uniform()).collect();
        assert_eq!(seq_a, seq_b);
    }

    #[test]
    fn rng_uniform_draws_lie_in_the_unit_interval() {
        let mut rng = Rng::seeded(7);
        for _ in 0..10_000 {
            let u = rng.uniform();
            assert!((0.0..1.0).contains(&u), "draw {u} outside [0, 1)");
        }
    }

    #[test]
    fn capital_payments_never_take_a_syndicate_below_zero() {
        let mut syndicate = Syndicate::with_capital(100.0);
        let settlement = syndicate.settle(250.0);
        assert_eq!(settlement.settled, 100.0);
        assert_eq!(settlement.shortfall, 150.0);
        assert_eq!(syndicate.capital(), 0.0);
        assert!(!syndicate.is_solvent());
    }

    #[test]
    fn the_full_cascade_settles_a_multi_layer_tower_on_multi_member_panels() {
        // End-to-end settlement invariant: a GUL flows up a placed tower whose
        // layers sit on multi-member panels; each penetrated, in-force layer's
        // settled amounts sum to its insured loss, and the tower's total payout
        // never exceeds the GUL.
        let tower = vec![
            PlacedLayer {
                layer: Layer { attachment: 0.0, limit: 100.0 },
                panel: Panel::subscribe(&[SyndicateId(0), SyndicateId(1)], 1.0),
                inception: 0.0,
                expiry: 1.0,
            },
            PlacedLayer {
                layer: Layer { attachment: 100.0, limit: 300.0 },
                panel: Panel::subscribe(&[SyndicateId(1), SyndicateId(2), SyndicateId(0)], 1.0),
                inception: 0.0,
                expiry: 1.0,
            },
        ];
        let gul = 250.0; // fills layer 0 (100), penetrates layer 1 by 150
        let mut syndicates = vec![
            Syndicate::with_capital(1_000.0),
            Syndicate::with_capital(1_000.0),
            Syndicate::with_capital(1_000.0),
        ];

        let settlements = settle_placed_tower(&tower, gul, 0.5, &mut syndicates);

        // Per-layer panel sizes: 2 + 3 entries all in force.
        assert_eq!(settlements.len(), 5);
        // Settled amounts across the whole tower sum to the tower's aggregate
        // insured loss (fully placed, all solvent), which is min(GUL, top) = 250.
        let total_settled: f64 = settlements.iter().map(|s| s.settled).sum();
        let aggregate = Tower::new(tower.iter().map(|p| p.layer).collect()).aggregate_insured_loss(gul);
        assert!((total_settled - aggregate).abs() < 1e-9);
        assert!((total_settled - 250.0).abs() < 1e-9);
        // The tower never pays more than the ground-up loss.
        assert!(total_settled <= gul + 1e-9);
        // No shortfalls while every member is well capitalised.
        assert!(settlements.iter().all(|s| s.shortfall == 0.0));
    }

    #[test]
    fn ground_up_loss_flows_up_a_tower_of_consecutive_layers() {
        // A tower is a stack of independent layers over the same ground-up loss.
        // Bands [0, 100], [100, 400], [400, 1000]: a GUL of 600 fills the first
        // two layers and penetrates the third by 200.
        let tower = Tower::new(vec![
            Layer { attachment: 0.0, limit: 100.0 },
            Layer { attachment: 100.0, limit: 300.0 },
            Layer { attachment: 400.0, limit: 600.0 },
        ]);
        let losses = tower.insured_losses(600.0);
        assert_eq!(losses, vec![100.0, 300.0, 200.0]);
        // The tower's aggregate payout equals min(GUL, top of tower) and so never
        // exceeds the ground-up loss.
        assert_eq!(tower.aggregate_insured_loss(600.0), 600.0);
    }

    #[test]
    fn a_panel_subscribes_equal_shares_summing_to_the_placed_portion() {
        // The trivial deterministic placement rule: the broker assembles a panel
        // of equal shares over the shortlisted syndicates, summing to the placed
        // portion, with the first designated lead and the rest followers.
        let panel = Panel::subscribe(
            &[SyndicateId(2), SyndicateId(0), SyndicateId(1)],
            1.0,
        );
        // Shares sum to the placed portion (fully placed here).
        assert!((panel.placed_portion() - 1.0).abs() < 1e-12);
        // Equal shares.
        for entry in &panel.entries {
            assert!((entry.share - 1.0 / 3.0).abs() < 1e-12);
        }
        // First is lead, the rest are followers, preserving the shortlist order.
        assert_eq!(panel.lead().syndicate, SyndicateId(2));
        let followers: Vec<SyndicateId> = panel.followers().iter().map(|e| e.syndicate).collect();
        assert_eq!(followers, vec![SyndicateId(0), SyndicateId(1)]);
    }

    #[test]
    fn a_panel_of_one_is_a_lead_with_no_followers() {
        // Single-syndicate placement is a panel of one, not a separate mode.
        let panel = Panel::subscribe(&[SyndicateId(7)], 0.8);
        assert_eq!(panel.lead().syndicate, SyndicateId(7));
        assert!((panel.lead().share - 0.8).abs() < 1e-12);
        assert!(panel.followers().is_empty());
        assert!((panel.placed_portion() - 0.8).abs() < 1e-12);
    }

    #[test]
    fn settlement_pro_rates_a_layer_loss_across_the_panel_by_share() {
        // A penetrated layer's net insured loss is pro-rated across its panel by
        // share; each syndicate's capital is debited its share, and the settled
        // amounts sum to the layer's insured loss (a fully placed panel).
        let panel = Panel::subscribe(&[SyndicateId(0), SyndicateId(1), SyndicateId(2)], 1.0);
        let mut syndicates = vec![
            Syndicate::with_capital(10_000.0),
            Syndicate::with_capital(10_000.0),
            Syndicate::with_capital(10_000.0),
        ];
        let insured_loss = 900.0;
        let settlements = panel.settle(insured_loss, &mut syndicates);

        // Each member pays its share of the loss.
        for s in &settlements {
            assert!((s.settled - 300.0).abs() < 1e-9);
            assert_eq!(s.shortfall, 0.0);
        }
        // The settled amounts sum to the layer's insured loss.
        let total: f64 = settlements.iter().map(|s| s.settled).sum();
        assert!((total - insured_loss).abs() < 1e-9);
        // Capital is debited by each member's share.
        for s in &syndicates {
            assert!((s.capital() - 9_700.0).abs() < 1e-9);
        }
    }

    #[test]
    fn several_liability_leaves_an_insolvent_members_shortfall_with_the_insured() {
        // Zero-floor partial settlement under several liability: an insolvent
        // member pays min(share·loss, capital); the remainder is an unrecovered
        // shortfall borne by the insured, NEVER redistributed to co-subscribers.
        let panel = Panel::subscribe(&[SyndicateId(0), SyndicateId(1), SyndicateId(2)], 1.0);
        let mut syndicates = vec![
            Syndicate::with_capital(10_000.0), // solvent co-subscriber
            Syndicate::with_capital(100.0),    // undercapitalised: share is 300
            Syndicate::with_capital(10_000.0), // solvent co-subscriber
        ];
        let insured_loss = 900.0; // each share = 300

        let settlements = panel.settle(insured_loss, &mut syndicates);

        // The insolvent member pays only what it has; the rest is its shortfall.
        assert_eq!(settlements[1].settled, 100.0);
        assert!((settlements[1].shortfall - 200.0).abs() < 1e-9);
        assert_eq!(syndicates[1].capital(), 0.0);
        assert!(!syndicates[1].is_solvent());

        // Co-subscribers pay exactly their own share — the shortfall is NOT
        // redistributed onto them.
        assert!((settlements[0].settled - 300.0).abs() < 1e-9);
        assert!((settlements[2].settled - 300.0).abs() < 1e-9);
        assert_eq!(settlements[0].shortfall, 0.0);
        assert_eq!(settlements[2].shortfall, 0.0);
        assert!((syndicates[0].capital() - 9_700.0).abs() < 1e-9);
        assert!((syndicates[2].capital() - 9_700.0).abs() < 1e-9);

        // The insured bears the gap: settled + shortfall reconstruct the loss.
        let settled: f64 = settlements.iter().map(|s| s.settled).sum();
        let shortfall: f64 = settlements.iter().map(|s| s.shortfall).sum();
        assert!((settled - 700.0).abs() < 1e-9);
        assert!((shortfall - 200.0).abs() < 1e-9);
        assert!((settled + shortfall - insured_loss).abs() < 1e-9);
    }

    #[test]
    fn a_placed_layer_is_in_force_only_between_inception_and_expiry() {
        let placed = PlacedLayer {
            layer: Layer { attachment: 0.0, limit: 100.0 },
            panel: Panel::subscribe(&[SyndicateId(0)], 1.0),
            inception: 0.25,
            expiry: 0.75,
        };
        assert!(!placed.is_in_force(0.1)); // before inception
        assert!(placed.is_in_force(0.25)); // at inception
        assert!(placed.is_in_force(0.5)); // mid-term
        assert!(!placed.is_in_force(0.75)); // at expiry (exclusive)
        assert!(!placed.is_in_force(0.9)); // after expiry
    }

    #[test]
    fn the_cascade_settles_in_force_layers_and_expired_layers_generate_no_claims() {
        // The settlement cascade flows a GUL up a placed tower at a date: in-force
        // layers settle on their panels; expired layers generate no claims.
        let tower = vec![
            PlacedLayer {
                layer: Layer { attachment: 0.0, limit: 100.0 },
                panel: Panel::subscribe(&[SyndicateId(0)], 1.0),
                inception: 0.0,
                expiry: 1.0, // in force at the settlement date
            },
            PlacedLayer {
                layer: Layer { attachment: 100.0, limit: 300.0 },
                panel: Panel::subscribe(&[SyndicateId(1)], 1.0),
                inception: 0.0,
                expiry: 0.5, // already expired at the settlement date
            },
        ];
        let mut syndicates = vec![
            Syndicate::with_capital(10_000.0),
            Syndicate::with_capital(10_000.0),
        ];

        // GUL of 600 would fill layer 0 (100) and penetrate layer 1 (300).
        let settlements = settle_placed_tower(&tower, 600.0, 0.7, &mut syndicates);

        // Only the in-force layer settles.
        assert_eq!(settlements.len(), 1);
        assert!((settlements[0].settled - 100.0).abs() < 1e-9);
        // Layer 0's panel member is debited; the expired layer's member is not.
        assert!((syndicates[0].capital() - 9_900.0).abs() < 1e-9);
        assert_eq!(syndicates[1].capital(), 10_000.0);
    }

    #[test]
    fn an_insolvent_syndicate_in_runoff_writes_no_new_business() {
        // Insolvency triggers runoff: the syndicate is no longer available for new
        // placements, so panel formation draws only from the solvent roster.
        let syndicates = vec![
            Syndicate::with_capital(10_000.0),
            Syndicate::with_capital(0.0), // insolvent → in runoff
            Syndicate::with_capital(10_000.0),
        ];
        let available = available_for_new_business(&syndicates);
        assert_eq!(available, vec![SyndicateId(0), SyndicateId(2)]);

        // A new panel formed from the available roster excludes the runoff member.
        let panel = Panel::subscribe(&available, 1.0);
        assert!(panel.entries.iter().all(|e| e.syndicate != SyndicateId(1)));
    }

    #[test]
    fn a_syndicate_in_runoff_settles_in_force_layers_until_they_expire() {
        // Runoff is a gradual withdrawal: an insolvent syndicate still settles its
        // in-force layers (against the zero floor) until they expire; expired
        // layers generate no claims. Here the syndicate is already exhausted, so
        // the in-force layer is still processed but pays nothing — the shortfall
        // falls to the insured — while the expired layer is skipped entirely.
        let tower = vec![
            PlacedLayer {
                layer: Layer { attachment: 0.0, limit: 100.0 },
                panel: Panel::subscribe(&[SyndicateId(0)], 1.0),
                inception: 0.0,
                expiry: 1.0, // still in force
            },
            PlacedLayer {
                layer: Layer { attachment: 100.0, limit: 300.0 },
                panel: Panel::subscribe(&[SyndicateId(0)], 1.0),
                inception: 0.0,
                expiry: 0.5, // already expired
            },
        ];
        let mut syndicates = vec![Syndicate::with_capital(0.0)]; // in runoff
        assert!(!syndicates[0].is_solvent());

        let settlements = settle_placed_tower(&tower, 600.0, 0.7, &mut syndicates);

        // The in-force layer is still settled (the obligation survives runoff),
        // paying nothing with the whole loss recorded as an unrecovered shortfall.
        assert_eq!(settlements.len(), 1);
        assert_eq!(settlements[0].settled, 0.0);
        assert!((settlements[0].shortfall - 100.0).abs() < 1e-9);
        // The expired layer generated no claim at all.
    }

    #[test]
    fn a_broker_holds_a_relationship_score_per_syndicate() {
        // A broker is a stateful agent: it carries a relationship score per
        // syndicate (indexed by SyndicateId) and a broker-level update inertia.
        // Syndicates the broker has never dealt with score zero (the new-entrant
        // starting point — relationships are earned, not given).
        let broker = Broker::new(vec![0.8, 0.2, 0.0], 0.9);
        assert_eq!(broker.relationship(SyndicateId(0)), 0.8);
        assert_eq!(broker.relationship(SyndicateId(1)), 0.2);
        assert_eq!(broker.relationship(SyndicateId(2)), 0.0);
        // A syndicate beyond the known roster is an unknown — score zero.
        assert_eq!(broker.relationship(SyndicateId(9)), 0.0);
        assert_eq!(broker.inertia, 0.9);
    }

    #[test]
    fn the_broker_shortlists_by_relationship_and_leads_the_strongest_not_the_cheapest() {
        // Routing is relationship-driven, never price-driven: shortlist() takes no
        // prices at all. With one dominant relationship and two weak ones, the
        // broker shortlists the strong syndicate almost always and designates it
        // lead (the strongest relationship is first). A would-be cheaper rival has
        // no way to buy its way onto the lead slot — price is simply not an input.
        let broker = Broker::new(vec![10.0, 0.1, 0.1], 0.9);
        let capable = [SyndicateId(0), SyndicateId(1), SyndicateId(2)];

        let mut led_by_strong = 0;
        let mut shortlisted_strong = 0;
        let trials = 2_000;
        for s in 0..trials {
            let mut rng = Rng::seeded(1_000 + s as u64);
            let shortlist = broker.shortlist(&capable, 2, &mut rng);
            assert_eq!(shortlist.len(), 2, "shortlist of the requested size");
            // The lead is always the strongest relationship on the shortlist.
            let lead = shortlist[0];
            let lead_rel = broker.relationship(lead);
            for &member in &shortlist[1..] {
                assert!(broker.relationship(member) <= lead_rel, "lead is not the strongest");
            }
            if lead == SyndicateId(0) {
                led_by_strong += 1;
            }
            if shortlist.contains(&SyndicateId(0)) {
                shortlisted_strong += 1;
            }
        }
        // The strong relationship dominates both shortlisting and the lead slot.
        assert!(shortlisted_strong > 1_950, "strong syndicate shortlisted only {shortlisted_strong}/{trials}");
        assert!(led_by_strong > 1_950, "strong syndicate led only {led_by_strong}/{trials}");
    }

    #[test]
    fn several_brokers_have_heterogeneous_portfolios_and_lead_different_syndicates() {
        // The market has SEVERAL brokers with heterogeneous relationship
        // portfolios — different brokers favour different syndicates, and that
        // heterogeneity is the network topology herding (#3) and the entry lag (#6)
        // ride on. Two brokers over the SAME capable roster lead different
        // syndicates, purely from their own relationship books (and they differ in
        // loyalty via inertia).
        let alpha = Broker::new(vec![9.0, 0.1, 0.1], 0.95); // loyal to syndicate 0
        let beta = Broker::new(vec![0.1, 0.1, 9.0], 0.5); // loyal to syndicate 2
        assert_ne!(alpha.inertia, beta.inertia, "brokers differ in loyalty");
        let capable = [SyndicateId(0), SyndicateId(1), SyndicateId(2)];

        let mut alpha_leads_0 = 0;
        let mut beta_leads_2 = 0;
        let trials = 1_000;
        for s in 0..trials {
            let mut ra = Rng::seeded(20_000 + s as u64);
            let mut rb = Rng::seeded(20_000 + s as u64);
            if alpha.shortlist(&capable, 2, &mut ra)[0] == SyndicateId(0) {
                alpha_leads_0 += 1;
            }
            if beta.shortlist(&capable, 2, &mut rb)[0] == SyndicateId(2) {
                beta_leads_2 += 1;
            }
        }
        assert!(alpha_leads_0 > 950, "broker alpha did not consistently lead its favourite");
        assert!(beta_leads_2 > 950, "broker beta did not consistently lead its favourite");
    }

    #[test]
    fn a_new_entrant_is_occasionally_shortlisted_so_relationships_can_be_built() {
        // New entrants start at zero everywhere, yet must be able to win a little
        // business to build relationships over years (the relational half of the
        // entry lag, #6). So a zero-relationship syndicate is shortlisted rarely
        // but not never, against an incumbent — weighting, not exclusion.
        let broker = Broker::new(vec![5.0, 0.0], 0.9);
        let capable = [SyndicateId(0), SyndicateId(1)];
        let mut entrant_shortlisted = 0;
        let trials = 4_000;
        for s in 0..trials {
            let mut rng = Rng::seeded(7_000 + s as u64);
            if broker.shortlist(&capable, 1, &mut rng).contains(&SyndicateId(1)) {
                entrant_shortlisted += 1;
            }
        }
        assert!(entrant_shortlisted > 0, "new entrant never gets a chance");
        assert!(entrant_shortlisted < trials / 2, "new entrant not rare against an incumbent");
    }

    #[test]
    fn a_syndicate_quotes_blind_from_its_own_technical_premium() {
        // The lead quotes BLIND: its price is its own technical premium scaled by
        // its ask multiplier (AvT; 1.0 until the cycle layer sets it), computed
        // from its own TP alone — blind_quote takes no other quote, so seeing no
        // rival is structural, not a convention. At AvT 1.0 the quote IS the TP;
        // the ask multiplier is the (future) competitive lever around it.
        let model = cat_model();
        let book = NetBook { lines: vec![] };
        let risk = LayerExposure { layer: Layer { attachment: 0.0, limit: 100.0 }, exposure: 1_000.0, territory: Territory(0), reinstatement: ReinstatementTerms::none() };
        let experience = AttritionalExperience { own_burning_cost: 30.0, benchmark: 25.0, volume: 50.0 };
        let params = PricingParams { hurdle_rate: 0.15, credibility_k: 10.0, target_loss_ratio: 0.6, return_period: 200.0, tail_trials: 4_000 };
        let mut rng = Rng::seeded(2024);
        let tp = technical_premium(&risk, &book, &model, &experience, &params, &mut rng).technical_premium;

        let blind = blind_quote(SyndicateId(0), tp, 1.0);
        assert_eq!(blind.syndicate, SyndicateId(0));
        assert_eq!(blind.price, tp);

        // The ask multiplier scales the TP — the lever the competitive AvT channel
        // will eventually move; at 1.2 the lead holds out 20% above its floor.
        let firmer = blind_quote(SyndicateId(0), tp, 1.2);
        assert!((firmer.price - tp * 1.2).abs() < 1e-9);
    }

    #[test]
    fn the_follower_weight_is_derived_from_confidence_reputation_and_susceptibility() {
        // The herding weight w is DERIVED, never a fixed constant (a fixed weight
        // would make #3 tautological). It rises when the follower's OWN estimate
        // is low-confidence (thin own data leaning on the lead, the same
        // information-content logic as credibility) and when the LEAD is more
        // reputable, and it is scaled by the follower's per-syndicate herding
        // susceptibility (a genome parameter). It always lands in [0, 1].

        // Rises as own confidence falls (more willing to lean on the lead).
        let low_conf = follower_weight(0.1, 0.8, 1.0);
        let high_conf = follower_weight(0.9, 0.8, 1.0);
        assert!(low_conf > high_conf, "w should rise as own confidence falls");

        // Rises as lead reputation rises.
        let rep_lead = follower_weight(0.5, 0.9, 1.0);
        let unknown_lead = follower_weight(0.5, 0.1, 1.0);
        assert!(rep_lead > unknown_lead, "w should rise with lead reputation");

        // Scales with herding susceptibility: a more susceptible follower herds
        // harder on identical information.
        let susceptible = follower_weight(0.4, 0.7, 1.5);
        let stubborn = follower_weight(0.4, 0.7, 0.3);
        assert!(susceptible > stubborn, "w should scale with herding susceptibility");

        // Boundary behaviour: a fully confident follower ignores the lead (w = 0);
        // a zero-susceptibility follower never herds (w = 0); w stays in [0, 1]
        // even when the raw product would exceed 1.
        assert_eq!(follower_weight(1.0, 1.0, 2.0), 0.0);
        assert_eq!(follower_weight(0.0, 1.0, 0.0), 0.0);
        let saturated = follower_weight(0.0, 1.0, 5.0);
        assert!((0.0..=1.0).contains(&saturated) && saturated == 1.0);
    }

    #[test]
    fn a_follower_anchors_its_price_toward_the_lead_without_touching_its_belief() {
        // A follower anchors its QUOTE toward the lead's, not its beliefs:
        //   follower_quote = (1 − w)·own + w·lead.
        // At w = 0 it quotes its own blind price; at w = 1 it fully matches the
        // lead; in between it interpolates and moves toward the lead as w rises.
        let own = 100.0;
        let lead = 60.0;
        assert!((anchored_quote(own, lead, 0.0) - own).abs() < 1e-12);
        assert!((anchored_quote(own, lead, 1.0) - lead).abs() < 1e-12);
        let half = anchored_quote(own, lead, 0.5);
        assert!((half - 80.0).abs() < 1e-12);
        // Strictly toward the lead as w grows.
        assert!(anchored_quote(own, lead, 0.7) < anchored_quote(own, lead, 0.3));

        // Belief is NOT overwritten — only the price moves. A follower prices off
        // its OWN cat model, anchors its quote toward a lead, then re-prices: its
        // technical premium is byte-identical, because anchoring touched price
        // alone (this keeps pricing herding #3 orthogonal to model homogeneity
        // #14). anchored_quote cannot even see the model — it takes only prices.
        let model = cat_model();
        let book = NetBook { lines: vec![] };
        let risk = LayerExposure { layer: Layer { attachment: 0.0, limit: 100.0 }, exposure: 1_000.0, territory: Territory(0), reinstatement: ReinstatementTerms::none() };
        let experience = AttritionalExperience { own_burning_cost: 30.0, benchmark: 25.0, volume: 50.0 };
        let params = PricingParams { hurdle_rate: 0.15, credibility_k: 10.0, target_loss_ratio: 0.6, return_period: 200.0, tail_trials: 4_000 };

        let mut rng = Rng::seeded(2024);
        let before = technical_premium(&risk, &book, &model, &experience, &params, &mut rng).technical_premium;
        // Anchor hard toward a very different lead quote.
        let _ = anchored_quote(before, before * 0.3, 0.9);
        let mut rng = Rng::seeded(2024);
        let after = technical_premium(&risk, &book, &model, &experience, &params, &mut rng).technical_premium;
        assert_eq!(before, after, "anchoring a price must not move the follower's belief-driven TP");
    }

    #[test]
    fn a_follower_declines_on_its_own_exposure_limits_regardless_of_the_lead() {
        // Herding moves price, never capacity discipline: a follower applies its
        // OWN exposure limits (#4's ExposurePolicy) and may decline regardless of
        // how attractive the lead quote is. Here a follower already loaded up in a
        // zone trips its cat aggregate, so it declines even though anchoring to the
        // cheap, reputable lead would otherwise be irresistible.
        let model = cat_model();
        let policy = ExposurePolicy { return_period: 200.0, solvency_fraction: 0.5, line_fraction: 0.9, tail_trials: 4_000 };
        let syndicate = Syndicate::with_capital(100.0);
        // A book already heavy in Territory(0): one more line trips the aggregate.
        let loaded = NetBook { lines: (0..60).map(|_| NetLine { territory: Territory(0), net_limit: 1.0 }).collect() };
        let candidate = NetLine { territory: Territory(0), net_limit: 1.0 };
        let mut rng = Rng::seeded(5);
        let decision = policy.assess(&syndicate, &loaded, &model, candidate, &mut rng);
        assert!(matches!(decision, UnderwritingDecision::Decline(DeclineReason::CatAggregate)), "setup: follower should be over-exposed");

        // A cheap, fully-herdable lead quote (w = 1) cannot drag a declined
        // follower onto the panel.
        let response = follower_response(SyndicateId(1), 100.0, 10.0, 1.0, decision);
        assert_eq!(response, FollowerResponse::Decline(DeclineReason::CatAggregate));

        // With headroom the same follower accepts and quotes — anchored toward the
        // lead by w (here pulled fully onto the lead's 10.0).
        let mut rng = Rng::seeded(5);
        let ok = policy.assess(&syndicate, &NetBook { lines: vec![] }, &model, candidate, &mut rng);
        assert_eq!(ok, UnderwritingDecision::Accept);
        let response = follower_response(SyndicateId(1), 100.0, 10.0, 1.0, ok);
        assert_eq!(response, FollowerResponse::Quote(Quote { syndicate: SyndicateId(1), price: 10.0 }));
    }

    #[test]
    fn the_insured_clears_on_the_cheapest_acceptable_quote_within_willingness_to_pay() {
        // An insured's willingness-to-pay is its expected loss scaled by a private
        // risk-aversion loading (> 1). It clears on the CHEAPEST quote at or below
        // WTP — and clearing is a demand-side decision separate from routing, so
        // the relationship-designated lead need not win: a cheaper follower takes
        // the placement.
        let insured = Insured { risk_aversion: 1.4, expected_loss: 100.0 };
        assert!((insured.willingness_to_pay() - 140.0).abs() < 1e-9);

        // Lead (by relationship) quotes 150 — above WTP; two followers quote 120
        // and 135. The cheapest acceptable (≤ 140) is the 120 follower.
        let quotes = [
            Quote { syndicate: SyndicateId(0), price: 150.0 }, // the relationship lead
            Quote { syndicate: SyndicateId(1), price: 135.0 },
            Quote { syndicate: SyndicateId(2), price: 120.0 },
        ];
        let cleared = clear_cheapest(&quotes, insured.willingness_to_pay());
        assert_eq!(cleared, Some(Quote { syndicate: SyndicateId(2), price: 120.0 }));

        // When every quote exceeds WTP the insured declines (no acceptable cover).
        let dear = [
            Quote { syndicate: SyndicateId(0), price: 200.0 },
            Quote { syndicate: SyndicateId(1), price: 160.0 },
        ];
        assert_eq!(clear_cheapest(&dear, insured.willingness_to_pay()), None);
    }

    #[test]
    fn a_priced_out_insured_restructures_its_tower_or_declines() {
        // Demand is price-elastic in QUANTITY (the cycle damper, #1). Facing the
        // cleared price per band, the insured buys the whole tower when it fits its
        // WTP budget; when priced out it drops the worst value-for-money band first
        // (raising retention / lowering limit / self-insuring a tranche); and when
        // even the single most affordable band exceeds WTP it declines entirely.
        let offers = [
            // working/primary: heavily loaded (price/expected = 2.5)
            TowerLayerOffer { layer: Layer { attachment: 0.0, limit: 100.0 }, expected_loss: 60.0, price: 150.0 },
            TowerLayerOffer { layer: Layer { attachment: 100.0, limit: 100.0 }, expected_loss: 20.0, price: 30.0 },
            TowerLayerOffer { layer: Layer { attachment: 200.0, limit: 100.0 }, expected_loss: 5.0, price: 7.0 },
        ];

        // Generous budget: buy the whole tower.
        match restructure_tower(&offers, 200.0) {
            TowerPurchase::Bound { layers, total_price } => {
                assert_eq!(layers.len(), 3, "full tower should bind within a generous WTP");
                assert!((total_price - 187.0).abs() < 1e-9);
            }
            other => panic!("expected the full tower to bind, got {other:?}"),
        }

        // Priced out at WTP 50: the primary band (worst loading) is dropped —
        // retention rises to 100 — and the upper bands bind within budget.
        match restructure_tower(&offers, 50.0) {
            TowerPurchase::Bound { layers, total_price } => {
                assert_eq!(layers.len(), 2, "should restructure to a smaller tower");
                assert!(layers.iter().all(|l| l.attachment >= 100.0), "primary band should be dropped, raising retention");
                assert!(total_price <= 50.0, "restructured price {total_price} must fit WTP");
            }
            other => panic!("expected a restructured tower, got {other:?}"),
        }

        // WTP below even the cheapest single band: no acceptable cover → decline.
        assert_eq!(restructure_tower(&offers, 5.0), TowerPurchase::Declined);
    }

    #[test]
    fn experience_rating_surcharges_chronic_loss_generators_so_the_pool_self_selects() {
        // Experience rating (#9) is supply-side pricing on a demand-side attribute:
        // a syndicate scales its loss-cost estimate for a SPECIFIC risk by that
        // insured's own loss history, credibility-weighted. A clean history earns a
        // credit (modifier < 1), a chronic one a surcharge (modifier > 1), and an
        // insured with no own history is unrated (modifier = 1).
        let k = 10.0;
        let n = 20.0; // Z = 20/30 ≈ 0.667
        assert!(experience_modifier(40.0, 100.0, n, k) < 1.0, "a clean history should earn a credit");
        assert!(experience_modifier(250.0, 100.0, n, k) > 1.0, "a chronic history should be surcharged");
        assert!((experience_modifier(123.0, 100.0, 0.0, k) - 1.0).abs() < 1e-12, "no own history → unrated");

        // Self-selection emerges where per-risk rating meets the WTP exit. The same
        // syndicate prices a base loss cost of 60 (loaded ÷ 0.6 TLR = 100) for
        // three insureds differing only in loss history; each clears against its
        // own WTP (130). The chronic loss-generator is surcharged beyond WTP and
        // declines out of the pool, while clean and neutral risks bind — no cull.
        let base_loss_cost = 60.0;
        let tlr = 0.6;
        let histories = [
            ("clean", 30.0),
            ("neutral", 100.0),
            ("chronic", 300.0),
        ];
        let mut bound = Vec::new();
        for (label, own_losses) in histories {
            let insured = Insured { risk_aversion: 1.3, expected_loss: 100.0 }; // WTP = 130
            let modifier = experience_modifier(own_losses, 100.0, n, k);
            let price = actuarial_technical_price(base_loss_cost, tlr) * modifier;
            let quote = Quote { syndicate: SyndicateId(0), price };
            if clear_cheapest(&[quote], insured.willingness_to_pay()).is_some() {
                bound.push(label);
            }
        }
        assert!(bound.contains(&"clean"), "the clean risk should clear");
        assert!(bound.contains(&"neutral"), "the neutral risk should clear");
        assert!(!bound.contains(&"chronic"), "the chronic loss-generator should be priced out of the pool");
    }

    #[test]
    fn relationship_scores_update_slowly_with_broker_inertia_so_renewals_stay_sticky() {
        // Relationship scores update slowly at year-end with broker-level inertia:
        //   new = inertia·old + (1 − inertia)·signal.
        // A single good year barely moves the score (the lag), and that lag is
        // where renewal stickiness (#5) emerges — it is not a hardcoded stickiness
        // factor.
        let mut broker = Broker::new(vec![0.5], 0.9);
        broker.update_relationship(SyndicateId(0), RelationshipOutcome { quoted: true, won: true, solvent: true });
        let after_one_win = broker.relationship(SyndicateId(0));
        // Winning pushes the score up, but only a little — slow, not a jump to the
        // signal.
        assert!(after_one_win > 0.5, "a winning year should raise the score");
        assert!(after_one_win < 0.56, "the update is slow under high inertia, not a jump: {after_one_win}");

        // Stickiness across renewals: a well-established incumbent keeps being the
        // strongest relationship (and so the lead) for years even as a brand-new
        // challenger wins business every year — relationships trail the competitive
        // landscape. The incumbent merely quotes and loses; the challenger wins.
        let mut broker = Broker::new(vec![0.8, 0.0], 0.9);
        let incumbent = SyndicateId(0);
        let challenger = SyndicateId(1);
        let mut incumbent_led_for = 0;
        for year in 0..30 {
            if broker.relationship(incumbent) >= broker.relationship(challenger) {
                incumbent_led_for = year;
            } else {
                break;
            }
            // The challenger wins; the incumbent quotes competitively but loses.
            broker.update_relationship(challenger, RelationshipOutcome { quoted: true, won: true, solvent: true });
            broker.update_relationship(incumbent, RelationshipOutcome { quoted: true, won: false, solvent: true });
        }
        assert!(incumbent_led_for >= 5, "incumbent lost the lead after only {incumbent_led_for} renewals — not sticky enough");
        // But stickiness is a lag, not a lock: the persistently winning challenger
        // does eventually overtake — share adjusts slowly, it does adjust.
        assert!(incumbent_led_for < 30, "challenger never overtook — that would be a hardcoded lock, not a lag");

        // Insolvency is the strongest erosion signal — it drives the score toward
        // zero faster than a mere decline.
        let mut broker = Broker::new(vec![0.9], 0.9);
        broker.update_relationship(SyndicateId(0), RelationshipOutcome { quoted: false, won: false, solvent: false });
        let after_insolvency = broker.relationship(SyndicateId(0));
        broker = Broker::new(vec![0.9], 0.9);
        broker.update_relationship(SyndicateId(0), RelationshipOutcome { quoted: true, won: false, solvent: true });
        let after_decline = broker.relationship(SyndicateId(0));
        assert!(after_insolvency < after_decline, "insolvency should erode trust faster than a decline");
    }

    #[test]
    fn herding_emerges_as_quote_clustering_behind_a_reputable_lead() {
        // The #3 emergent test, composing the derived weight and the price anchor:
        // followers with dispersed own beliefs but POOR own info, facing a
        // REPUTABLE lead, derive a high w and cluster their quotes tightly around
        // the lead. The SAME followers, confident in their own estimates (or facing
        // an unknown lead), derive w ≈ 0 and stay dispersed. The clustering is not
        // imposed — it falls out of follower_weight × anchored_quote.
        let own_prices = [80.0, 100.0, 120.0, 140.0]; // heterogeneous beliefs
        let lead_quote = 100.0;
        let susceptibility = 1.0;

        // Low own-confidence + reputable lead → high w → clustering.
        let w_herd = follower_weight(0.1, 0.9, susceptibility);
        let clustered: Vec<f64> = own_prices.iter().map(|&p| anchored_quote(p, lead_quote, w_herd)).collect();

        // High own-confidence → w ≈ 0 → quotes stay near own beliefs (dispersed).
        let w_indep = follower_weight(0.95, 0.9, susceptibility);
        let dispersed: Vec<f64> = own_prices.iter().map(|&p| anchored_quote(p, lead_quote, w_indep)).collect();

        let spread_clustered = coefficient_of_variation(&clustered);
        let spread_dispersed = coefficient_of_variation(&dispersed);

        // Herding sharply compresses the spread of quotes.
        assert!(
            spread_clustered < 0.3 * spread_dispersed,
            "clustered spread {spread_clustered} not far below dispersed {spread_dispersed}"
        );
        // And the clustered quotes sit tightly around the reputable lead.
        for q in &clustered {
            assert!((q - lead_quote).abs() < 0.25 * lead_quote, "quote {q} did not cluster near the lead {lead_quote}");
        }
        // A facsimile of #14-orthogonality: clustering is on PRICE; the dispersion
        // of underlying beliefs (own_prices) is untouched by the blend.
        assert!(coefficient_of_variation(&own_prices) > spread_clustered);
    }

    #[test]
    fn a_tower_never_pays_more_in_aggregate_than_the_ground_up_loss() {
        // Even a GUL beyond the top of the tower is capped at the tower's limit,
        // which is below the GUL — the aggregate can never exceed the GUL.
        let tower = Tower::new(vec![
            Layer { attachment: 0.0, limit: 100.0 },
            Layer { attachment: 100.0, limit: 300.0 },
        ]);
        for &gul in &[0.0, 50.0, 250.0, 400.0, 1_000.0] {
            assert!(tower.aggregate_insured_loss(gul) <= gul);
        }
        // A GUL above the tower top (400) is capped at 400.
        assert_eq!(tower.aggregate_insured_loss(1_000.0), 400.0);
    }

    #[test]
    fn a_single_event_settles_on_the_original_limit_with_no_reinstatement_premium() {
        // A cat XoL layer carries reinstatement terms (count + factor). The
        // original limit is "free": a single event drawing within it settles its
        // claim across the panel and triggers no reinstatement premium, so the
        // panel's capital is debited the claim and credited nothing.
        let layer = Layer { attachment: 100.0, limit: 200.0 };
        let panel = Panel::subscribe(&[SyndicateId(0), SyndicateId(1)], 1.0);
        let terms = ReinstatementTerms { count: 1, factor: 1.0 };
        let mut placed = ReinstatementLayer::new(layer, panel, terms, 50.0, 0.0, 1.0);

        let mut syndicates =
            vec![Syndicate::with_capital(10_000.0), Syndicate::with_capital(10_000.0)];
        // GUL 250 → layer loss = clamp(250 − 100, 0, 200) = 150, within the limit.
        let outcome = placed.absorb_event(250.0, 0.5, &mut syndicates);

        let settled: f64 = outcome.claim.iter().map(|s| s.settled).sum();
        assert!((settled - 150.0).abs() < 1e-9);
        assert_eq!(outcome.reinstatement_premium, 0.0, "the free original limit triggers no reinstatement");
        assert_eq!(outcome.uncovered, 0.0);
        // Each member pays its 0.5 share of the 150 claim; nothing credited back.
        assert!((syndicates[0].capital() - 9_925.0).abs() < 1e-9);
        assert!((syndicates[1].capital() - 9_925.0).abs() < 1e-9);
    }

    #[test]
    fn a_second_event_triggers_a_reinstatement_premium_crediting_the_panel_in_the_same_year() {
        // Once the original limit is eroded, a later event draws on a reinstatement:
        // the layer pays the claim AND charges a reinstatement premium pro-rated to
        // the fraction of limit reinstated (factor × premium × reinstated/limit).
        // That income credits the panel's capital by share, in the same year as the
        // loss — the mirror of the settlement cascade.
        let layer = Layer { attachment: 0.0, limit: 100.0 };
        let panel = Panel::subscribe(&[SyndicateId(0), SyndicateId(1)], 1.0);
        let terms = ReinstatementTerms { count: 1, factor: 1.0 };
        let premium = 40.0;
        let mut placed = ReinstatementLayer::new(layer, panel, terms, premium, 0.0, 1.0);

        let mut syndicates =
            vec![Syndicate::with_capital(10_000.0), Syndicate::with_capital(10_000.0)];

        // Event 1 (early) erodes the full original limit: claim 100, no reinstatement.
        let first = placed.absorb_event(100.0, 0.2, &mut syndicates);
        assert_eq!(first.reinstatement_premium, 0.0);

        // Event 2 (later) erodes the full limit again — entirely above the original
        // limit, so it consumes one full reinstatement: premium = 1.0 × 40 × 1.0.
        let second = placed.absorb_event(100.0, 0.6, &mut syndicates);
        let claim: f64 = second.claim.iter().map(|s| s.settled).sum();
        assert!((claim - 100.0).abs() < 1e-9, "the reinstated limit pays the second claim in full");
        assert!((second.reinstatement_premium - 40.0).abs() < 1e-9, "a full reinstatement at 100% of premium");
        assert_eq!(second.uncovered, 0.0);

        // The reinstatement income is credited pro-rata by share (0.5 each = 20).
        assert_eq!(second.reinstatement_credits.len(), 2);
        for c in &second.reinstatement_credits {
            assert!((c - 20.0).abs() < 1e-9);
        }
        // Net capital per member over the year: −50 (two 50-share claims) + 20 credit.
        for s in &syndicates {
            assert!((s.capital() - (10_000.0 - 100.0 + 20.0)).abs() < 1e-9);
        }
    }

    #[test]
    fn reinstatements_are_finite_so_an_exhausted_layer_stays_eroded() {
        // Reinstatements are finite: with count = 1 the aggregate cover is
        // (1 + 1) × limit = two full limits. A third full-limit event finds the
        // aggregate exhausted — no further reinstatement, no claim paid, and the
        // whole loss is uncovered. The layer stays eroded for the rest of the term.
        let layer = Layer { attachment: 0.0, limit: 100.0 };
        let panel = Panel::subscribe(&[SyndicateId(0)], 1.0);
        let terms = ReinstatementTerms { count: 1, factor: 1.0 };
        let mut placed = ReinstatementLayer::new(layer, panel, terms, 30.0, 0.0, 1.0);
        assert!((placed.aggregate_limit() - 200.0).abs() < 1e-9);

        let mut syndicates = vec![Syndicate::with_capital(10_000.0)];

        placed.absorb_event(100.0, 0.1, &mut syndicates); // uses original limit
        placed.absorb_event(100.0, 0.4, &mut syndicates); // uses the one reinstatement
        assert_eq!(placed.remaining_limit(), 0.0, "both limits consumed");

        // Third event: nothing left to pay, no reinstatement to charge, all uncovered.
        let third = placed.absorb_event(100.0, 0.8, &mut syndicates);
        let claim: f64 = third.claim.iter().map(|s| s.settled).sum();
        assert_eq!(claim, 0.0, "an exhausted layer pays no further claim");
        assert_eq!(third.reinstatement_premium, 0.0, "no reinstatement remains to charge");
        assert!((third.uncovered - 100.0).abs() < 1e-9, "the whole loss is uncovered");
    }

    #[test]
    fn a_year_of_events_is_absorbed_chronologically_with_out_of_force_events_skipped() {
        // The within-year time axis drives reinstatement: a layer absorbs a year's
        // events over its exposure in the chronological order they arrive. Each
        // shared occurrence strikes the whole exposure (gul = damage_fraction ×
        // exposure); the first in-force event uses the original limit, a later one
        // triggers a reinstatement, and events outside the cover window generate
        // nothing.
        let layer = Layer { attachment: 0.0, limit: 100.0 };
        let panel = Panel::subscribe(&[SyndicateId(0)], 1.0);
        let terms = ReinstatementTerms { count: 1, factor: 1.0 };
        let mut placed = ReinstatementLayer::new(layer, panel, terms, 20.0, 0.25, 0.75);
        let exposure = 100.0;

        // Chronological events (as annual_events returns them). The 0.1 event is
        // before inception and the 0.9 event after expiry — both skipped.
        let events = [
            CatastropheEvent { time: 0.1, damage_fraction: 1.0 }, // pre-inception
            CatastropheEvent { time: 0.3, damage_fraction: 1.0 }, // in force: original limit
            CatastropheEvent { time: 0.6, damage_fraction: 1.0 }, // in force: reinstatement
            CatastropheEvent { time: 0.9, damage_fraction: 1.0 }, // post-expiry
        ];

        let mut syndicates = vec![Syndicate::with_capital(10_000.0)];
        let settlements = placed.absorb_year(&events, exposure, &mut syndicates);

        assert_eq!(settlements.len(), 4, "one settlement per event, in order");
        // Out-of-force events generate nothing.
        for skipped in [0usize, 3] {
            assert!(settlements[skipped].claim.is_empty());
            assert_eq!(settlements[skipped].reinstatement_premium, 0.0);
            assert_eq!(settlements[skipped].uncovered, 0.0);
        }
        // The first in-force event uses the free original limit; the second triggers
        // the reinstatement (charged 1.0 × 20 × 1.0 = 20).
        assert_eq!(settlements[1].reinstatement_premium, 0.0);
        assert!((settlements[2].reinstatement_premium - 20.0).abs() < 1e-9);
        // Only the two in-force claims (100 each) were settled; one reinstatement
        // credited 20 back.
        assert!((syndicates[0].capital() - (10_000.0 - 200.0 + 20.0)).abs() < 1e-9);
    }

    #[test]
    fn the_quoted_premium_incorporates_the_expected_reinstatement_cost() {
        // The quoted technical premium folds in the layer's reinstatement terms.
        // Reinstatement premiums are extra income the layer collects when losses
        // recur within the year, so a cat XoL layer carrying reinstatements quotes
        // BELOW an otherwise-identical layer with none — the expected
        // reinstatement-premium income credits the base price. The credit is
        // model-anchored (read off the cat model, like the cat ELF) and wired into
        // the same technical_premium path.
        let model = cat_model();
        let book = NetBook { lines: vec![] };
        let exposure = 1_000.0;
        let layer = Layer { attachment: 0.0, limit: 100.0 };
        let experience = AttritionalExperience { own_burning_cost: 30.0, benchmark: 25.0, volume: 50.0 };
        let params = PricingParams { hurdle_rate: 0.15, credibility_k: 10.0, target_loss_ratio: 0.6, return_period: 200.0, tail_trials: 8_000 };

        let bare = LayerExposure { layer, exposure, territory: Territory(0), reinstatement: ReinstatementTerms::none() };
        let with_reinstatement = LayerExposure {
            layer,
            exposure,
            territory: Territory(0),
            reinstatement: ReinstatementTerms { count: 1, factor: 1.0 },
        };

        let mut rng = Rng::seeded(2024);
        let tp_bare = technical_premium(&bare, &book, &model, &experience, &params, &mut rng);
        let mut rng = Rng::seeded(2024);
        let tp_reinst = technical_premium(&with_reinstatement, &book, &model, &experience, &params, &mut rng);

        // The bare layer carries no reinstatement credit and prices exactly as the
        // pre-reinstatement TP (ATP + cost of capital).
        assert_eq!(tp_bare.expected_reinstatement_credit, 0.0);
        assert!((tp_bare.technical_premium - (tp_bare.actuarial_technical_price + tp_bare.cost_of_capital)).abs() < 1e-9);

        // The reinstatement-bearing layer earns a positive expected credit, so it
        // quotes strictly below the bare layer — the terms are reflected in price.
        assert!(tp_reinst.expected_reinstatement_credit > 0.0, "a cat-exposed layer with reinstatements should expect reinstatement income");
        assert!(
            tp_reinst.technical_premium < tp_bare.technical_premium,
            "reinstatement income should credit the quote: {} !< {}",
            tp_reinst.technical_premium,
            tp_bare.technical_premium
        );
        // The breakdown reconciles: TP = ATP + cost of capital − reinstatement credit.
        assert!(
            (tp_reinst.technical_premium
                - (tp_reinst.actuarial_technical_price + tp_reinst.cost_of_capital
                    - tp_reinst.expected_reinstatement_credit))
                .abs()
                < 1e-9
        );

        // The credit grows with the reinstatement factor (richer reinstatement
        // terms move more income into the quote).
        let dearer_reinstatement = LayerExposure {
            layer,
            exposure,
            territory: Territory(0),
            reinstatement: ReinstatementTerms { count: 1, factor: 2.0 },
        };
        let mut rng = Rng::seeded(2024);
        let tp_dearer = technical_premium(&dearer_reinstatement, &book, &model, &experience, &params, &mut rng);
        assert!(tp_dearer.expected_reinstatement_credit > tp_reinst.expected_reinstatement_credit);
    }

    #[test]
    fn a_clustered_second_event_costs_the_insured_more_than_twice_a_single_event_year() {
        // Within-year hardening — the non-linear cat-frequency penalty. The insured
        // cost a reinstatement layer imposes is the reinstatement premiums it pays
        // plus any loss left uncovered once cover is exhausted. The first event of a
        // year sits within the free original limit (no reinstatement, fully
        // covered), so a single-event year imposes NO such cost. A clustered SECOND
        // event in the same year both draws a claim AND triggers a reinstatement
        // premium — so a two-event year costs materially more than twice a
        // single-event year. Spread one-per-year, those same events would each enjoy
        // a fresh free limit and never harden the cover; clustering within the year
        // is what bites.
        let layer = Layer { attachment: 0.0, limit: 100.0 };
        let terms = ReinstatementTerms { count: 1, factor: 1.0 };
        let premium = 40.0;
        let exposure = 100.0; // a damage fraction of 1.0 erodes the full limit
        let panel = Panel::subscribe(&[SyndicateId(0)], 1.0);

        // Insured cost of a profile of within-year events = reinstatement premiums
        // paid + loss left uncovered. (Claims recovered are the cover working, not a
        // cost to the insured.)
        let insured_cost = |events: &[CatastropheEvent]| -> f64 {
            let mut placed =
                ReinstatementLayer::new(layer, panel.clone(), terms, premium, 0.0, 1.0);
            let mut syndicates = vec![Syndicate::with_capital(1_000_000.0)];
            placed
                .absorb_year(events, exposure, &mut syndicates)
                .iter()
                .map(|s| s.reinstatement_premium + s.uncovered)
                .sum()
        };

        let one_event = [CatastropheEvent { time: 0.3, damage_fraction: 1.0 }];
        let two_events = [
            CatastropheEvent { time: 0.3, damage_fraction: 1.0 },
            CatastropheEvent { time: 0.6, damage_fraction: 1.0 },
        ];
        let three_events = [
            CatastropheEvent { time: 0.2, damage_fraction: 1.0 },
            CatastropheEvent { time: 0.5, damage_fraction: 1.0 },
            CatastropheEvent { time: 0.8, damage_fraction: 1.0 },
        ];

        let single_cost = insured_cost(&one_event);
        let clustered_cost = insured_cost(&two_events);
        let triple_cost = insured_cost(&three_events);

        // A single-event year imposes no reinstatement cost — the free original
        // limit absorbs it.
        assert_eq!(single_cost, 0.0, "one event sits within the free original limit");
        // The clustered second event triggers a full reinstatement premium (100% of
        // the original premium), a material cost a single-event year never pays.
        assert!((clustered_cost - premium).abs() < 1e-9);
        // The headline: a clustered two-event year costs materially more than twice
        // a single-event year (within-year hardening).
        assert!(
            clustered_cost > 2.0 * single_cost,
            "clustered two-event cost {clustered_cost} should exceed twice the single-event cost {single_cost}"
        );

        // Erosion compounds: a third clustered event finds the finite reinstatement
        // exhausted, so on top of the reinstatement premium the insured retains a
        // full uncovered limit — the cost escalates super-linearly with frequency,
        // far beyond three times a single-event year.
        assert!((triple_cost - (premium + layer.limit)).abs() < 1e-9);
        assert!(triple_cost > 3.0 * single_cost + layer.limit);
        assert!(triple_cost > 2.0 * clustered_cost, "the third event escalates the cost again");
    }

    fn neutral_avt_params() -> AvtParams {
        AvtParams { headroom_responsiveness: 0.25, feedback_responsiveness: 0.25, share_appetite: 0.5 }
    }

    #[test]
    fn avt_relaxes_slowly_toward_the_headroom_implied_target() {
        // At normal headroom the headroom-implied target is exactly 1 (price at the
        // TP floor), and with the win-rate sitting on the syndicate's share-appetite
        // the feedback channel is silent. A syndicate currently asking above the floor
        // therefore relaxes DOWN toward 1 — but slowly, not in a single jump.
        let params = neutral_avt_params();
        let next = updated_avt(1.5, NORMAL_HEADROOM, params.share_appetite, &params);

        // It moved toward 1 from 1.5 ...
        assert!(next < 1.5, "AvT relaxes down toward the floor, got {next}");
        // ... but did not snap to it: the multiplier is slow-moving.
        assert!(next > 1.0, "AvT does not jump to the target in one year, got {next}");
        // The relaxation is exactly the responsiveness fraction of the gap to target.
        assert!((next - 1.375).abs() < 1e-9, "expected 1.5 + 0.25*(1.0-1.5) = 1.375, got {next}");
    }

    #[test]
    fn the_placement_feedback_channel_homeostatically_chases_the_share_appetite() {
        // Hold AvT on the floor at normal headroom so the headroom channel is silent
        // (target 1, current 1, gap 0) and only the feedback channel moves the ask.
        let params = neutral_avt_params(); // share_appetite = 0.5
        let on_floor = 1.0;

        // Winning MORE than appetite lifts AvT to give margin back ...
        let winning = updated_avt(on_floor, NORMAL_HEADROOM, 0.9, &params);
        assert!(winning > on_floor, "over-winning lifts the ask, got {winning}");
        assert!((winning - (1.0 + 0.25 * (0.9 - 0.5))).abs() < 1e-9);

        // ... winning LESS than appetite cuts it to compete ...
        let losing = updated_avt(on_floor, NORMAL_HEADROOM, 0.1, &params);
        assert!(losing < on_floor, "under-winning cuts the ask, got {losing}");

        // ... and sitting exactly on appetite leaves the floor untouched.
        let neutral = updated_avt(on_floor, NORMAL_HEADROOM, params.share_appetite, &params);
        assert!((neutral - on_floor).abs() < 1e-9, "on-appetite is a rest point, got {neutral}");
    }

    #[test]
    fn the_headroom_target_anchors_at_the_floor_and_hardens_as_capacity_tightens() {
        // Normal capacity utilisation targets exactly the TP floor.
        assert!((headroom_target(NORMAL_HEADROOM) - 1.0).abs() < 1e-9);
        // Abundant headroom (idle capital) targets BELOW 1 — undercut to win business.
        assert!(headroom_target(0.9) < 1.0, "abundant headroom softens the ask");
        // Scarce headroom (a full book) targets ABOVE 1 — hold out for rate.
        assert!(headroom_target(0.1) > 1.0, "scarce headroom hardens the ask");
        // Monotone decreasing: the tighter the capacity, the higher the target.
        assert!(headroom_target(0.1) > headroom_target(0.5));
        assert!(headroom_target(0.5) > headroom_target(0.9));
    }

    #[test]
    fn the_two_avt_channels_combine_additively() {
        // With both inputs off their rest points, the year's increment is exactly the
        // sum of the independent channels — they are separate reads (own capital state
        // vs own price discovery), so summing double-counts nothing.
        let params = neutral_avt_params();
        let current = 1.2;
        let headroom = 0.2; // scarce → headroom target above 1
        let win_rate = 0.8; // above appetite → feedback positive

        let headroom_channel = params.headroom_responsiveness * (headroom_target(headroom) - current);
        let feedback_channel = params.feedback_responsiveness * (win_rate - params.share_appetite);
        let expected = current + headroom_channel + feedback_channel;

        let next = updated_avt(current, headroom, win_rate, &params);
        assert!((next - expected).abs() < 1e-9, "channels must sum, got {next} vs {expected}");
    }

    #[test]
    fn avt_is_floored_at_zero_so_the_ask_never_goes_negative() {
        // A syndicate already on the floor, drowning in idle capacity and losing every
        // placement, would be driven below zero by an unclamped update — a negative
        // price is nonsense, so the multiplier floors at zero.
        let aggressive = AvtParams { headroom_responsiveness: 2.0, feedback_responsiveness: 2.0, share_appetite: 0.9 };
        let next = updated_avt(0.05, 1.0, 0.0, &aggressive);
        assert!(next >= 0.0, "AvT never goes negative, got {next}");
    }

    #[test]
    fn an_empty_book_has_full_capacity_headroom() {
        // Headroom is the free cat-aggregate budget as a fraction of the whole
        // budget. An idle syndicate with no book has consumed none of it, so its
        // headroom is 1 — maximally abundant capital, the soft end of the cycle.
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let policy = ExposurePolicy { return_period: 200.0, solvency_fraction: 0.5, line_fraction: 0.5, tail_trials: 4_000 };
        let syndicate = Syndicate::with_capital(100.0); // cat budget = 0.5 × 100 = 50
        let book = NetBook { lines: vec![] };
        let mut rng = Rng::seeded(7);

        let headroom = policy.capacity_headroom(&syndicate, &book, &model, &mut rng);
        assert!((headroom - 1.0).abs() < 1e-9, "an empty book is all free budget, got {headroom}");
    }

    #[test]
    fn a_capital_drawdown_collapses_capacity_headroom() {
        // The budget scales with current capital, so holding the book fixed and
        // depleting capital eats the free headroom — the structural channel by which
        // a catastrophe hardens an exposed syndicate. Insolvency leaves none at all.
        let model = CatModel { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
        let policy = ExposurePolicy { return_period: 200.0, solvency_fraction: 0.5, line_fraction: 0.5, tail_trials: 4_000 };
        let book = NetBook { lines: vec![NetLine { territory: Territory(0), net_limit: 40.0 }] };
        let headroom_at = |capital: f64| {
            let mut rng = Rng::seeded(11);
            policy.capacity_headroom(&Syndicate::with_capital(capital), &book, &model, &mut rng)
        };

        let healthy = headroom_at(100.0);
        let depleted = headroom_at(50.0);
        let insolvent = headroom_at(0.0);

        assert!(healthy > depleted, "a drawdown must shrink headroom: {healthy} !> {depleted}");
        assert!(depleted > insolvent, "deeper depletion shrinks it further: {depleted} !> {insolvent}");
        assert_eq!(insolvent, 0.0, "an insolvent syndicate has no capacity headroom");
    }

    fn offer(id: usize, quote: f64, share: f64) -> SubscriptionOffer {
        SubscriptionOffer {
            syndicate: SyndicateId(id),
            quote,
            decision: UnderwritingDecision::Accept,
            offered_share: share,
        }
    }

    #[test]
    fn a_follower_subscribes_to_the_firm_order_only_at_or_below_it() {
        // The lead's quote is the layer's firm order — the single price the insured
        // pays. A follower subscribes when its anchored quote sits at or below that
        // firm order (it will write at the lead's terms); a follower whose anchored
        // quote exceeds the firm order will not write that cheap, and drops off.
        let lead = offer(0, 100.0, 0.3); // firm order = 100
        let followers = [
            offer(1, 90.0, 0.3),  // below firm order → subscribes
            offer(2, 100.0, 0.2), // exactly at firm order → subscribes
            offer(3, 110.0, 0.3), // above firm order → does not subscribe
        ];

        let panel = form_panel(lead, &followers);
        let members: Vec<usize> = panel.entries.iter().map(|e| e.syndicate.0).collect();

        assert_eq!(members, vec![0, 1, 2], "only the lead and the at/below-firm followers subscribe");
    }

    #[test]
    fn a_follower_declining_on_exposure_never_subscribes_however_cheap_the_firm_order() {
        // Herding moves price, never capacity discipline: a follower whose own
        // exposure limits decline the risk drops off the panel even though its
        // anchored quote sits comfortably below the firm order.
        let lead = offer(0, 100.0, 0.4);
        let declined = SubscriptionOffer {
            syndicate: SyndicateId(1),
            quote: 50.0, // far below the firm order — would subscribe on price alone
            decision: UnderwritingDecision::Decline(DeclineReason::CatAggregate),
            offered_share: 0.4,
        };

        let panel = form_panel(lead, &[declined]);
        let members: Vec<usize> = panel.entries.iter().map(|e| e.syndicate.0).collect();
        assert_eq!(members, vec![0], "the capacity-declined follower stays off the panel");
    }

    #[test]
    fn capacity_first_fill_caps_the_panel_at_the_full_layer() {
        // Offers fill the layer in order; the share that completes the layer is
        // trimmed to fit exactly, and willing offers beyond a full layer are not
        // needed. The placed portion never exceeds the whole layer.
        let lead = offer(0, 100.0, 0.5);
        let followers = [
            offer(1, 100.0, 0.4), // takes 0.4 → running total 0.9
            offer(2, 100.0, 0.3), // only 0.1 left → trimmed to 0.1, total 1.0
            offer(3, 100.0, 0.3), // layer already full → unused
        ];

        let panel = form_panel(lead, &followers);
        assert!((panel.placed_portion() - 1.0).abs() < 1e-9, "a full layer places exactly 1.0");
        let ids: Vec<usize> = panel.entries.iter().map(|e| e.syndicate.0).collect();
        assert_eq!(ids, vec![0, 1, 2], "the surplus offer (3) is dropped once the layer fills");
        let shares: Vec<f64> = panel.entries.iter().map(|e| e.share).collect();
        assert!((shares[2] - 0.1).abs() < 1e-9, "the completing share is trimmed to 0.1, got {}", shares[2]);
    }

    #[test]
    fn a_layer_with_insufficient_willing_capacity_is_left_partially_placed() {
        // When the willing subscribers cannot fill the layer, it is partially placed
        // — the panel's placed portion falls short of 1.0 and the insured carries the
        // gap (restructure or retain).
        let lead = offer(0, 100.0, 0.3);
        let followers = [
            offer(1, 100.0, 0.2),
            offer(2, 130.0, 0.4), // above firm order → does not subscribe, so capacity is short
        ];

        let panel = form_panel(lead, &followers);
        assert!((panel.placed_portion() - 0.5).abs() < 1e-9, "only 0.3 + 0.2 places, got {}", panel.placed_portion());
        assert!(panel.placed_portion() < 1.0, "the layer is left partially placed");
    }

    #[test]
    fn a_profitable_well_capitalised_year_distributes_a_fraction_of_the_profit() {
        // Year-end distribution releases the genome payout fraction of the year's
        // underwriting profit to capital providers — the only thing that stops
        // capital accumulating without bound and lets the market re-soften.
        let params = DistributionParams { payout_fraction: 0.6, solvency_floor: 100.0 };
        let released = distribution(1_000.0, 200.0, &params);
        assert!((released - 120.0).abs() < 1e-9, "0.6 × 200 profit = 120, got {released}");
    }

    #[test]
    fn distributions_are_suppressed_in_loss_years() {
        // A loss year releases nothing — there is no profit to distribute, and the
        // capital must absorb the loss, not be paid out.
        let params = DistributionParams { payout_fraction: 0.6, solvency_floor: 100.0 };
        assert_eq!(distribution(1_000.0, -50.0, &params), 0.0, "a loss year distributes nothing");
        assert_eq!(distribution(1_000.0, 0.0, &params), 0.0, "a break-even year distributes nothing");
    }

    #[test]
    fn an_impaired_syndicate_below_the_solvency_floor_rebuilds_before_distributing() {
        // Even a profitable year releases nothing while capital sits below the
        // solvency floor: the impaired syndicate rebuilds first.
        let params = DistributionParams { payout_fraction: 0.6, solvency_floor: 100.0 };
        assert_eq!(distribution(80.0, 200.0, &params), 0.0, "below the floor, profit is retained to rebuild");
    }

    #[test]
    fn a_distribution_never_drives_capital_below_the_solvency_floor() {
        // A thin-but-solvent syndicate with a big nominal profit only releases down
        // to the floor — the payout is capped by the capital available above it.
        let params = DistributionParams { payout_fraction: 0.9, solvency_floor: 100.0 };
        // 0.9 × 500 = 450 desired, but only 120 − 100 = 20 sits above the floor.
        let released = distribution(120.0, 500.0, &params);
        assert!((released - 20.0).abs() < 1e-9, "capped at the headroom above the floor, got {released}");
        assert!(120.0 - released >= params.solvency_floor, "capital never falls below the floor");
    }
}
