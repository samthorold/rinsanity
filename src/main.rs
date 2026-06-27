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
    anchored_quote, attritional_aggregate_samples, catastrophe_aggregate_samples,
    coefficient_of_variation, follower_weight, AttritionalPeril, Broker, CatastrophePeril,
    RelationshipOutcome, Rng, SyndicateId,
};

fn main() {
    attritional_diagnostic();
    println!();
    catastrophe_diagnostic();
    println!();
    placement_demonstration();
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

fn placement_demonstration() {
    // The placement loop's two emergent behaviours, both fast and deterministic.
    println!("Placement loop — herding (#3) and relationship stickiness (#5)");
    println!();

    // Herding: followers with dispersed beliefs but poor own info cluster their
    // quotes behind a reputable lead, with a DERIVED weight w (not a constant).
    let own_beliefs = [80.0, 100.0, 120.0, 140.0];
    let lead_quote = 100.0;
    let w_herd = follower_weight(0.1, 0.9, 1.0); // low own-confidence, reputable lead
    let w_indep = follower_weight(0.95, 0.9, 1.0); // confident followers
    let clustered: Vec<f64> = own_beliefs.iter().map(|&p| anchored_quote(p, lead_quote, w_herd)).collect();
    let dispersed: Vec<f64> = own_beliefs.iter().map(|&p| anchored_quote(p, lead_quote, w_indep)).collect();
    println!("Follower beliefs (dispersed):    {own_beliefs:?}  CV={:.3}", coefficient_of_variation(&own_beliefs));
    println!("Low confidence, reputable lead:  w={w_herd:.2} -> quotes {:?}  CV={:.3}", round2(&clustered), coefficient_of_variation(&clustered));
    println!("High confidence (own info good): w={w_indep:.2} -> quotes {:?}  CV={:.3}", round2(&dispersed), coefficient_of_variation(&dispersed));
    println!("=> w is derived; poor info + reputable lead clusters PRICE (beliefs untouched).");
    println!();

    // Stickiness: a loyal broker keeps leading its incumbent for years even as a
    // brand-new challenger wins every renewal — relationships trail, slowly.
    let mut broker = Broker::new(vec![0.8, 0.0], 0.9);
    let (incumbent, challenger) = (SyndicateId(0), SyndicateId(1));
    println!("Year-end relationship scores (incumbent vs winning challenger), inertia=0.9:");
    println!("{:>5}  {:>10}  {:>10}  {:>8}", "year", "incumbent", "challenger", "lead");
    for year in 0..12 {
        let lead = if broker.relationship(incumbent) >= broker.relationship(challenger) { "incumbent" } else { "challenger" };
        println!("{year:>5}  {:>10.3}  {:>10.3}  {lead:>8}", broker.relationship(incumbent), broker.relationship(challenger));
        broker.update_relationship(challenger, RelationshipOutcome { quoted: true, won: true, solvent: true });
        broker.update_relationship(incumbent, RelationshipOutcome { quoted: true, won: false, solvent: true });
    }
    println!("=> the incumbent retains the lead for years; share adjusts slowly, not instantly.");
}

fn round2(xs: &[f64]) -> Vec<f64> {
    xs.iter().map(|x| (x * 100.0).round() / 100.0).collect()
}
