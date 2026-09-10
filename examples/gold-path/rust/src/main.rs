use safemesh_crdt::{
    local::DurableReplica, ownership::WriterConfig, Admission, GCounter, GCounterDelta, OrSet,
    OrSetDelta, Record, WireDecode, WireEncode, WireError,
};
use std::{fs, path::Path};

type Counter = DurableReplica<GCounter>;
type Members = DurableReplica<OrSet<String, u64>>;

fn main() {
    let step = std::env::args().nth(1).expect("pass persist or restart");
    let root = Path::new(".gold-rust");
    let config = WriterConfig {
        writers: 2,
        writer: 0,
    };
    let counter_dir = root.join("counter");
    let members_dir = root.join("members");
    if step == "persist" {
        fs::create_dir(root).expect("use a fresh directory; never reset an existing writer");
        fs::create_dir(&counter_dir).unwrap();
        fs::create_dir(&members_dir).unwrap();
        let mut counter = Counter::counter(&counter_dir, config).unwrap();
        let mut members = Members::utf8_set(&members_dir, config).unwrap();
        // Each call commits its own transaction before returning.
        counter.bump(counter.ticket(), 3).unwrap();
        members.add(members.ticket(), "compass".into()).unwrap();
        assert_eq!(counter.state().value(), 3);
        assert_eq!(
            members.state().elements(),
            ["compass".to_string()].into_iter().collect()
        );
        println!("saved counter=3 members=[compass]");
    } else {
        assert_eq!(step, "restart");
        // This process obtains a new ticket and restores both state and allocation.
        let mut counter = Counter::restart_counter(&counter_dir, config).unwrap();
        let mut members = Members::restart_utf8_set(&members_dir, config).unwrap();
        assert_eq!(counter.state().value(), 3);
        assert_eq!(
            members.state().elements(),
            ["compass".to_string()].into_iter().collect()
        );
        println!("restored counter=3 members=[compass]");
        counter.bump(counter.ticket(), 4).unwrap(); // cumulative tally, not +4
        members.add(members.ticket(), "map".into()).unwrap(); // allocates a fresh token
        let peer = WriterConfig {
            writers: 2,
            writer: 1,
        };
        let mut other_counter = Counter::counter(&counter_dir, peer).unwrap();
        let mut other_members = Members::utf8_set(&members_dir, peer).unwrap();
        other_counter.bump(other_counter.ticket(), 2).unwrap();
        other_members
            .add(other_members.ticket(), "rope".into())
            .unwrap();
        // In an application, your transport carries these encoded records.
        let counter_bytes: Vec<_> = counter
            .log()
            .records()
            .iter()
            .map(|r| r.to_wire_bytes().unwrap())
            .collect();
        let member_bytes: Vec<_> = members
            .log()
            .records()
            .iter()
            .map(|r| r.to_wire_bytes().unwrap())
            .collect();
        for bytes in counter_bytes {
            let record = Record::<GCounterDelta>::from_wire_bytes(&bytes).unwrap();
            assert_eq!(
                other_counter
                    .receive(other_counter.ticket(), record)
                    .unwrap(),
                Admission::Accepted
            );
        }
        for bytes in member_bytes {
            let record = Record::<OrSetDelta<String, u64>>::from_wire_bytes(&bytes).unwrap();
            assert_eq!(
                other_members
                    .receive(other_members.ticket(), record)
                    .unwrap(),
                Admission::Accepted
            );
        }
        for r in other_counter.log().records() {
            let r = Record::<GCounterDelta>::from_wire_bytes(&r.to_wire_bytes().unwrap()).unwrap();
            assert_ne!(
                counter.receive(counter.ticket(), r).unwrap(),
                Admission::Collision
            );
        }
        for r in other_members.log().records() {
            let r = Record::<OrSetDelta<String, u64>>::from_wire_bytes(&r.to_wire_bytes().unwrap())
                .unwrap();
            assert_ne!(
                members.receive(members.ticket(), r).unwrap(),
                Admission::Collision
            );
        }
        assert_eq!(counter.state().value(), 6);
        assert_eq!(
            members.state().elements(),
            ["compass".to_string(), "map".to_string(), "rope".to_string()]
                .into_iter()
                .collect()
        );
        assert_eq!(counter.state(), other_counter.state());
        assert_eq!(members.state(), other_members.state()); // includes tokens and tombstones
        assert_eq!(counter.log().version(), other_counter.log().version());
        assert_eq!(members.log().version(), other_members.log().version());
        assert_eq!(counter.log().records().len(), 3);
        assert_eq!(members.log().records().len(), 3);
        println!("synced counter=6 members=[compass,map,rope] records=3+3");
        let before = counter.log().to_wire_bytes().unwrap();
        let error = Record::<GCounterDelta>::from_wire_bytes(&[]).unwrap_err();
        assert_eq!(error, WireError::UnexpectedEof);
        assert_eq!(counter.log().to_wire_bytes().unwrap(), before);
        println!("malformed record: {error:?}");
        // Only discard this disposable exercise after all writers release their locks.
        drop((counter, members, other_counter, other_members));
        for dir in [&counter_dir, &members_dir] {
            for entry in fs::read_dir(dir).unwrap() {
                fs::remove_file(entry.unwrap().path()).unwrap();
            }
            fs::remove_dir(dir).unwrap();
        }
        fs::remove_dir(root).unwrap();
        println!("cleaned exercise stores");
    }
}
