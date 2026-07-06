// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use safemesh_crdt::{
    anti_entropy, EventLog, GCounter, GCounterDelta, GSet, InMemoryTransport, OrSet, OrSetDelta,
    Record, Rga, RgaDelta, TransportAdapter, TransportError,
};

const CLINIC: u64 = 0;
const COURIER: u64 = 1;
const LAB: u64 = 2;
const REPLICA_COUNT: usize = 3;

const SAMPLE_ID: u64 = 9001;

const HOLDER_CLINIC: u64 = 1;
const HOLDER_COURIER: u64 = 2;
const HOLDER_LAB: u64 = 3;

const TOKEN_CLINIC_CUSTODY: u64 = 1001;
const TOKEN_COURIER_CUSTODY: u64 = 1002;
const TOKEN_LAB_CUSTODY: u64 = 1003;
const TOKEN_TEMP_ALERT: u64 = 2001;

const ALERT_TEMPERATURE_EXCURSION: u64 = 1;

const AUDIT_SAMPLE_COLLECTED: u64 = 100;
const AUDIT_COURIER_PICKUP: u64 = 200;
const AUDIT_FREEZER_POWER_BLIP: u64 = 300;
const AUDIT_LAB_RECEIPT: u64 = 400;

#[derive(Clone, Debug, PartialEq, Eq)]
enum ColdChainDelta {
    Sample(u64),
    Custody(OrSetDelta<u64, u64>),
    Alert(OrSetDelta<u64, u64>),
    Audit(RgaDelta<u64, u64>),
    Count(GCounterDelta),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ColdChainState {
    samples: GSet<u64>,
    custody: OrSet<u64, u64>,
    alerts: OrSet<u64, u64>,
    audit: Rga<u64, u64>,
    event_count: GCounter,
}

impl ColdChainState {
    fn new() -> Self {
        ColdChainState {
            samples: GSet::new(),
            custody: OrSet::new(),
            alerts: OrSet::new(),
            audit: Rga::new(),
            event_count: GCounter::new(REPLICA_COUNT),
        }
    }

    fn apply(&mut self, delta: ColdChainDelta) {
        match delta {
            ColdChainDelta::Sample(sample_id) => self.samples.insert(sample_id),
            ColdChainDelta::Custody(OrSetDelta::Add { element, token }) => {
                self.custody.add(element, token)
            }
            ColdChainDelta::Custody(OrSetDelta::Remove { tokens }) => {
                self.custody.apply_remove(tokens)
            }
            ColdChainDelta::Alert(OrSetDelta::Add { element, token }) => {
                self.alerts.add(element, token)
            }
            ColdChainDelta::Alert(OrSetDelta::Remove { tokens }) => {
                self.alerts.apply_remove(tokens)
            }
            ColdChainDelta::Audit(RgaDelta::Insert { position, value }) => {
                self.audit.insert(position, value)
            }
            ColdChainDelta::Audit(RgaDelta::Delete { position }) => self.audit.delete(position),
            ColdChainDelta::Count(delta) => self.event_count.apply_bump(delta.replica, delta.tally),
        }
    }

