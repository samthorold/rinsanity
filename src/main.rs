//! Runnable risk-pooling diagnostic. Two halves of one invariant:
//!
//!   * Attritional losses are drawn independently per asset, so the CV of the
//!     insurer's aggregate falls as ~1/√N — each quadrupling of N halves the CV.
//!   * A catastrophe is a single shared occurrence per territory, so the CV is
//!     ~flat as the pool grows within a territory and only falls when exposure
//!     is spread across uncorrelated territories (~1/√T).
//!
//! If both halves hold, the attritional/catastrophe distinction is physically
//! real in the model.

use rinsanity::{
    attritional_aggregate_samples, catastrophe_aggregate_samples, coefficient_of_variation,
    AttritionalPeril, CatastrophePeril, Rng,
};

fn main() {
    attritional_diagnostic();
    println!();
    catastrophe_diagnostic();
}

fn attritional_diagnostic() {
    let peril = AttritionalPeril { occurrence_probability: 0.25, mean_damage_fraction: 0.1 };
    let sum_insured = 1_000.0;
    let trials = 4_000;
    let mut rng = Rng::seeded(2024);

    println!("Risk-pooling diagnostic — attritional aggregate CV vs pool size N");
    println!("(peril: P(occurrence)={}, mean damage fraction={}, trials/N={trials})", peril.occurrence_probability, peril.mean_damage_fraction);
    println!();
    println!("{:>8}  {:>12}  {:>14}  {:>16}", "N", "CV", "CV x sqrt(N)", "ratio to prev");

    let mut prev_cv: Option<f64> = None;
    for &n in &[50usize, 200, 800, 3_200, 12_800] {
        let samples = attritional_aggregate_samples(n, sum_insured, &peril, trials, &mut rng);
        let cv = coefficient_of_variation(&samples);
        let normalised = cv * (n as f64).sqrt();
        let ratio = prev_cv.map(|p| cv / p);
        match ratio {
            Some(r) => println!("{n:>8}  {cv:>12.5}  {normalised:>14.4}  {r:>16.4}"),
            None => println!("{n:>8}  {cv:>12.5}  {normalised:>14.4}  {:>16}", "-"),
        }
        prev_cv = Some(cv);
    }

    println!();
    println!("CV x sqrt(N) is roughly constant => CV ~ 1/sqrt(N): risk pooling holds.");
    println!("Each 4x growth in N roughly halves the CV (ratio ~ 0.5).");
}

fn catastrophe_diagnostic() {
    let peril = CatastrophePeril { annual_frequency: 0.6, min_damage_fraction: 0.02, tail_alpha: 1.4 };
    let sum_insured = 1_000.0;
    let trials = 12_000;
    let mut rng = Rng::seeded(2024);

    println!("Risk-pooling diagnostic — catastrophe aggregate CV (the shared occurrence)");
    println!(
        "(cat process: frequency={}/yr, Pareto alpha={}, min damage fraction={}, trials={trials})",
        peril.annual_frequency, peril.tail_alpha, peril.min_damage_fraction
    );
    println!();

    println!("Within ONE territory — CV should stay ~flat as the pool grows:");
    println!("{:>8}  {:>12}  {:>16}", "N", "cat CV", "ratio to prev");
    let mut prev: Option<f64> = None;
    for &n in &[50usize, 100, 200, 800] {
        let cv = coefficient_of_variation(&catastrophe_aggregate_samples(1, n, sum_insured, &peril, trials, &mut rng));
        match prev {
            Some(p) => println!("{n:>8}  {cv:>12.5}  {:>16.4}", cv / p),
            None => println!("{n:>8}  {cv:>12.5}  {:>16}", "-"),
        }
        prev = Some(cv);
    }

    println!();
    println!("Same total exposure spread across T uncorrelated territories — CV should fall ~1/sqrt(T):");
    println!("{:>8}  {:>12}  {:>16}", "T", "cat CV", "ratio to prev");
    let total = 1_024usize;
    let mut prev: Option<f64> = None;
    for &t in &[1usize, 4, 16, 64] {
        let cv = coefficient_of_variation(&catastrophe_aggregate_samples(t, total / t, sum_insured, &peril, trials, &mut rng));
        match prev {
            Some(p) => println!("{t:>8}  {cv:>12.5}  {:>16.4}", cv / p),
            None => println!("{t:>8}  {cv:>12.5}  {:>16}", "-"),
        }
        prev = Some(cv);
    }

    println!();
    println!("Flat in N (shared draw) but falling in T (diversification across zones):");
    println!("the catastrophe component does NOT pool with size, only with spread.");
}
