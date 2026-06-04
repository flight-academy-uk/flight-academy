# ADR-016 — Compliance baseline and certification commitments

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-02 |
| **Deciders** | @ICreateThunder |
| **Tags** | compliance, certification, gdpr, iso27001, soc2, aviation, easa, caa, self-host |
| **Supersedes** | (none — consolidates compliance-relevant statements scattered across ADRs 001–012) |

## Context

Decisions across ADRs 001–012 implicitly target a high compliance bar
(envelope encryption + crypto-shred, hash-chained audit, three-chain
partitioning, ABAC over RBAC, hardware passkey for staff, separate
platform binary, tenant transparency reporting). The bar is recorded
nowhere as such. Three concrete payoffs to writing it down:

1. **Decision discipline** — future trade-offs answer against a recorded
   baseline instead of re-deriving from first principles.
2. **Buyer-facing readiness** — B2B sales conversations open with "are
   you ISO 27001 certified / can you do SOC 2 / are you Cyber Essentials
   Plus?" The answer must be on a page.
3. **Self-host accountability split** — self-hosters inherit some
   compliance posture (we ship the architecture) and own others (they run
   their own audit log review, their own access control discipline). The
   split should be written.

The architecture is **deliberately above the certification baseline** in
several places. Hash-chained audit exceeds ISO 27001 / SOC 2 integrity
requirements. Hardware passkey for staff is NIST 800-63B AAL3 territory,
stronger than typical B2B SaaS internal access. Tenant transparency
reporting ([ADR-010 §J](ADR-010-platform-operator-access.md)) is not
required by any framework. This ADR records both — the **floor** (what we
must do by law or by chosen certification target) and the **deliberate
over-target** (architectural choices made above-baseline and why).

## Decision

**Adopt a four-bucket compliance baseline per topic — *applicable by
law*, *design-aligned framework* (architected to satisfy without a
dated certification commitment), *operating standard followed without
certification*, *out of scope*. Document the self-host accountability
split. Maintain forward considerations for directions that change the
baseline (notably airborne planning features).**

### A. Aviation regulatory — applicable by law

| Framework | What it governs | Our posture |
| --- | --- | --- |
| **EASA Part-FCL** (Section A + B) | Pilot licensing, ratings, logbook, currency | Logbook + currency outputs comply with Part-FCL Section A; school records support Section B |
| **EASA Part-MED** | Pilot medical | We record references only; medical detail is `special-category` ([ADR-008 §B](ADR-008-data-sharing-posture.md)) — never leaves the platform |
| **EASA Part-OPS** / **Part-NCO** | Operations rules | Logbook / training records support compliance; not an operations system itself |
| **EASA Part-ORA / Part-ATO / Part-DTO** | Approved / declared training organisations | Records support ATO/DTO compliance; not a substitute for the approval |
| **UK CAA** equivalents | UK post-Brexit divergence, currently aligned | Track CAA divergence in `docs/operations/regulatory-watch.md` (TBD) |
| **ICAO Annex 1** | Personnel licensing | Inherited via EASA Part-FCL |
| **ICAO Annex 6** | Operation of aircraft | Inherited via Part-OPS / Part-NCO |
| **ICAO Annex 13** | Accident / incident investigation | ECCAIRS2 occurrence reporting; supports investigators, not investigation |
| **ICAO Annex 19** | Safety management systems | School SMS evidence; supports, not substitutes |
| **ECCAIRS2** | EU/EASA occurrence reporting taxonomy | `flight-academy-aviation` outputs ECCAIRS2-shaped reports ([ADR-005 §C](ADR-005-workspace-layout.md)) |

**Boundary statement.** Flight Academy produces records and decisions used
by regulated parties (pilots, schools, ATOs, DTOs). It is not itself a
regulated party in the airworthiness sense, and our outputs are aids to
the user's compliance — not substitutes for it. The user remains the
authority for go/no-go decisions; we provide verifiable evidence.

### B. Data protection — applicable by law

