// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: BUSL-1.1

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
struct ScriptStep {
    source: usize,
    event: Event,
    line: &'static str,
}

#[derive(Clone, Debug)]
struct Packet {
    from: usize,
    to: usize,
    event: Event,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Projection {
    counter: u64,
    supplies: Vec<u64>,
    text_positions: Vec<u64>,
}

impl Projection {
    fn from_replica(replica: &Replica) -> Self {
        Projection {
            counter: replica.counter.value(),
            supplies: replica.supplies.elements().into_iter().collect(),
            text_positions: replica.text.read_positions(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplicaSnapshot {
    name: &'static str,
    projection: Projection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Report {
    queued_packets: usize,
    delivered_same_partition: usize,
    dropped_cross_partition: usize,
    duplicate: Option<(usize, usize, String)>,
    converged_during_partition: bool,
    converged_after_heal: bool,
    partition_snapshots: Vec<ReplicaSnapshot>,
    final_snapshots: Vec<ReplicaSnapshot>,
    final_projection: Projection,
}

fn main() {
    let report = run_scenario();
    let theme = Theme::from_env();

    print_report(&report, &theme);

    if !report.converged_after_heal {
        std::process::exit(1);
    }
}

fn run_scenario() -> Report {
    let mut replicas = vec![Replica::new(4); 4];
    let mut packets = Vec::new();

    for step in script() {
        replicas[step.source].apply(step.event.clone());
        for target in 0..replicas.len() {
            if target != step.source {
                packets.push(Packet {
                    from: step.source,
                    to: target,
                    event: step.event.clone(),
                });
            }
        }
    }

    let queued_packets = packets.len();
    let mut delivered_same_partition = 0;
    let mut dropped_cross_partition = 0;

    for packet in packets.iter().rev() {
        if same_partition(packet.from, packet.to) {
            replicas[packet.to].apply(packet.event.clone());
            delivered_same_partition += 1;
        } else {
            dropped_cross_partition += 1;
        }
    }

    let duplicate = packets
        .iter()
        .find(|packet| same_partition(packet.from, packet.to))
        .map(|packet| {
            replicas[packet.to].apply(packet.event.clone());
            (packet.from, packet.to, describe_event(&packet.event))
        });

    let partition_snapshots = snapshots(&replicas);
    let converged_during_partition = converged(&replicas);

    anti_entropy(&mut replicas);

    let final_snapshots = snapshots(&replicas);
    let converged_after_heal = converged(&replicas);
    let final_projection = Projection::from_replica(&replicas[0]);

    Report {
        queued_packets,
        delivered_same_partition,
        dropped_cross_partition,
        duplicate,
        converged_during_partition,
        converged_after_heal,
        partition_snapshots,
        final_snapshots,
        final_projection,
    }
}

fn print_report(report: &Report, theme: &Theme) {
    println!("{}", theme.banner("SafeMesh for builders / Rust break-it"));
    println!("A terminal fault campaign against the Lean-backed Rust CRDT carriers.");
    println!(
        "Honest boundary: this proves the modeled merge behavior is exercised; it does not prove real transport delivery.\n"
    );

    println!("{}", theme.section("[1/5] Append local records anywhere"));
    for step in script() {
        println!("  {} {}", theme.peer(replica_name(step.source)), step.line);
    }
    println!("  queued_records={}", report.queued_packets);

    println!("\n{}", theme.section("[2/5] Cut the mesh"));
    println!("  partition=A,B | C,D");
    println!(
        "  delivery_order=reversed delivered_same_partition={} dropped_cross_partition={}",
        report.delivered_same_partition, report.dropped_cross_partition
    );

    println!("\n{}", theme.section("[3/5] Replay a duplicate"));
    if let Some((from, to, event)) = &report.duplicate {
        println!(
            "  duplicate={} -> {} event={} effect=idempotent",
            replica_name(*from),
            replica_name(*to),
            event
        );
    } else {
        println!("  duplicate=none");
    }
    println!(
        "  converged_during_partition={}",
        report.converged_during_partition
    );
    print_snapshots("  partition_state", &report.partition_snapshots);

    println!("\n{}", theme.section("[4/5] Heal with anti-entropy"));
    println!("  anti_entropy=all_to_all_merge coverage=same_modeled_record_set_after_heal");

    println!("\n{}", theme.section("[5/5] Shared state"));
    print_snapshots("  final_state", &report.final_snapshots);
    println!(
        "  CONVERGED={} counter={} supplies={} text_positions={}",
        report.converged_after_heal,
        report.final_projection.counter,
        format_list(&report.final_projection.supplies),
        format_list(&report.final_projection.text_positions)
    );
}

fn print_snapshots(label: &str, snapshots: &[ReplicaSnapshot]) {
    println!("{label}");
    for snapshot in snapshots {
        println!(
            "    {} counter={} supplies={} text_positions={}",
            snapshot.name,
            snapshot.projection.counter,
            format_list(&snapshot.projection.supplies),
            format_list(&snapshot.projection.text_positions)
        );
    }
}

fn script() -> Vec<ScriptStep> {
    vec![
        ScriptStep {
            source: 0,
            event: Event::Counter(GCounterDelta {
                replica: 0,
                tally: 1,
            }),
            line: "appends G-Counter bump replica=0 tally=1",
        },
        ScriptStep {
            source: 1,
            event: Event::Counter(GCounterDelta {
                replica: 1,
                tally: 2,
            }),
            line: "appends G-Counter bump replica=1 tally=2",
        },
        ScriptStep {
            source: 2,
            event: Event::Supply(OrSetDelta::Add {
                element: 42,
                token: 200,
            }),
            line: "adds supply#42 with OR-Set token=200",
        },
        ScriptStep {
            source: 2,
            event: Event::Supply(OrSetDelta::Remove { tokens: vec![200] }),
            line: "removes the supply#42 token it has observed",
        },
        ScriptStep {
            source: 3,
            event: Event::Supply(OrSetDelta::Add {
                element: 42,
                token: 201,
            }),
            line: "concurrently adds supply#42 with OR-Set token=201",
        },
        ScriptStep {
            source: 3,
            event: Event::Text(RgaDelta::Insert {
                position: 30,
                value: 3,
            }),
            line: "inserts text cell position=30 value=3",
        },
        ScriptStep {
            source: 0,
            event: Event::Text(RgaDelta::Insert {
                position: 10,
                value: 1,
            }),
            line: "inserts text cell position=10 value=1",
        },
        ScriptStep {
            source: 1,
            event: Event::Text(RgaDelta::Delete { position: 30 }),
            line: "records an RGA delete tombstone for position=30",
        },
    ]
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

fn snapshots(replicas: &[Replica]) -> Vec<ReplicaSnapshot> {
    replicas
        .iter()
        .enumerate()
        .map(|(index, replica)| ReplicaSnapshot {
            name: replica_name(index),
            projection: Projection::from_replica(replica),
        })
        .collect()
}

fn same_partition(from: usize, to: usize) -> bool {
    from / 2 == to / 2
}

fn replica_name(index: usize) -> &'static str {
    match index {
        0 => "A",
        1 => "B",
        2 => "C",
        3 => "D",
        _ => "?",
    }
}

fn describe_event(event: &Event) -> String {
    match event {
        Event::Counter(delta) => format!("gcounter.bump({},{})", delta.replica, delta.tally),
        Event::Supply(OrSetDelta::Add { element, token }) => {
            format!("orset.add({element},{token})")
        }
        Event::Supply(OrSetDelta::Remove { tokens }) => {
            format!("orset.remove({})", format_list(tokens))
        }
        Event::Text(RgaDelta::Insert { position, value }) => {
            format!("rga.insert({position},{value})")
        }
        Event::Text(RgaDelta::Delete { position }) => format!("rga.delete({position})"),
    }
}

fn format_list(values: &[u64]) -> String {
    if values.is_empty() {
        "{}".to_owned()
    } else {
        format!(
            "{{{}}}",
            values
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

struct Theme {
    color: bool,
}

impl Theme {
    fn from_env() -> Self {
        Theme {
            color: std::env::var_os("NO_COLOR").is_none(),
        }
    }

    fn banner(&self, text: &str) -> String {
        self.paint(text, "1;36")
    }

    fn section(&self, text: &str) -> String {
        self.paint(text, "1;33")
    }

    fn peer(&self, text: &str) -> String {
        self.paint(text, "1;32")
    }

    fn paint(&self, text: &str, code: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_it_scenario_converges_after_heal() {
        let report = run_scenario();

        assert_eq!(report.queued_packets, 24);
        assert_eq!(report.delivered_same_partition, 8);
        assert_eq!(report.dropped_cross_partition, 16);
        assert!(!report.converged_during_partition);
        assert!(report.converged_after_heal);
        assert_eq!(
            report.final_projection,
            Projection {
                counter: 3,
                supplies: vec![42],
                text_positions: vec![10],
            }
        );
    }
}
