#!/usr/bin/env python3

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
import sys
from typing import Protocol

import safemesh_python as sm


SAMPLE_ID = 9001

CLINIC = 100
COURIER = 200
LAB = 300
UNKNOWN = 0

TEMP_EXCURSION = 7001

AUDIT_TRAIL = [
    "clinic_collected_vaccine",
    "courier_collected_offline",
    "freezer_power_blip_recorded",
    "lab_receipt_signed",
]


class LogReplica(Protocol):
    def log_bytes(self) -> bytes: ...

    def merge_log_bytes(self, payload: bytes) -> list[str]: ...


@dataclass(frozen=True)
class Report:
    converged: bool
    holders: list[int]
    holder_names: list[str]
    audit_counts: list[int]
    temperature_alerts: list[bool]
    holder_digest: str
    audit_digest: str
    alert_digest: str


def sync_logs(replicas: list[LogReplica]) -> None:
    payloads = [replica.log_bytes() for replica in replicas]
    for replica in replicas:
        for payload in payloads:
            require_collision_free(replica, payload)


def require_collision_free(replica: LogReplica, payload: bytes) -> None:
    admissions = replica.merge_log_bytes(payload)
    if "collision" in admissions:
        raise RuntimeError(f"batch collision after admissions={admissions!r}")


def log_digest(replicas: list[LogReplica]) -> str:
    digest = sha256()
    for replica in replicas:
        digest.update(replica.log_bytes())
    return digest.hexdigest()[:16]


def holder_name(holder: int) -> str:
    return {
        CLINIC: "clinic",
        COURIER: "courier",
        LAB: "lab",
        UNKNOWN: "unknown",
    }.get(holder, f"unknown:{holder}")


def bools(values: list[bool]) -> str:
    return "[" + ", ".join(str(value).lower() for value in values) + "]"


def run_scenario() -> Report:
    holder = [
        sm.LwwMapReplica(1),
        sm.LwwMapReplica(2),
        sm.LwwMapReplica(3),
    ]
    audit = [
        sm.GCounterReplica(0, 3),
        sm.GCounterReplica(1, 3),
        sm.GCounterReplica(2, 3),
    ]
    alert = [
        sm.EnableWinsFlagReplica(1),
        sm.EnableWinsFlagReplica(2),
        sm.EnableWinsFlagReplica(3),
    ]

    # Clinic starts the custody record before the courier has network.
    holder[0].append_set(SAMPLE_ID, 10, 1, CLINIC)
    audit[0].append_bump(0, 1)

    # The courier receives the clinic log, then moves while offline.
    require_collision_free(holder[1], holder[0].log_bytes())
    require_collision_free(audit[1], audit[0].log_bytes())
    holder[1].append_set(SAMPLE_ID, 20, 2, COURIER)
    audit[1].append_bump(1, 1)

    # The freezer blip is a recorded alert. The demo does not prove the sensor reading.
    alert[1].append_enable(TEMP_EXCURSION)
    audit[1].append_bump(1, 2)

    sync_logs(holder[:2])
    sync_logs(audit[:2])
    sync_logs(alert[:2])

    # The lab was dark during the handoff. The data mule reconnects it later.
    sync_logs(holder)
    sync_logs(audit)
    sync_logs(alert)

    holder[2].append_set(SAMPLE_ID, 30, 3, LAB)
    audit[2].append_bump(2, 1)

    sync_logs(holder)
    sync_logs(audit)
    sync_logs(alert)

    holders = [replica.value_or(SAMPLE_ID, UNKNOWN) for replica in holder]
    audit_counts = [replica.value() for replica in audit]
    temperature_alerts = [replica.value() for replica in alert]
    converged = (
        holders == [LAB, LAB, LAB]
        and audit_counts == [4, 4, 4]
        and temperature_alerts == [True, True, True]
    )

    return Report(
        converged=converged,
        holders=holders,
        holder_names=[holder_name(holder) for holder in holders],
        audit_counts=audit_counts,
        temperature_alerts=temperature_alerts,
        holder_digest=log_digest(holder),
        audit_digest=log_digest(audit),
        alert_digest=log_digest(alert),
    )


def print_report(report: Report) -> None:
    print("SafeMesh for builders / Python data mule")
    print("Case: vaccine shipment 9001 crosses a clinic, courier, and lab while links fail.")
    print(
        "Honest boundary: this shows modeled custody convergence and matching log bytes; "
        "it does not prove sensor truth, real transport delivery, or durable storage.\n"
    )

    print("[1/5] Clinic seals the shipment")
    print("  custody=clinic audit=clinic_collected_vaccine")

    print("\n[2/5] Courier collects offline")
    print("  custody=courier link=clinic<->courier lab_link=offline")

    print("\n[3/5] Freezer power blip is recorded")
    print("  temperature_alert=true sensor_truth=outside_safemesh_claim")

    print("\n[4/5] Data mule reaches the lab")
    print("  sync=log_bytes merge=Rust_core thin_binding=Python")

    print("\n[5/5] Lab receives and everyone syncs")
    print(f"  holders={report.holder_names}")
    print(f"  audit_trail={AUDIT_TRAIL}")
    print(f"  audit_counts={report.audit_counts}")
    print(f"  temperature_alerts={bools(report.temperature_alerts)}")
    print(
        "  evidence_digests="
        f"holder:{report.holder_digest} "
        f"audit:{report.audit_digest} "
        f"alert:{report.alert_digest}"
    )
    print(
        "  CONVERGED="
        f"{str(report.converged).lower()} "
        f"python_data_mule_sample={SAMPLE_ID} "
        f"holders={report.holders} "
        f"audit_counts={report.audit_counts} "
        f"temperature_alerts={report.temperature_alerts}"
    )


def main() -> int:
    report = run_scenario()
    print_report(report)
    return 0 if report.converged else 1


if __name__ == "__main__":
    sys.exit(main())
