//! rinsanity substrate — the attritional loss path.
//!
//! See `docs/system-design/README.md` (*Loss architecture*, *Diagnostic
//! invariants*) and `CONTEXT.md` for the vocabulary used here.

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
}
