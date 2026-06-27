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
        // 1 − uniform() lands in (0, 1], avoiding a divide-by-zero blow-up.
        let u = 1.0 - rng.uniform();
        let pareto = self.min_damage_fraction * u.powf(-1.0 / self.tail_alpha);
        pareto.clamp(0.0, 1.0)
    }

    /// The number of catastrophe events arriving in one year, a Poisson count
    /// with mean [`annual_frequency`](Self::annual_frequency), drawn by Knuth's
    /// algorithm from the in-crate uniform stream.
    fn annual_event_count(&self, rng: &mut Rng) -> usize {
        let threshold = (-self.annual_frequency).exp();
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
