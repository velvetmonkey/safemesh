// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

#![cfg(feature = "laws")]

use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, TestCaseResult};
use safemesh_crdt::laws::{check_crdt_convergence, check_merge_laws};
use safemesh_crdt::{
    Crdt, EnableWinsFlag, EnableWinsFlagDelta, GCounter, GCounterDelta, GSet, PnCounter,
    PnCounterDelta,
};

// Three independent histories give the merge harness every operand of associativity.
// Bounded history lengths keep its cubic state comparisons cheap enough for 256 cases.
fn histories<D: Strategy>(delta: D) -> impl Strategy<Value = [Vec<D::Value>; 3]> {
    proptest::array::uniform3(proptest::collection::vec(delta, 0..17))
}

fn check_histories<C>(identity: C, histories: [Vec<C::Delta>; 3]) -> TestCaseResult
where
    C: Crdt + Clone + PartialEq,
    C::Delta: Clone,
{
    let samples: Vec<_> = histories
        .iter()
        .map(|history| {
            let mut state = identity.clone();
            for delta in history {
                state.apply_delta(delta.clone());
            }
            state
        })
        .collect();
    let merge = check_merge_laws(&identity, &samples);
    prop_assert!(merge.is_ok(), "merge law failure: {:?}", merge);
    for history in &histories {
        let convergence = check_crdt_convergence(&identity, history);
        prop_assert!(
            convergence.is_ok(),
            "convergence failure: {:?}",
            convergence
        );
    }
    // Also deliver all histories together, including to an already populated replica.
    let deltas: Vec<_> = histories.into_iter().flatten().collect();
    for seed in std::iter::once(&identity).chain(&samples) {
        let convergence = check_crdt_convergence(seed, &deltas);
        prop_assert!(
            convergence.is_ok(),
            "combined convergence failure: {:?}",
            convergence
        );
    }
    Ok(())
}

fn gcounter_histories() -> impl Strategy<Value = (usize, [Vec<GCounterDelta>; 3])> {
    (1usize..9).prop_flat_map(|replicas| {
        let delta = (0..replicas, any::<u64>())
            .prop_map(|(replica, tally)| GCounterDelta { replica, tally });
        (Just(replicas), histories(delta))
    })
}

fn pncounter_histories() -> impl Strategy<Value = (usize, [Vec<PnCounterDelta>; 3])> {
    (1usize..9).prop_flat_map(|replicas| {
        let delta =
            (any::<bool>(), 0..replicas, any::<u64>()).prop_map(|(increment, replica, tally)| {
                if increment {
                    PnCounterDelta::Inc { replica, tally }
                } else {
                    PnCounterDelta::Dec { replica, tally }
                }
            });
        (Just(replicas), histories(delta))
    })
}

fn flag_delta() -> impl Strategy<Value = EnableWinsFlagDelta<u8>> {
    // A small token domain exercises overlap and redelivery. A disable may arrive
    // before its enable: tombstones are valid deltas even on a fresh receiver.
    prop_oneof![
        (0u8..16).prop_map(|token| EnableWinsFlagDelta::Enable { token }),
        proptest::collection::vec(0u8..16, 0..9)
            .prop_map(|tokens| EnableWinsFlagDelta::Disable { tokens }),
    ]
}

proptest! {
    // Explicitly retain at least 256 random cases even if PROPTEST_CASES is lower.
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource("proptest-regressions"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn gcounter_laws((replicas, histories) in gcounter_histories()) {
        check_histories(GCounter::new(replicas), histories)?;
    }

    #[test]
    fn gset_laws(histories in histories(any::<u8>())) {
        check_histories(GSet::new(), histories)?;
    }

    #[test]
    fn pncounter_laws((replicas, histories) in pncounter_histories()) {
        check_histories(PnCounter::new(replicas), histories)?;
    }

    #[test]
    fn enable_wins_flag_laws(histories in histories(flag_delta())) {
        check_histories(EnableWinsFlag::new(), histories)?;
    }
}
