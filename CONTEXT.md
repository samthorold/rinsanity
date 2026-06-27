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
A syndicate's vector of selectable per-syndicate parameters (AvT responsiveness, herding susceptibility, hurdle rate, credibility `k`, payout rule, reserving bias, cat-model parameters) on which market selection acts.

## Flagged ambiguities

- **"Phenomenon"** was historically used for both substrate checks (e.g. risk pooling, §5 #0) and genuine emergent market behaviours. Resolved: substrate checks are **diagnostic invariants**; only emergent market behaviours are **phenomena**.
