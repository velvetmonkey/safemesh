use safemesh_crdt::GCounter;

fn main() {
    let mut a = GCounter::new(2);
    let mut b = GCounter::new(2);

    // Each writer reports its own total.
    a.try_apply_bump(0, 2).unwrap();
    b.try_apply_bump(1, 3).unwrap();

    a.try_merge(&b).unwrap();
    assert_eq!(a.value(), 5);
}

#[test]
fn replay_same_state() {
    main();
    let mut a = GCounter::new(2);
    let mut b = GCounter::new(2);
    a.try_apply_bump(0, 2).unwrap();
    b.try_apply_bump(1, 3).unwrap();
    a.try_merge(&b).unwrap();
    assert_eq!(a.value(), 5);
    a.try_merge(&b).unwrap();
    assert_eq!(a.value(), 5);
}
