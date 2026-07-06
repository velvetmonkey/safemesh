// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(feature = "laws")]

use safemesh_crdt::laws::{check_crdt_convergence, check_merge_laws};
use safemesh_crdt::{
    EnableWinsFlag, EnableWinsFlagDelta, GCounter, GCounterDelta, GSet, LwwRegister,
    LwwRegisterDelta, OrSet, OrSetDelta, PnCounter, PnCounterDelta, Rga, RgaDelta,
};

#[test]
fn merge_laws_hold_for_in_house_samples() {
    let mut g_a = GCounter::new(3);
    g_a.apply_bump(0, 1);
    let mut g_b = GCounter::new(3);
    g_b.apply_bump(1, 2);
    let mut g_c = GCounter::new(3);
    g_c.apply_bump(2, 3);
    assert!(check_merge_laws(&GCounter::new(3), &[g_a, g_b, g_c]).is_ok());

    let mut set_a = GSet::new();
    set_a.insert(1_u64);
    let mut set_b = GSet::new();
    set_b.insert(2);
    assert!(check_merge_laws(&GSet::new(), &[set_a, set_b]).is_ok());

    let mut reg_a = LwwRegister::new();
    reg_a.set(1, 1, 10_u64);
    let mut reg_b = LwwRegister::new();
    reg_b.set(2, 1, 20);
    let mut reg_c = LwwRegister::new();
    reg_c.set(2, 2, 30);
    assert!(check_merge_laws(&LwwRegister::new(), &[reg_a, reg_b, reg_c]).is_ok());

    let mut flag_a = EnableWinsFlag::new();
    flag_a.enable(1_u64);
    let mut flag_b = EnableWinsFlag::new();
    flag_b.disable([1]);
    let mut flag_c = EnableWinsFlag::new();
    flag_c.enable(2);
    assert!(check_merge_laws(&EnableWinsFlag::new(), &[flag_a, flag_b, flag_c]).is_ok());
}

#[test]
fn convergence_harness_hammers_delta_delivery_shapes() {
    assert!(check_crdt_convergence(
        &GCounter::new(4),
        &[
            GCounterDelta {
                replica: 0,
                tally: 1
            },
            GCounterDelta {
                replica: 2,
                tally: 7
            },
            GCounterDelta {
                replica: 0,
                tally: 4
            },
        ],
    )
    .is_ok());

    assert!(check_crdt_convergence(
        &PnCounter::new(4),
        &[
            PnCounterDelta::Inc {
                replica: 0,
                tally: 5
            },
            PnCounterDelta::Dec {
                replica: 1,
                tally: 3
            },
            PnCounterDelta::Inc {
                replica: 2,
                tally: 8
            },
        ],
    )
    .is_ok());

    assert!(check_crdt_convergence(
        &OrSet::new(),
        &[
            OrSetDelta::Add {
                element: 1_u64,
                token: 10_u64
            },
            OrSetDelta::Remove { tokens: vec![10] },
            OrSetDelta::Add {
                element: 1,
                token: 11
            },
            OrSetDelta::Add {
                element: 2,
                token: 20
            },
        ],
    )
    .is_ok());

    assert!(check_crdt_convergence(
        &Rga::new(),
        &[
            RgaDelta::Insert {
                position: 30_u64,
                value: 3_u64
            },
            RgaDelta::Insert {
                position: 10,
                value: 1
            },
            RgaDelta::Insert {
                position: 20,
                value: 2
            },
            RgaDelta::Delete { position: 10 },
        ],
    )
    .is_ok());

    assert!(check_crdt_convergence(
        &LwwRegister::new(),
        &[
            LwwRegisterDelta {
                timestamp: 1,
                replica: 1,
                value: 10_u64,
            },
            LwwRegisterDelta {
                timestamp: 2,
                replica: 1,
                value: 20,
            },
            LwwRegisterDelta {
                timestamp: 2,
                replica: 2,
                value: 30,
            },
        ],
    )
    .is_ok());

    assert!(check_crdt_convergence(
        &EnableWinsFlag::new(),
        &[
            EnableWinsFlagDelta::Enable { token: 1_u64 },
            EnableWinsFlagDelta::Disable { tokens: vec![1] },
            EnableWinsFlagDelta::Enable { token: 2 },
        ],
    )
    .is_ok());
}
