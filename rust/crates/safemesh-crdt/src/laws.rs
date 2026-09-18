// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use super::{Crdt, Mergeable};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Law {
    Commutative,
    Associative,
    Idempotent,
    Identity,
    Convergence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LawFailure {
    pub law: Law,
    pub a: usize,
    pub b: Option<usize>,
    pub c: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LawReport {
    pub scenarios: usize,
}

pub fn check_merge_laws<T>(identity: &T, samples: &[T]) -> Result<LawReport, LawFailure>
where
    T: Mergeable + Clone + PartialEq,
{
    let mut scenarios = 0;

    for (a_idx, a) in samples.iter().enumerate() {
        let mut aa = a.clone();
        aa.merge(a).map_err(|_| LawFailure {
            law: Law::Idempotent,
            a: a_idx,
            b: None,
            c: None,
        })?;
        scenarios += 1;
        if aa != *a {
            return Err(LawFailure {
                law: Law::Idempotent,
                a: a_idx,
                b: None,
                c: None,
            });
        }

        let mut left_identity = identity.clone();
        left_identity.merge(a).map_err(|_| LawFailure {
            law: Law::Identity,
            a: a_idx,
            b: None,
            c: None,
        })?;
        let mut right_identity = a.clone();
        right_identity.merge(identity).map_err(|_| LawFailure {
            law: Law::Identity,
            a: a_idx,
            b: None,
            c: None,
        })?;
        scenarios += 2;
        if left_identity != *a || right_identity != *a {
            return Err(LawFailure {
                law: Law::Identity,
                a: a_idx,
                b: None,
                c: None,
            });
        }

        for (b_idx, b) in samples.iter().enumerate() {
            let mut ab = a.clone();
            ab.merge(b).map_err(|_| LawFailure {
                law: Law::Commutative,
                a: a_idx,
                b: Some(b_idx),
                c: None,
            })?;
            let mut ba = b.clone();
            ba.merge(a).map_err(|_| LawFailure {
                law: Law::Commutative,
                a: a_idx,
                b: Some(b_idx),
                c: None,
            })?;
            scenarios += 1;
            if ab != ba {
                return Err(LawFailure {
                    law: Law::Commutative,
                    a: a_idx,
                    b: Some(b_idx),
                    c: None,
                });
            }

            for (c_idx, c) in samples.iter().enumerate() {
                let mut left = a.clone();
                left.merge(b).map_err(|_| LawFailure {
                    law: Law::Associative,
                    a: a_idx,
                    b: Some(b_idx),
                    c: Some(c_idx),
                })?;
                left.merge(c).map_err(|_| LawFailure {
                    law: Law::Associative,
                    a: a_idx,
                    b: Some(b_idx),
                    c: Some(c_idx),
                })?;

                let mut right_inner = b.clone();
                right_inner.merge(c).map_err(|_| LawFailure {
                    law: Law::Associative,
                    a: a_idx,
                    b: Some(b_idx),
                    c: Some(c_idx),
                })?;
                let mut right = a.clone();
                right.merge(&right_inner).map_err(|_| LawFailure {
                    law: Law::Associative,
                    a: a_idx,
                    b: Some(b_idx),
                    c: Some(c_idx),
                })?;

                scenarios += 1;
                if left != right {
                    return Err(LawFailure {
                        law: Law::Associative,
                        a: a_idx,
                        b: Some(b_idx),
                        c: Some(c_idx),
                    });
                }
            }
        }
    }

    Ok(LawReport { scenarios })
}

pub fn check_crdt_convergence<C>(
    seed_state: &C,
    deltas: &[C::Delta],
) -> Result<LawReport, LawFailure>
where
    C: Crdt + Clone + PartialEq,
    C::Delta: Clone,
{
    let expected = apply_all(seed_state, deltas.iter().cloned());
    let mut scenarios = 1;

    let reverse = apply_all(seed_state, deltas.iter().cloned().rev());
    scenarios += 1;
    if reverse != expected {
        return Err(convergence_failure(0));
    }

    let duplicated = apply_all(
        seed_state,
        deltas.iter().cloned().flat_map(|d| [d.clone(), d]),
    );
    scenarios += 1;
    if duplicated != expected {
        return Err(convergence_failure(1));
    }

    for (seed_idx, seed) in [0x51a7_3eed_u64, 0xc0ff_ee13_u64, 0x5afe_0001_u64]
        .iter()
        .copied()
        .enumerate()
    {
        let shuffled = shuffled(deltas, seed);
        let shuffled_state = apply_all(seed_state, shuffled);
        scenarios += 1;
        if shuffled_state != expected {
            return Err(convergence_failure(2 + seed_idx));
        }
    }

    let mut even = seed_state.clone();
    let mut odd = seed_state.clone();
    for (idx, delta) in deltas.iter().cloned().enumerate() {
        if idx % 2 == 0 {
            even.apply_delta(delta);
        } else {
            odd.apply_delta(delta);
        }
    }
    even.merge(&odd).map_err(|_| convergence_failure(5))?;
    scenarios += 1;
    if even != expected {
        return Err(convergence_failure(5));
    }

    let mut lossy_a = seed_state.clone();
    let mut lossy_b = seed_state.clone();
    for (idx, delta) in deltas.iter().cloned().enumerate() {
        if idx % 3 == 0 {
            lossy_b.apply_delta(delta);
        } else {
            lossy_a.apply_delta(delta);
        }
    }
    lossy_a
        .merge(&lossy_b)
        .map_err(|_| convergence_failure(6))?;
    scenarios += 1;
    if lossy_a != expected {
        return Err(convergence_failure(6));
    }

    Ok(LawReport { scenarios })
}

fn apply_all<C, I>(seed_state: &C, deltas: I) -> C
where
    C: Crdt + Clone,
    I: IntoIterator<Item = C::Delta>,
{
    let mut state = seed_state.clone();
    for delta in deltas {
        state.apply_delta(delta);
    }
    state
}

fn convergence_failure(a: usize) -> LawFailure {
    LawFailure {
        law: Law::Convergence,
        a,
        b: None,
        c: None,
    }
}

fn shuffled<T: Clone>(items: &[T], seed: u64) -> Vec<T> {
    let mut out = items.to_vec();
    let mut rng = XorShift64::new(seed);
    let len = out.len();
    if len < 2 {
        return out;
    }
    let mut i = len - 1;
    while i > 0 {
        let j = rng.next_usize(i + 1);
        out.swap(i, j);
        i -= 1;
    }
    out
}

struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        let state = if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        };
        XorShift64 { state }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn next_usize(&mut self, upper: usize) -> usize {
        (self.next_u64() as usize) % upper
    }
}
