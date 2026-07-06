// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use safemesh_crdt::{GCounter, GCounterDelta, OrSet, OrSetDelta, Rga, RgaDelta};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Replica {
    counter: GCounter,
    supplies: OrSet<u64, u64>,
    text: Rga<u64, u64>,
}

impl Replica {
    fn new(replicas: usize) -> Self {
        Replica {
            counter: GCounter::new(replicas),
            supplies: OrSet::new(),
            text: Rga::new(),
        }
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Counter(delta) => self.counter.apply_bump(delta.replica, delta.tally),
            Event::Supply(OrSetDelta::Add { element, token }) => self.supplies.add(element, token),
            Event::Supply(OrSetDelta::Remove { tokens }) => self.supplies.apply_remove(tokens),
            Event::Text(RgaDelta::Insert { position, value }) => self.text.insert(position, value),
            Event::Text(RgaDelta::Delete { position }) => self.text.delete(position),
        }
    }

    fn merge(&mut self, other: &Self) {
        self.counter.merge(&other.counter);
        self.supplies.merge(&other.supplies);
        self.text.merge(&other.text);
    }
}

#[derive(Clone, Debug)]
enum Event {
    Counter(GCounterDelta),
    Supply(OrSetDelta<u64, u64>),
    Text(RgaDelta<u64, u64>),
}

#[derive(Clone, Debug)]
struct Packet {
    from: usize,
    to: usize,
    event: Event,
}

fn main() {
    let mut replicas = vec![Replica::new(4); 4];
    let script = vec![
        (
            0,
            Event::Counter(GCounterDelta {
                replica: 0,
                tally: 1,
            }),
        ),
        (
            1,
            Event::Counter(GCounterDelta {
                replica: 1,
                tally: 2,
            }),
        ),
        (
            2,
            Event::Supply(OrSetDelta::Add {
                element: 42,
                token: 200,
            }),
        ),
        (2, Event::Supply(OrSetDelta::Remove { tokens: vec![200] })),
        (
            3,
            Event::Supply(OrSetDelta::Add {
                element: 42,
                token: 201,
            }),
        ),
        (
            3,
            Event::Text(RgaDelta::Insert {
                position: 30,
                value: 3,
            }),
        ),
        (
            0,
            Event::Text(RgaDelta::Insert {
                position: 10,
                value: 1,
            }),
        ),
        (1, Event::Text(RgaDelta::Delete { position: 30 })),
    ];

    let mut packets = Vec::new();
    for (source, event) in script {
        replicas[source].apply(event.clone());
        for target in 0..replicas.len() {
            if target != source {
                packets.push(Packet {
                    from: source,
                    to: target,
                    event: event.clone(),
                });
            }
        }
    }

    println!("SafeMesh break-it demo");
    println!("phase=partition groups=[0,1]|[2,3]");

    let mut delivered = 0;
    for packet in packets.iter().rev() {
        let same_partition = packet.from / 2 == packet.to / 2;
        if same_partition && (packet.from + packet.to) % 3 != 0 {
            replicas[packet.to].apply(packet.event.clone());
            delivered += 1;
        }
    }
    println!("phase=drop+reorder delivered_same_partition={delivered}; cross_partition=dropped");

    if let Some(packet) = packets
        .iter()
        .find(|packet| packet.from / 2 == packet.to / 2)
    {
        replicas[packet.to].apply(packet.event.clone());
        println!(
            "phase=duplicate replayed_packet={}->{}",
            packet.from, packet.to
        );
    }

    println!("converged_during_partition={}", converged(&replicas));
    println!("phase=heal anti_entropy=all_to_all_merge");
    anti_entropy(&mut replicas);

    let final_state = &replicas[0];
    println!(
        "CONVERGED={} counter={} supplies={:?} text_positions={:?}",
        converged(&replicas),
        final_state.counter.value(),
        final_state.supplies.elements(),
        final_state.text.read_positions()
    );

    if !converged(&replicas) {
        std::process::exit(1);
    }
}

fn anti_entropy(replicas: &mut [Replica]) {
    let mut joined = replicas[0].clone();
    for replica in replicas.iter().skip(1) {
        joined.merge(replica);
    }
    for replica in replicas {
        replica.merge(&joined);
    }
}

fn converged(replicas: &[Replica]) -> bool {
    replicas
        .first()
        .map(|first| replicas.iter().all(|replica| replica == first))
        .unwrap_or(true)
}