    fn projection(&self) -> Projection {
        let samples = self.samples.elements().iter().copied().collect();
        let active_holders = self
            .custody
            .elements()
            .into_iter()
            .filter_map(|pair| {
                let (sample, holder) = decode_custody(pair);
                if sample == SAMPLE_ID {
                    Some(holder_name(holder))
                } else {
                    None
                }
            })
            .collect();
        let active_alerts = self.alerts.elements().into_iter().map(alert_name).collect();
        let audit_codes = self
            .audit
            .live_entries()
            .into_iter()
            .map(|(_, code)| audit_name(code))
            .collect();

        Projection {
            samples,
            active_holders,
            active_alerts,
            audit_codes,
            event_count: self.event_count.value(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Replica {
    id: u64,
    next_event_tally: u64,
    log: EventLog<ColdChainDelta>,
    state: ColdChainState,
}

impl Replica {
    fn new(id: u64) -> Self {
        Replica {
            id,
            next_event_tally: 0,
            log: EventLog::new(),
            state: ColdChainState::new(),
        }
    }

    fn emit(&mut self, delta: ColdChainDelta) {
        self.log.append(self.id, delta.clone());
        self.state.apply(delta);
    }

    fn note_domain_event(&mut self, audit_position: u64, audit_code: u64) {
        self.emit(ColdChainDelta::Audit(RgaDelta::Insert {
            position: audit_position,
            value: audit_code,
        }));
        self.next_event_tally += 1;
        self.emit(ColdChainDelta::Count(GCounterDelta {
            replica: self.id as usize,
            tally: self.next_event_tally,
        }));
    }

    fn receive(&mut self, records: Vec<Record<ColdChainDelta>>) {
        let fresh: Vec<_> = records
            .into_iter()
            .filter(|record| !self.log.version().includes(record.id))
            .collect();

        for record in &fresh {
            self.state.apply(record.delta.clone());
        }
        self.log.merge_records(fresh);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Projection {
    samples: Vec<u64>,
    active_holders: Vec<&'static str>,
    active_alerts: Vec<&'static str>,
    audit_codes: Vec<&'static str>,
    event_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Report {
    cross_partition_blocked: bool,
    converged_during_partition: bool,
    converged_after_heal: bool,
    final_projection: Projection,
}

fn main() {
    let report = run_scenario();

    println!("SafeMesh cold-chain kill-test");
    println!("vertical=field-science-cold-chain software_only=true");
    println!("phase=drop+duplicate+reorder transport=InMemoryTransport");
    println!("phase=partition groups=[clinic,courier]|[lab]");
    println!("cross_partition_blocked={}", report.cross_partition_blocked);
    println!(
        "converged_during_partition={}",
        report.converged_during_partition
    );
    println!("phase=heal anti_entropy=EventLog::since(version_vector)");
    println!(
        "CONVERGED={} samples={:?} active_holders={:?} active_alerts={:?} audit={:?} event_count={}",
        report.converged_after_heal,
        report.final_projection.samples,
        report.final_projection.active_holders,
        report.final_projection.active_alerts,
        report.final_projection.audit_codes,
        report.final_projection.event_count
    );
    println!(
        "KILL_TEST_PASS={} questions=10",
        report.converged_after_heal
    );

    if !report.converged_after_heal {
        std::process::exit(1);
    }
}

fn run_scenario() -> Report {
    let mut replicas = vec![
        Replica::new(CLINIC),
        Replica::new(COURIER),
        Replica::new(LAB),
    ];
    let mut transport = InMemoryTransport::new();
    for peer in 0..REPLICA_COUNT {
        transport.subscribe(peer as u64);
    }

    collect_sample(&mut replicas[CLINIC as usize]);

    transport.drop_next_send();
    sync_pair(&mut transport, &replicas, CLINIC, COURIER).unwrap();
    transport.duplicate_next_send();
    sync_pair(&mut transport, &replicas, CLINIC, LAB).unwrap();
    sync_pair(&mut transport, &replicas, CLINIC, COURIER).unwrap();
    transport.reverse_pending_for(LAB);
    drain_all(&mut transport, &mut replicas);

    transport.set_connected(CLINIC, LAB, false);
    transport.set_connected(COURIER, LAB, false);

    courier_pickup(&mut replicas[COURIER as usize]);
    lab_power_blip(&mut replicas[LAB as usize]);

    sync_pair(&mut transport, &replicas, COURIER, CLINIC).unwrap();
    let cross_partition_blocked = matches!(
        sync_pair(&mut transport, &replicas, COURIER, LAB),
        Err(TransportError::Disconnected {
            from: COURIER,
            to: LAB
        })
    );
    drain_all(&mut transport, &mut replicas);

    let converged_during_partition = converged(&replicas);

    transport.set_connected(CLINIC, LAB, true);
    transport.set_connected(COURIER, LAB, true);
    anti_entropy_round(&mut transport, &mut replicas);

    lab_receipt(&mut replicas[LAB as usize]);
    anti_entropy_round(&mut transport, &mut replicas);

    Report {
        cross_partition_blocked,
        converged_during_partition,
        converged_after_heal: converged(&replicas),
        final_projection: replicas[0].state.projection(),
    }
}

fn collect_sample(replica: &mut Replica) {
    replica.emit(ColdChainDelta::Sample(SAMPLE_ID));
    replica.emit(ColdChainDelta::Custody(OrSetDelta::Add {
        element: custody_pair(SAMPLE_ID, HOLDER_CLINIC),
        token: TOKEN_CLINIC_CUSTODY,
    }));
    replica.note_domain_event(10, AUDIT_SAMPLE_COLLECTED);
}

fn courier_pickup(replica: &mut Replica) {
    replica.emit(ColdChainDelta::Custody(OrSetDelta::Remove {
        tokens: vec![TOKEN_CLINIC_CUSTODY],
    }));
    replica.emit(ColdChainDelta::Custody(OrSetDelta::Add {
        element: custody_pair(SAMPLE_ID, HOLDER_COURIER),
        token: TOKEN_COURIER_CUSTODY,
    }));
    replica.note_domain_event(20, AUDIT_COURIER_PICKUP);
}

fn lab_power_blip(replica: &mut Replica) {
    replica.emit(ColdChainDelta::Alert(OrSetDelta::Add {
        element: ALERT_TEMPERATURE_EXCURSION,
        token: TOKEN_TEMP_ALERT,
    }));
    replica.note_domain_event(30, AUDIT_FREEZER_POWER_BLIP);
}

fn lab_receipt(replica: &mut Replica) {
    replica.emit(ColdChainDelta::Custody(OrSetDelta::Remove {
        tokens: vec![TOKEN_COURIER_CUSTODY],
    }));
    replica.emit(ColdChainDelta::Custody(OrSetDelta::Add {
        element: custody_pair(SAMPLE_ID, HOLDER_LAB),
        token: TOKEN_LAB_CUSTODY,
    }));
    replica.note_domain_event(40, AUDIT_LAB_RECEIPT);
}

fn sync_pair(
    transport: &mut InMemoryTransport<ColdChainDelta>,
    replicas: &[Replica],
    from: u64,
    to: u64,
) -> Result<(), TransportError> {
    let remote_version = replicas[to as usize].log.version().clone();
    anti_entropy(
        transport,
        from,
        to,
        &replicas[from as usize].log,
        &remote_version,
    )
}

fn anti_entropy_round(transport: &mut InMemoryTransport<ColdChainDelta>, replicas: &mut [Replica]) {
    for from in 0..replicas.len() {
        for to in 0..replicas.len() {
            if from != to {
                sync_pair(transport, replicas, from as u64, to as u64).unwrap();
            }
        }
    }
    drain_all(transport, replicas);
}

fn drain_all(transport: &mut InMemoryTransport<ColdChainDelta>, replicas: &mut [Replica]) {
    for peer in 0..replicas.len() {
        let incoming = transport.drain(peer as u64);
        for envelope in incoming {
            replicas[peer].receive(envelope.records);
        }
    }
}

fn converged(replicas: &[Replica]) -> bool {
    replicas
        .first()
        .map(|first| replicas.iter().all(|replica| replica.state == first.state))
        .unwrap_or(true)
}

fn custody_pair(sample_id: u64, holder: u64) -> u64 {
    sample_id * 10 + holder
}

fn decode_custody(pair: u64) -> (u64, u64) {
    (pair / 10, pair % 10)
}

fn holder_name(holder: u64) -> &'static str {
    match holder {
        HOLDER_CLINIC => "clinic",
        HOLDER_COURIER => "courier",
        HOLDER_LAB => "lab",
        _ => "unknown",
    }
}

fn alert_name(alert: u64) -> &'static str {
    match alert {
        ALERT_TEMPERATURE_EXCURSION => "temperature_excursion",
        _ => "unknown",
    }
}

fn audit_name(code: u64) -> &'static str {
    match code {
        AUDIT_SAMPLE_COLLECTED => "sample_collected",
        AUDIT_COURIER_PICKUP => "courier_pickup",
        AUDIT_FREEZER_POWER_BLIP => "freezer_power_blip",
        AUDIT_LAB_RECEIPT => "lab_receipt",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_chain_scenario_converges_after_heal() {
        let report = run_scenario();

        assert!(report.cross_partition_blocked);
        assert!(!report.converged_during_partition);
        assert!(report.converged_after_heal);
        assert_eq!(
            report.final_projection,
            Projection {
                samples: vec![SAMPLE_ID],
                active_holders: vec!["lab"],
                active_alerts: vec!["temperature_excursion"],
                audit_codes: vec![
                    "sample_collected",
                    "courier_pickup",
                    "freezer_power_blip",
                    "lab_receipt",
                ],
                event_count: 4,
            }
        );
    }
}
