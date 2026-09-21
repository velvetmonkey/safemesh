use safemesh_crdt::GCounter;

fn main() {
    let mut added_a = GCounter::new(2);
    let mut removed_a = GCounter::new(2);
    added_a.try_apply_bump(0, 10).unwrap();
    let mut added_b = added_a.clone();
    let mut removed_b = removed_a.clone();
    removed_a.try_apply_bump(0, 3).unwrap();
    added_b.try_apply_bump(1, 2).unwrap();
    removed_b.try_apply_bump(1, 1).unwrap();
    added_a.try_merge(&added_b).unwrap();
    removed_a.try_merge(&removed_b).unwrap();
    added_b.try_merge(&added_a).unwrap();
    removed_b.try_merge(&removed_a).unwrap();
    assert_eq!(added_a.state(), added_b.state());
    assert_eq!(removed_a.state(), removed_b.state());
    // Checked conversion/subtraction: counter totals themselves are u128.
    let stock = i128::try_from(added_a.value())
        .unwrap()
        .checked_sub(i128::try_from(removed_a.value()).unwrap())
        .unwrap();
    assert_eq!(stock, 8);
    println!(
        "added={} removed={} stock={stock}",
        added_a.value(),
        removed_a.value()
    );
}