| Framework | What it governs | Our posture |
| --- | --- | --- |
| **UK GDPR + DPA 2018** | UK data protection | Primary regime for UK-established service |
| **EU GDPR** | EU data protection | Applies to EU users / EU establishment via Art. 3 territorial scope |
| **ICO** (UK) | Regulator | Notifications, DPIA filing where required, data-subject complaints |
| **EU DPAs** | National regulators | Engaged via lead-supervisor mechanism where applicable |

Architectural choices supporting this baseline:

- Envelope encryption + crypto-shred per controller
  ([ADR-001 §D](ADR-001-platform.md), [ADR-012](ADR-012-cross-tenant-dek-erasure.md)) —
  right-to-erasure done cryptographically.
- Data classification ([ADR-008 §B](ADR-008-data-sharing-posture.md)) —
  special-category never leaves the platform.
- Audit hash-chain ([ADR-004 §D](ADR-004-defence-in-depth.md)) —
  demonstrable integrity for ICO inquiries.
- Tenant transparency reporting
  ([ADR-010 §J](ADR-010-platform-operator-access.md)) — beyond Art.
  15 right-of-access; proactive.
- DPA template (`docs/contracts/dpa-template.md`, TBD) — controller /
  processor relationship with tenants and self-hosters.

### C. Information security frameworks — design-aligned

Architectures are built so these certifications are achievable when
pursued. Pursuit is a procurement decision, not an architectural one;
no dates committed here.

| Framework | Architectural readiness | Rationale |
| --- | --- | --- |
| **Cyber Essentials Plus** (NCSC, UK) | Inherited from infrastructure hardening + dependency management already in place | UK public-sector signal; low-scope baseline |
| **ISO/IEC 27001:2022** | ISMS controls satisfied by architecture (access control, encryption, audit, incident response, change management); evidence-gathering automatable | Near-table-stakes for UK/EU B2B |
| **ISO/IEC 27018:2019** | Cloud-PII controls covered by [ADR-008](ADR-008-data-sharing-posture.md), [ADR-012](ADR-012-cross-tenant-dek-erasure.md) | Cloud-PII extension on 27001 |
| **SOC 2 Type II** (AICPA) | Trust principles (Security, Availability, Processing Integrity, Confidentiality, Privacy) overlap 27001 substantially; readiness mostly inherited | Relevant if US / multinational customers are pursued |

The intent is **designed against, not certified against** — the
architecture exceeds these baselines, and pursuing certification is a
matter of producing evidence and engaging an auditor, not redesigning
controls. Dated pursuit lives in `docs/operations/compliance-roadmap.md`
(TBD) when budget is allocated.

### D. Accessibility — design-aligned

| Framework | Architectural readiness | Rationale |
| --- | --- | --- |
| **WCAG 2.2 AA** | Two-layer: build-time automated checks + manual review per release (see below) | UK Equality Act 2010 reasonable-adjustment; EU Accessibility Act 2025; UK public-sector procurement |

Two-layer implementation. Accessibility is a legal requirement; the
posture is deliberately above the baseline.

**Build-time automated checks:**

- **`axe-core` via `@axe-core/playwright`** for end-to-end
  accessibility assertions on built routes — the industry-standard
  engine; catches ~30-50% of WCAG violations (the deterministic
  subset).
- **`vitest-axe`** for component-level testing; primitives in
  `apps/web-ui` assert accessibility in isolation.
- **`pa11y-ci`** as a complementary check (wraps axe + HTML
  CodeSniffer) on the built bundle in CI.
- **ESLint with `eslint-plugin-svelte` accessibility rules** plus
  patterns adapted from `eslint-plugin-jsx-a11y` — catches static
  errors at write time.
- Underlying primitives: Svelte + `bits-ui` for accessible interactive
  components; semantic HTML; the white-label oklch token system
  enforces contrast at the colour layer
  ([ADR-014 §F](ADR-014-frontend-architecture.md)).

**Manual review per release:**

- Keyboard-only navigation walkthrough of critical paths.
- Screen-reader verification (NVDA on Windows, VoiceOver on
  macOS/iOS, TalkBack on Android) for critical paths.
- Colour-contrast audit on real-world tenant configurations beyond
  what tokens enforce in isolation.

