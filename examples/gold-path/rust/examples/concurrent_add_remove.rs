use safemesh_crdt::OrSet;

fn main() {
    let mut left = OrSet::<String, u64>::new();
    let milk = "milk".to_owned();
    left.add(milk.clone(), 100);
    let mut right = left.clone();
    right.apply_remove(right.observed_tokens(&milk));
    left.add(milk.clone(), 201);
    left.merge(&right);
    right.merge(&left);
    assert!(left.contains(&milk) && right.contains(&milk));
    assert_eq!(left.elements(), right.elements());
    println!("milk remains=true; observed token removed, fresh token survives");
}
