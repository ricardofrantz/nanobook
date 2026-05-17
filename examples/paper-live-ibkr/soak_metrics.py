#!/usr/bin/env python3
"""Compute paper-soak release numbers from sanitized audit JSONL.

This is the machine-readable companion to SOAK_STATUS.md and
`docs/ops/v0.15-release-evidence-checklist.md`. It intentionally uses only
sanitized audit events, so README/release numbers can be reproduced offline.
"""

from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

INCIDENT_TOKENS = ("incident", "error", "failed", "reconnect", "stale", "kill", "mismatch")
ORDER_EVENTS = {"order_submitted", "order_intent", "order_submit_result"}
FILL_EVENTS = {"order_filled", "fill", "order_fill"}
RUN_EVENTS = {"run_completed", "cron_completed"}


def load_events(path: Path) -> list[dict[str, Any]]:
    events: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_no, line in enumerate(handle, 1):
            if not line.strip():
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"invalid JSONL at line {line_no}: {exc}") from exc
            if not isinstance(event, dict):
                raise SystemExit(f"invalid JSONL at line {line_no}: expected object")
            events.append(event)
    return events


def summarize(events: list[dict[str, Any]]) -> dict[str, Any]:
    counts: Counter[str] = Counter()
    days: set[str] = set()
    incident_events: list[dict[str, Any]] = []
    manual_interventions = 0

    for event in events:
        name = str(event.get("event", ""))
        name_lower = name.lower()
        timestamp = str(event.get("ts", ""))
        if timestamp:
            days.add(timestamp[:10])

        if name in RUN_EVENTS:
            counts["rebalances"] += 1
        if name in ORDER_EVENTS:
            counts["orders"] += 1
        if name in FILL_EVENTS:
            counts["fills"] += 1
        if "cancel" in name_lower:
            counts["cancels"] += 1
        if "reconcile" in name_lower:
            counts["reconciles"] += 1
            mismatch_count = event.get("mismatch_count")
            if isinstance(mismatch_count, int):
                counts["reconcile_mismatches"] += mismatch_count
            elif event.get("mismatches"):
                counts["reconcile_mismatches"] += len(event.get("mismatches", []))

        level = str(event.get("level", "")).lower()
        if level in {"warn", "warning", "error"} or any(token in name_lower for token in INCIDENT_TOKENS):
            counts["incidents"] += 1
            incident_events.append(event)
            recovery = str(event.get("recovery", event.get("resolution", ""))).lower()
            if "manual" in recovery:
                manual_interventions += 1

    sorted_days = sorted(day for day in days if day and day != "None")
    return {
        "calendar_days": len(sorted_days),
        "first_day": sorted_days[0] if sorted_days else None,
        "last_day": sorted_days[-1] if sorted_days else None,
        "rebalances": counts["rebalances"],
        "orders": counts["orders"],
        "fills": counts["fills"],
        "cancels": counts["cancels"],
        "reconciles": counts["reconciles"],
        "reconcile_mismatches": counts["reconcile_mismatches"],
        "incidents": counts["incidents"],
        "manual_interventions": manual_interventions,
        "incident_events": [
            {
                "ts": event.get("ts"),
                "event": event.get("event"),
                "message": event.get("message") or event.get("error") or event.get("summary"),
            }
            for event in incident_events
        ],
    }


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("Usage: python soak_metrics.py <sanitized-audit.jsonl>", file=sys.stderr)
        return 2
    summary = summarize(load_events(Path(argv[1])))
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
