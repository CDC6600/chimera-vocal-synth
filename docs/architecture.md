# Architecture: the acoustic model

How CHIMERA makes sound. This was the original design (moved here from `DESIGN.md`); the
engine's real-time contract and the browser boundary are in [rust-engine.md](rust-engine.md),
and the look in [ui-redesign.md](ui-redesign.md).

The owner's brief: "Create the ultimate vocal synthesizer. Web app for now. Be extremely
ambitious. Sophisticated throat, lips, tongue modeling. Different animals: human, bird, reptile,
etc. Tonal and percussive (beatboxing). Beautiful visuals/diagrams, dynamic animations based on
params."

**Not built (yet):** the design's wider bestiary (only songbird, seal, alligator and elephant
exist), the two-mass vocal folds, an IPA chart mode, the app-side EQ/reverb/limiter chain and
in-app WAV export (the CLI renders WAVs; the app has a close-mic chain in WebAudio). Treat the
code as the truth where this file disagrees.

## Overview

```
main thread (React UI, canvas renderers)
   │ commands (note/score/param/drag), timestamped in audio time
   ▼
AudioWorklet  ──  Engine (Rust crate `chimera`, compiled to wasm; the same code runs natively
                  for tests, the `chimera` CLI and, later, JDAW — see docs/rust-engine.md)
                   ├─ Voice[] (pool)
                   │   ├─ Controller: gesture scheduler → articulator dynamics (control rate)
                   │   ├─ Anatomy: articulator state → geometry → area functions (control rate)
                   │   ├─ Sources: glottis (parametric area / two-mass), syrinx, valves (trills)
                   │   └─ Airway: branched FDTD tube network (internal rate = 2–4× output)
                   └─ FX: mic/proximity, EQ, compressor, saturation, reverb, limiter
   │ telemetry ~60 Hz (articulators, areas, pressures, flows, glottal trace, formants)
   ▼
main thread renderers
```

Control rate: every 64–128 output samples, with per-sample linear interpolation of areas.

## 1. Airway acoustics — branched staggered-grid FDTD transmission line

Chosen over Kelly-Lochbaum because it handles, in one framework: variable section
lengths (larynx height, lip protrusion, creature size), DC aerodynamics (pressure
build-up behind closures, bursts), nonlinear kinetic losses at constrictions, yielding
walls, moving-wall volume sources (needed for clicks, ejectives, implosives), arbitrary
branching (nasal tract, sinuses, piriform fossae, two bronchi, vocal sacs, OEC).

* Units CGS. ρ = 1.14e-3 g/cm³, c = 35 000 cm/s, μ = 1.86e-4 P.
* Generic graph: pressure nodes (volume V = AΔ) and flow branches between nodes.
  Branch inertance L = ρ(Δa/2Aa + Δb/2Ab) (series half-sections, so a tiny node is
  bracketed by large inertances → stable at CFL ≤ 1 regardless of area ratio).
* Leapfrog: branches update U(n+½) from p(n); nodes update p(n+1) from U(n+½).
  Branch update is implicit in U for losses: viscous R, plus Borda–Carnot expansion
  loss a·|U|U with a = ρk/2·(1/As − 1/Al)² chosen by flow direction; solved as an
  exact quadratic per sample (avoids the period-2 instability of lagged |U|).
* Continuity: (V/ρc²) dp/dt = ΣU_in − ΣU_out − U_wall − dV/dt (moving walls).
* Walls: per-node mass–spring–damper (m≈1.5 g/cm², r≈1600, k≈3e5 CGS, ×perimeter·Δ).
  Wall flow also radiates weakly (voice bar during voiced closures).
* Radiation: Flanagan parallel R–L load at lips/nostrils/beak; output = d/dt of radiated
  volume velocity (sum of mouth, nostrils, skin).
* Turbulence: per branch Reynolds number Re = |U|·d_h/(A·ν); dipole pressure source
  g·(Re² − Re_c²)·noise injected ~1 cm downstream of the constriction exit (flow-direction
  aware), plus an obstacle source at the lower incisors when a jet reaches them (sibilants).
  Aspiration is the same mechanism at the glottis.
* Closure: branch area < ε ⇒ U = 0 (no leakage); node volumes floored for compliance.
* Internal rate 96 kHz (adult human), 192 kHz (small birds/cats), 48 kHz allowed for
  huge tracts; N chosen per anatomy so Δ ≥ c·T/0.95. Polyphase half-band decimation to
  the output rate.
* Cross-check: a frequency-domain chain-matrix model with the same loss terms computes
  H(f) for analysis, UI formant display, and as a test oracle for the FDTD.

## 2. Sources

* **Parametric glottis (default, exact pitch)**: glottal *area* pulse (opening/closing
  phases, open quotient, skew, max area, rest/abduction area, posterior chink) drives the
  interactive glottal branch, so flow shape, skewing, F1 ripple and devoicing under
  oral closure all emerge from the tract. Perturbations: jitter, shimmer, vibrato with
  drift, 1/f flutter, diplophonia/period doubling, vocal fry (low f0, short OQ, double
  pulses), falsetto (sinusoidal, incomplete closure).
