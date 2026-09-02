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

**Float**:
The premium a syndicate holds between writing business and paying claims on it, which earns the market yield while held.

## Flagged ambiguities

- **"Phenomenon"** was historically used for both substrate checks (e.g. risk pooling, §5 #0) and genuine emergent market behaviours. Resolved: substrate checks are **diagnostic invariants**; only emergent market behaviours are **phenomena**.
