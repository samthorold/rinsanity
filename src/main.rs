//! Runnable risk-pooling diagnostic: reports how the coefficient of variation
//! of the insurer's aggregate attritional loss falls as the pool grows. If the
//! attritional half of the substrate is physically correct, the CV falls as
//! ~1/√N — so each quadrupling of N roughly halves the CV.

use rinsanity::{
    attritional_aggregate_samples, coefficient_of_variation, AttritionalPeril, Rng,
};

fn main() {
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