* **Two-mass vocal folds (advanced)**: Ishizaka–Flanagan with collision; self-oscillating;
  pitch tracked by a feedback controller on the tension factor Q.
* **Ventricular folds**: second valve above the glottis, entrains at f0/2 for growl /
  kargyraa; static narrowing for harshness. **Epilarynx** narrowing for twang/ring.
* **Syrinx (birds)**: two independent labial oscillators (Gardner–Mindlin normal form
  ẋ=y, ẏ=−αγ²−βγ²x−γ²x³−γx²y+γ²x²−γxy). γ is a pure time scale, so pitch is exact:
  γ = 2π f0 / F(α,β) with F tabulated offline; α (air-sac pressure) and β (tension)
  shape the waveform (pure tone → pulses → period doubling/chaos). Each labium gates its
  own bronchus; bronchi merge into the trachea → OEC (Helmholtz, volume tracks f0 for
  pure tones) → beak (gape-dependent radiation).
* **Flow-induced valves**: one mass–spring valve model driven by constriction pressure
  (Bernoulli suction / upstream push) for lip trill, tongue-tip trill, uvular trill,
  lip buzz (beatbox bass, elephant trumpet as a lip-reed at the trunk base). Optional
  phase-locked drive if self-oscillation is not robust.
* **Airstream mechanisms**: lung pressure may be negative (ingressive); glottal closure +
  larynx raising/lowering (ejectives/implosives via dV/dt); velaric closure + tongue
  dorsum lowering (clicks). All physical through the FDTD — no special-cased samples.

## 3. Anatomy & articulation (human first, others as parametric variants)

* 2D midsagittal geometry (cm, head frame): fixed outer wall (posterior pharynx, velum,
  hard palate, alveolar ridge, upper incisors, upper lip) and moving inner wall
  (epiglottis/larynx, tongue root, tongue body circle, blade, tip, lower incisors,
  lower lip, jaw).
* Articulators (state, all absolute head-frame so consonant constrictions are exact):
  jaw, tongue body centre (x,y), tongue tip (x,y), tip curl (retroflex), central groove,
  lateral channel, tongue root, lip aperture (negative = compression), lip protrusion,
  lip width/rounding, labiodental, velum port area, larynx height, epilarynx, glottal
  abduction / closure, lung pressure, f0, effort, ventricular, trill drives, dorsum
  lowering (clicks).
* Area function: semipolar grid (horizontal lines in pharynx, radial in oral cavity,
  vertical at lips) intersected with inner/outer contours → midsagittal distance d →
  A = α·d^β per region; tongue grooving/laterals modify A; lengths from midpoint chain.
  Resampled to N uniform sections of current length L(t).
* Calibration: optimise vowel targets against reference formants (Hillenbrand 1995 male
  means, plus cardinal vowels for non-English) with the chain-matrix model. Female/child
  via anatomical scaling (shorter pharynx proportion), not formant warping.
* The body is continuous (`BodyMix`: a female axis and a child axis): anatomy, calibrated
  phoneme targets and default pitch blend the three calibrated bodies by weight. A body
  change rebuilds the airway (its node counts follow the tract length).
* Other mammals/reptiles: same topology, species parameters (snout length, palate
  shape, larynx position/descent, nasal length, lip shape, trunk = long nasal tube,
  vocal sac = Helmholtz side branch). Birds: separate topology (syrinx/trachea/OEC/beak,
  tongue for parrots).

## 4. Control: gestures (Articulatory Phonology–style)

* Phoneme = set of gestures, each controlling a subset of articulators with targets,
  stiffness, blending strength and timing relative to the segment.
* Gestural score: vowel gestures span vowel to vowel (tongue body keeps moving through
  consonants — Öhman coarticulation); consonant gestures overlay their articulators
  with higher strength; uncontrolled articulators relax to neutral.
* Articulator dynamics: critically damped 2nd-order per articulator, physiological
  speeds (lips/tip fast, tongue body/jaw slower, velum/larynx slow).
* Consonant closures use overshoot targets clamped by the geometry (contact), so
  closures are robust; fricatives specify exact gap.
* Text: CMUdict (lazy-loaded, compact) + rule fallback → ARPAbet → IPA; stress;
  syllabification; Klatt-style durations; intonation (declination, H* on stressed
  syllables, final fall / question rise). Singing: notes+lyrics → syllable alignment.

## 5. FX / output

DC blocker → mic model (distance/proximity) → EQ → compressor → saturation → reverb →
limiter. WAV export via the same engine in a Worker (unlimited voices, faster than RT).

## Known risks

* Fricative and burst realism depends on noise-source calibration (gain, placement, spectrum).
* Geometry → area calibration decides vowel quality.
* Self-oscillating valves (trills) can be finicky; keep parametric fallbacks.