**Honest caveat.** Automated tools catch only the deterministic
subset of WCAG violations; semantic correctness, screen-reader UX
quality, keyboard logic, and content clarity require human review.
WCAG 2.2 AA conformance claims rest on **both layers**, never on
automated-green alone.

### E. Operating standards followed without certification

Recorded as design references, not audited targets:

| Standard | Where it shows up |
| --- | --- |
| **NCSC Cloud Security Principles** (14 principles) | [ADR-004](ADR-004-defence-in-depth.md) defence-in-depth posture aligns |
| **OWASP ASVS** L2 baseline (L3 in critical paths) | Input validation, authn, authz, cryptography requirements |
| **NIST SP 800-63B** | Auth strength: AAL3 for staff ([ADR-010 §D](ADR-010-platform-operator-access.md)), AAL2 for users |
| **W3C WebAuthn L3** | Passkey implementation ([ADR-001 §F](ADR-001-platform.md)) |
| **CISA Secure by Design** | Default-secure config, no separate hardening cost |
| **OAuth 2.1 + PKCE** | User consent grants ([ADR-011](ADR-011-user-consent-grant.md)) |
| **RFC 9457** (problem+json), **RFC 8594** (Sunset), **AIP-151** (LRO) | API contract ([ADR-006](ADR-006-api-contract.md)) |

### F. PCI scope

**SAQ-A** via PSP boundary. We never touch card data directly —
PSP-hosted elements / redirect flow keep card data out of our
network, scope, and audit boundary. Eligible PSPs include Stripe,
Adyen, Mollie, Square, and any provider that supports
fully-redirected or fully-hosted card capture. The specific PSP
choice is a procurement decision; the architectural commitment is
that whichever PSP ships, the integration is **SAQ-A-scoped** —
that is, we use hosted-payment-page or redirect-checkout patterns,
never direct card capture, never PAN storage, never SAD storage.
SAQ-A is annual self-assessment, not external audit.

If we ever build a payments feature that accepts card data directly,
scope changes to SAQ-D / full PCI-DSS — a new ADR amendment.

### G. Explicitly out of scope

Clarifies what we **do not claim** so contributors and buyers don't
infer commitments we haven't made:

| Framework | Reason out of scope |
| --- | --- |
| **DO-178C / ED-12C** "Software Considerations in Airborne Systems" | Governs software *in aircraft* (avionics, FMS, autopilot, type-certificated EFB). Flight Academy is a ground system producing records and decisions. We are not airborne software; we do not seek airworthiness certification of our outputs. |
| **DO-326A / ED-202A** "Airworthiness Security Process" | Cybersecurity counterpart to DO-178C — applies to systems with data links to aircraft that could affect airworthiness. Not us. |
| **DO-355 / ED-204** "Information Security for Continuing Airworthiness" | Operational/maintenance cybersecurity for ground systems integrating with type-certified maintenance. **Potential future boundary** — see §I if logbook data ever feeds into type-certified CAMO workflows. |
| **FedRAMP** (US federal cloud) | Out of scope unless US federal customers are explicitly pursued. |
| **HIPAA** (US healthcare) | Out of scope — pilot medical is `special-category` and handled under GDPR; no US healthcare-provider relationship. |
| **NIS Regulations 2018 / NIS2 (EU)** | Out of scope at MVP scale (not a relevant digital service provider as defined). Revisit if usage thresholds cross applicability triggers. |

**On safety-critical outputs.** W&B (mass and balance), currency,
performance calculations are not airborne software but are
safety-critical information for the user's go/no-go decisions. Our
duty of care: documented calculation method, conservative defaults,
clear units, source-data provenance, "verify against POH" disclaimers.
The pilot remains the authority; we are an aid.

### H. Self-host accountability split

What the architecture provides; what the self-hoster owns.

