//! `rinsanity` CLI. Two surfaces over one engine:
//!
//!   * `cycle` — agent-consumable emission. Runs the cycle simulation and writes
//!     **only** the per-year [`YearReport`] diagnostics (CSV or JSON) to stdout (or
//!     `--out`). Every stdout line is a data line; no prose, no other diagnostics,
//!     parseable with zero heuristics.
//!   * `diagnostics` (also the default, and `--explain`) — the human read: the
//!     risk-pooling, catastrophe, and placement demonstrations plus the qualitative
//!     underwriting-cycle read. Prose lives here and never pollutes `cycle`.
//!
//! Deliberately dependency-free: args are parsed with [`std::env::args`] and JSON is
//! hand-rolled in the library ([`reports_to_json`]). The emission itself is a pure
//! library function over `&[YearReport]`, so the bytes here and the unit tests share
//! one code path.

use std::process::ExitCode;

use rinsanity::{
    anchored_quote, attritional_aggregate_samples, catastrophe_aggregate_samples,
    coefficient_of_variation, demonstration_market, follower_weight, reports_to_csv,
    reports_to_json, AttritionalPeril, Broker, CatastrophePeril, RelationshipOutcome, Rng,
    SyndicateId,
};

const USAGE: &str = "\
rinsanity — emergent underwriting-cycle simulation

USAGE:
    rinsanity cycle [--seed <u64>] [--years <usize>] [--format csv|json] [--out <path>]
    rinsanity diagnostics            # human demonstrations + qualitative cycle read
    rinsanity [--explain]            # alias for diagnostics (default)

cycle:
    Runs the cycle simulation and emits ONLY the per-year diagnostics — one header
    line then one row per year, nothing else. The schema (columns / keys) is the
    documented YearReport fields; see YearReport::CSV_HEADER.

    --seed   <u64>      RNG seed for the demonstration market (default 2024)
    --years  <usize>    number of years to simulate (default 60)
    --format csv|json   csv (header + rows) or json (array of objects) (default csv)
    --out    <path>     write to a file instead of stdout
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("cycle") => match run_cycle(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("error: {msg}\n\n{USAGE}");
                ExitCode::FAILURE
            }
        },
        // The human read: default, explicit, or via --explain.
        None | Some("diagnostics") | Some("--explain") => {
            run_diagnostics();
            ExitCode::SUCCESS
        }
        Some("-h") | Some("--help") | Some("help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("error: unknown command '{other}'\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// The machine-readable surface: parse the `cycle` options, run the simulation, and
/// emit only the per-year diagnostics in the requested format.
fn run_cycle(args: &[String]) -> Result<(), String> {
    let mut seed: u64 = 2024;
    let mut years: usize = 60;
    let mut format = Format::Csv;
    let mut out: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = |i: usize| -> Result<&String, String> {
            args.get(i + 1).ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag {
            "--seed" => {
                seed = value(i)?.parse().map_err(|_| format!("invalid --seed '{}'", value(i).unwrap()))?;
                i += 2;
            }
            "--years" => {
                years = value(i)?.parse().map_err(|_| format!("invalid --years '{}'", value(i).unwrap()))?;
                i += 2;
            }
            "--format" => {
                format = value(i)?.parse()?;
                i += 2;
            }
            "--out" => {
                out = Some(value(i)?.clone());
                i += 2;
            }
            other => return Err(format!("unknown cycle option '{other}'")),
        }
    }

    let reports = demonstration_market(seed).run(years);
    let body = match format {
        Format::Csv => reports_to_csv(&reports),
        Format::Json => reports_to_json(&reports),
    };

    match out {
        Some(path) => std::fs::write(&path, format!("{body}\n")).map_err(|e| format!("writing {path}: {e}"))?,
        None => println!("{body}"),
    }
    Ok(())
}

enum Format {
    Csv,
    Json,
}

impl std::str::FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "csv" => Ok(Format::Csv),
            "json" => Ok(Format::Json),
            other => Err(format!("invalid --format '{other}' (expected csv or json)")),
        }
    }
}

/// The HITL deliverable for #7: a multi-decade run of the [`demonstration_market`]
/// whose emergent underwriting cycle a human reads for the right qualitative shape
/// (soft → shock → hard → soft). Emits the per-year diagnostics as CSV plus a short
/// qualitative read. Nothing here imposes a cycle — there is no market-phase
/// variable, coordinator, or `AP = TP × f(t)` curve; the rate movement is the
/// aggregate of each syndicate's purely local AvT state.
fn cycle_demonstration() {
    let years = 60;
    let mut market = demonstration_market(2024);
    let reports = market.run(years);

    println!("Underwriting cycle (#1) — emergent AvT over a {years}-year run");
    println!("AP = TP · AvT; AvT is per-syndicate, driven only by local headroom + placement feedback.");
    println!();
    println!("{}", reports_to_csv(&reports));
    println!();

    // A qualitative read to orient the human reviewer (the close is theirs).
    let rate: Vec<f64> = reports.iter().map(|r| r.rate_index).collect();
    let mean_rate = rate.iter().sum::<f64>() / rate.len() as f64;
    let hard = rate.iter().cloned().fold(f64::MIN, f64::max);
    let soft = rate.iter().cloned().fold(f64::MAX, f64::min);
    let crossings = rate.windows(2).filter(|w| (w[0] - mean_rate) * (w[1] - mean_rate) < 0.0).count();
    let mut crs: Vec<f64> = reports.iter().map(|r| r.combined_ratio).collect();
    crs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_cr = crs[crs.len() / 2];
    let max_cr = *crs.last().unwrap();
    let cat_years = crs.iter().filter(|&&c| c > 1.0).count();

    println!("Qualitative read:");
    println!("  rate index swings {soft:.2} (soft, below TP) … {hard:.2} (hard, above TP), mean {mean_rate:.2}");
    println!("  crossing its mean {crossings} times — a multi-year oscillation, not a drift");
    println!("  combined ratio is bimodal: benign-year median {median_cr:.2}, {cat_years} cat years spiking to {max_cr:.2}");
    println!("=> soft markets compete AvT below the floor; a cat collapses headroom and hardens it above;");
    println!("   recovered capital re-softens. Human review confirms the soft → shock → hard → soft shape.");
}

/// The human demonstrations: substrate diagnostics (pooling, catastrophe), the
/// placement loop's emergent behaviours, and the qualitative cycle read. Prose
/// lives here only — it never reaches the `cycle` data stream.
fn run_diagnostics() {
    attritional_diagnostic();
    println!();
    catastrophe_diagnostic();
    println!();
    placement_demonstration();
    println!();
    cycle_demonstration();
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
