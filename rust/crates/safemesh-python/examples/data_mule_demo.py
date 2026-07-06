#!/usr/bin/env python3

from __future__ import annotations

import sys

import safemesh_python as sm


SAMPLE_ID = 9001
CLINIC = 100
COURIER = 200
LAB = 300
TEMP_EXCURSION = 7001


def sync_logs(replicas: list[object]) -> None:
    payloads = [replica.log_bytes() for replica in replicas]
    for replica in replicas:
        for payload in payloads:
            replica.merge_log_bytes(payload)


def main() -> int:
    holder = [
        sm.LwwMapReplica(1),
        sm.LwwMapReplica(2),
        sm.LwwMapReplica(3),
    ]
    audit = [
        sm.GCounterReplica(1, 3),
        sm.GCounterReplica(2, 3),
        sm.GCounterReplica(3, 3),
    ]
    alert = [
        sm.EnableWinsFlagReplica(1),
        sm.EnableWinsFlagReplica(2),
        sm.EnableWinsFlagReplica(3),
    ]

    holder[0].append_set(SAMPLE_ID, 10, 1, CLINIC)
    audit[0].append_bump(0, 1)

    holder[1].merge_log_bytes(holder[0].log_bytes())
    audit[1].merge_log_bytes(audit[0].log_bytes())
    holder[1].append_set(SAMPLE_ID, 20, 2, COURIER)
    audit[1].append_bump(1, 1)
    alert[1].append_enable(TEMP_EXCURSION)

    sync_logs(holder[:2])
    sync_logs(audit[:2])
    sync_logs(alert[:2])

    sync_logs(holder)
    sync_logs(audit)
    sync_logs(alert)

    holder[2].append_set(SAMPLE_ID, 30, 3, LAB)
    audit[2].append_bump(2, 1)

    sync_logs(holder)
    sync_logs(audit)
    sync_logs(alert)

    holders = [replica.value_or(SAMPLE_ID, 0) for replica in holder]
    audit_counts = [replica.value() for replica in audit]
    alerts = [replica.value() for replica in alert]

    converged = holders == [LAB, LAB, LAB] and audit_counts == [3, 3, 3] and alerts == [
        True,
        True,
        True,
    ]

    print(
        "CONVERGED="
        f"{str(converged).lower()} "
        f"python_data_mule_sample={SAMPLE_ID} "
        f"holders={holders} "
        f"audit_counts={audit_counts} "
        f"temperature_alerts={alerts}"
    )

    return 0 if converged else 1


if __name__ == "__main__":
    sys.exit(main())