| **Provided by Flight Academy architecture** | **Owned by self-hoster** |
| --- | --- |
| Envelope encryption + crypto-shred ([ADR-001 §D](ADR-001-platform.md), [ADR-012](ADR-012-cross-tenant-dek-erasure.md)) | Cluster cert procurement; key custody for the cluster's KMS / `age` keys |
| Hash-chained audit ([ADR-004 §D](ADR-004-defence-in-depth.md), [ADR-009 §C](ADR-009-event-streams-and-retention.md)) | Audit-log retention discipline; periodic integrity verification |
| Data-class enforcement ([ADR-008 §B](ADR-008-data-sharing-posture.md)) | Tenant-side classification of any custom fields they add |
| ABAC primitives ([ADR-001 §C](ADR-001-platform.md)) | Role assignments; access reviews |
| Expand-contract migrations ([ADR-002](ADR-002-release-deployment.md), [ADR-003](ADR-003-db-migrations.md)) | Migration scheduling; backup-before-migrate discipline |
| Self-host conformance matrix (`docs/operations/self-host-conformance.md`, TBD) | Adhering to it; declaring deviations |
| Container images, Helm chart, docker-compose | Their cluster, their network, their DNS, their backup target |
| Tenant transparency reporting code | **N/A** — no platform staff distinct from the self-hoster ([ADR-010 §H](ADR-010-platform-operator-access.md)) |
| **Not provided** — staff plane (no `apps/admin`) | Equivalent privileged-access discipline within their own organisation |
| WCAG 2.2 AA component-library and design-token discipline ([§D](#d-accessibility--design-aligned), [ADR-014 §F](ADR-014-frontend-architecture.md)) | Deployment-level conformance — tenant-supplied branding contrast against chosen brand colours, content clarity, custom-added text and assets, accessibility of any tenant-specific customisations |

A self-hoster's compliance posture *uses* the architecture but is not
*conferred by* it. Self-hoster claims (ISO 27001, SOC 2, Cyber
Essentials, WCAG 2.2 AA) cover their own deployment and operations;
Flight Academy does not certify them.

The DPA template (`docs/contracts/dpa-template.md`) covers the
hosted offering — self-hosters draft their own data-protection
arrangements with their data subjects, since we are not the data
processor in their deployment.

### I. Forward considerations

Directions that, if pursued, change the applicable baseline:

| Direction | Baseline change |
| --- | --- |
| **EFB Type B features** (flight planning, performance, in-flight weather/NOTAM display) | Falls under **EASA AMC 20-25** / **Air OPS Decision 2014/004/R** / **FAA AC 120-76D** — *operator approval, not type certification*. Class 1 hardware (portable / not installed), Type B software. AIRAC cycle compliance becomes a hard requirement; data-source licensing (charts / weather / NOTAM) becomes procurement work. Same family as ForeFlight, SkyDemon, Garmin Pilot. |
| **Connected aircraft maintenance data** (logbook → type-certified CAMO sync) | **DO-355 / ED-204** boundary becomes relevant at the interface. Either: (a) we stop at the boundary and the CAMO system handles DO-355 compliance; (b) we extend to participate, with substantial uplift in process / documentation / evidence. Default: (a). |
| **US market entry** | SOC 2 Type II moves from conditional to firm target. FAA-side AC 120-76D supplements EASA AMC 20-25 for EFB features. |
| **Public sector / NHS-adjacent customers** | NHS DSP Toolkit, public-sector procurement frameworks (G-Cloud), additional Cyber Essentials Plus reaudit cadence. |
| **Cross-border data flows beyond UK / EU** | Additional adequacy assessment per UK / EU GDPR; Standard Contractual Clauses; potentially new DPAs; adequacy-decision tracking per the EDPB framework. |
| **New jurisdictions** | The §A applicable-by-law list expands; this ADR amends to record. |

Forward considerations are *not* commitments. They name the regulatory
shape so a future decision lands informed, not surprised.

## Consequences

**Positive.** Explicit baseline; future decisions calibrated to recorded
targets; buyer-facing answer is one document; self-host accountability is
unambiguous; the deliberate over-target is recorded as a values choice,
not an accident. Existing ADRs gain an index point that consolidates
their compliance basis.

**Negative.** Design-aligned readiness still costs — evidence
gathering, control documentation, and audit-trail completeness are
ongoing work even before formal certification. The explicit
out-of-scope list constrains: any future feature that contradicts it
(e.g., touching card data directly) needs a new ADR amendment, not
just an implementation PR.

**Neutral.** Nothing here changes existing ADR decisions — this ADR
consolidates and labels. Cross-references back from existing ADRs (their
References sections gaining `[ADR-016 §X]`) are mechanical but useful.

## Alternatives considered

- **Leave compliance posture implicit, distributed across ADRs.**
  Cheapest short-term; loses the index payoff, loses the buyer-facing
  document, makes self-host accountability ambiguous. The implicit
  posture is also exactly how design decisions drift — without a
  recorded baseline, "we exceed X" becomes harder to defend over time.
- **Pursue all major frameworks simultaneously** (Cyber Essentials
  Plus, ISO 27001, ISO 27018, SOC 2, WCAG, sector-specific).
  Maximises buyer signal; multiplies audit cost; risks doing none
  well. Rejected as an active stance — the architecture is designed
  for any of these, pursuit is sequenced when budget allows.
- **Self-certify only, no external audits.** Cheapest of all; signals
  nothing to buyers; harder to defend to regulators after an incident.
  Rejected.
- **Commit specific dates for each certification.** Stronger external
  signal. Rejected — dates churn faster than architectural decisions
  do; this ADR records what we design for, the
  `docs/operations/compliance-roadmap.md` records when we pursue it.

## References

- [ADR-001 §C/§D/§F](ADR-001-platform.md) — ABAC; envelope encryption;
  passwordless sessions. Baseline §B, §C, §E.
- [ADR-002](ADR-002-release-deployment.md) — release / deployment;
  self-host. Baseline §H.
- [ADR-004 §D](ADR-004-defence-in-depth.md) — audit hash-chain mechanism (referenced as the baseline §B and §C support).
  Baseline §B, §C.
- [ADR-004](ADR-004-defence-in-depth.md) — defence in depth. Baseline §E.
- [ADR-005 §C](ADR-005-workspace-layout.md) — `flight-academy-aviation`
  ECCAIRS2 output. Baseline §A.
- [ADR-006](ADR-006-api-contract.md) — API contract IETF references.
  Baseline §E.
- [ADR-008 §B/§E/§F](ADR-008-data-sharing-posture.md) — data
  classification; export paths; DPA. Baseline §B, §H.
- [ADR-010 §D/§H/§J](ADR-010-platform-operator-access.md) — staff auth
  strength; self-host disable; tenant transparency. Baseline §B, §C, §H.
- [ADR-011](ADR-011-user-consent-grant.md) — OAuth 2.1 + PKCE.
  Baseline §E.
- [ADR-012](ADR-012-cross-tenant-dek-erasure.md) — DEK per controller;
  crypto-shred. Baseline §B.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 8 (honour
  all people — WCAG conformance is treating users with respect), 24
  (no pretence — design-aligned not certified-claimed; honest about
  WCAG automated-only coverage limits), 28 (truth — explicit
  out-of-scope list is honest about what we don't claim), 35–36
  (restraint — bucket-cataloguing not framework-multiplying; PCI
  scope is SAQ-A by construction), 48 (watchfulness — regulatory-
  watch tracks divergence; forward considerations name
  baseline-changing directions), 60 (obey those in authority — UK
  CAA, EASA, ICO, UK GDPR honoured as the applicable regulators) —
  the deliberate over-target is values-driven, recorded here.

## Notes

The frameworks in §C and §D are design-aligned — the architecture is
built so these are achievable when pursued. Pursuit is a procurement
decision. Specific dates live in `docs/operations/compliance-roadmap.md`
(TBD) when budget is allocated, not in this ADR — keeping date churn
out of an architectural decision record.

The out-of-scope list in §G is *load-bearing for buyer conversations*.
A prospect asking "are you DO-178C certified?" or "are you HIPAA
compliant?" gets a direct answer with the reasoning. Not knowing what
we don't claim is a more common cause of lost trust than not knowing
what we do.

The forward considerations in §I are the most likely place this ADR
amends over time. Adding a new direction is a small edit (one row);
removing one is a deliberate signal that the direction is no longer
considered.
