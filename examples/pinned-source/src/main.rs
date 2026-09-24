use safemesh_crdt::GCounter;

fn main() {
    let mut left = GCounter::new(2);
    let mut right = GCounter::new(2);
    left.try_apply_bump(0, 4).unwrap();
    right.try_apply_bump(1, 2).unwrap();
    left.try_merge(&right).unwrap();
    right.try_merge(&left).unwrap();
    assert_eq!(left.state(), right.state());
    assert_eq!(left.value(), 6);
    println!("merged total={}", left.value());
    left.try_merge(&right).unwrap();
    assert_eq!(left.value(), 6);
    println!("duplicate replay total={}", left.value());
}
