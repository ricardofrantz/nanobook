#!/usr/bin/env python3
"""Generate a reconstructable HTML report for the paper-live IBKR soak.

The report is intentionally derived from sanitized audit JSONL excerpts only.
It accepts old and new audit event shapes and ignores unknown fields so the
artifact can be regenerated after sensitive account/order details are scrubbed.
"""

from __future__ import annotations

import html
import json
import sys
from collections import defaultdict
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

CENTS = 100.0


def parse_ts(value: str | None) -> datetime | None:
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(UTC)
    except ValueError:
        return None


def event_day(event: dict[str, Any]) -> str:
    ts = parse_ts(event.get("ts"))
    return ts.date().isoformat() if ts else "unknown"


def money(cents: int | float | None) -> float | None:
    if cents is None:
        return None
    return float(cents) / CENTS


def read_audit_log(path: str | Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with Path(path).open("r", encoding="utf-8") as handle:
        for line_no, line in enumerate(handle, 1):
            if not line.strip():
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(f"invalid JSONL at line {line_no}: {exc}") from exc
            if not isinstance(row, dict):
                raise ValueError(f"invalid JSONL at line {line_no}: expected object")
            rows.append(row)
    return rows


def order_id(event: dict[str, Any]) -> str | None:
    value = event.get("ibkr_id") or event.get("order_id") or event.get("id")
    return str(value) if value is not None else None


def extract_positions(event: dict[str, Any]) -> list[dict[str, Any]]:
    data = event.get("positions") or event.get("data", {}).get("positions")
    return data if isinstance(data, list) else []


def extract_metrics(audit_data: list[dict[str, Any]], config: dict[str, Any]) -> dict[str, Any]:
    events = sorted(audit_data, key=lambda row: row.get("ts", ""))
    days = [day for event in events if (day := event_day(event)) != "unknown"]
    start_date = config.get("start_date") or (days[0] if days else "unknown")
    end_date = config.get("end_date") or (days[-1] if days else "unknown")

    equity_by_day: dict[str, float] = {}
    positions_by_day: dict[str, dict[str, float]] = defaultdict(dict)
    incidents: list[dict[str, str]] = []
    reconciles: list[dict[str, str]] = []
    submitted: dict[str, dict[str, Any]] = {}
    filled: dict[str, dict[str, Any]] = {}

    account = config.get("account", "sanitized")
    for event in events:
        name = str(event.get("event", ""))
        day = event_day(event)
        if event.get("account_id"):
            account = event["account_id"]

        equity_cents = (
            event.get("equity_cents")
            or event.get("net_liquidation_cents")
            or event.get("account", {}).get("equity_cents")
        )
        if equity_cents is not None:
            equity_by_day[day] = money(equity_cents) or 0.0

        for pos in extract_positions(event):
            symbol = str(pos.get("symbol", "unknown"))
            qty = pos.get("quantity", pos.get("qty", pos.get("shares", 0)))
            try:
                positions_by_day[day][symbol] = float(qty)
            except (TypeError, ValueError):
                positions_by_day[day][symbol] = 0.0

        oid = order_id(event)
        if name in {"order_submitted", "order_intent", "order_submit_result"} and oid:
            submitted[oid] = event
        if name in {"order_filled", "fill", "order_fill"} and oid:
            filled[oid] = event

        mismatch_count = event.get("mismatch_count") or len(event.get("mismatches", []))
        if "reconcile" in name and mismatch_count:
            reconciles.append(
                {
                    "when": event.get("ts", "unknown"),
                    "summary": str(event.get("summary") or f"{mismatch_count} mismatch(es)"),
                    "details": json.dumps(event.get("mismatches", event), sort_keys=True),
                }
            )

        if is_incident(event):
            incidents.append(
                {
                    "when": event.get("ts", "unknown"),
                    "type": classify_incident(event),
                    "description": str(event.get("message") or event.get("error") or name),
                    "recovery": str(event.get("recovery") or event.get("resolution") or "see audit trail"),
                }
            )

    slippage_rows = []
    for oid, sub in submitted.items():
        fill = filled.get(oid)
        if not fill:
            continue
        expected = sub.get("limit_price_cents") or sub.get("limit") or sub.get("expected_price_cents")
        actual = fill.get("avg_price_cents") or fill.get("avg_price") or fill.get("fill_price_cents")
        expected_usd = money(expected) if isinstance(expected, int) else float(expected or 0)
        actual_usd = money(actual) if isinstance(actual, int) else float(actual or 0)
        slippage_rows.append(
            {
                "order_id": oid,
                "symbol": sub.get("symbol", fill.get("symbol", "unknown")),
                "side": sub.get("action", sub.get("side", "unknown")),
                "expected": expected_usd,
                "actual": actual_usd,
                "slippage": actual_usd - expected_usd,
            }
        )

    daily_equity = [{"day": day, "equity": equity} for day, equity in sorted(equity_by_day.items())]
    if not daily_equity and config.get("daily_equity"):
        daily_equity = [
            {"day": f"d{i + 1}", "equity": float(value)}
            for i, value in enumerate(config["daily_equity"])
        ]

    first_equity = daily_equity[0]["equity"] if daily_equity else 0.0
    last_equity = daily_equity[-1]["equity"] if daily_equity else first_equity
    return_pct = ((last_equity / first_equity) - 1.0) * 100.0 if first_equity else 0.0
    max_drawdown_pct = compute_max_drawdown([row["equity"] for row in daily_equity])

    return {
        "start_date": start_date,
        "end_date": end_date,
        "duration_days": len({row["day"] for row in daily_equity}) or config.get("duration_days", 0),
        "account": account,
        "universe": config.get("universe", "sanitized universe"),
        "orders_sent": len(submitted),
        "orders_filled": len(filled),
        "reconcile_count": len(reconciles),
        "incidents": len(incidents),
        "auto_recovered": sum(1 for incident in incidents if incident["recovery"] != "manual"),
        "daily_equity": daily_equity,
        "positions_by_day": dict(sorted(positions_by_day.items())),
        "reconciles": reconciles,
        "slippage_rows": slippage_rows,
        "incident_log": incidents,
        "return_pct": return_pct,
        "max_drawdown_pct": max_drawdown_pct,
    }


def compute_max_drawdown(values: list[float]) -> float:
    peak = None
    max_drawdown = 0.0
    for value in values:
        peak = value if peak is None else max(peak, value)
        if peak:
            max_drawdown = min(max_drawdown, (value / peak - 1.0) * 100.0)
    return max_drawdown


def is_incident(event: dict[str, Any]) -> bool:
    name = str(event.get("event", "")).lower()
    level = str(event.get("level", "")).lower()
    return (
        level in {"warn", "warning", "error"}
        or any(token in name for token in ["incident", "error", "failed", "reconnect", "kill", "stale", "mismatch"])
    )


def classify_incident(event: dict[str, Any]) -> str:
    text = f"{event.get('event', '')} {event.get('message', '')} {event.get('error', '')}".lower()
    for token in ["reconnect", "cancel", "stale", "partial", "clock", "kill", "mismatch"]:
        if token in text:
            return token
    if "error" in text or "failed" in text:
        return "error"
    return "incident"


def rows_html(rows: list[str]) -> str:
    return "\n".join(rows) if rows else '<tr><td colspan="99">None observed in sanitized audit excerpt.</td></tr>'


def esc(value: Any) -> str:
    return html.escape(str(value))


def render_html(metrics: dict[str, Any], config: dict[str, Any]) -> str:
    equity_labels = [row["day"] for row in metrics["daily_equity"]]
    equity_values = [round(row["equity"], 2) for row in metrics["daily_equity"]]
    position_rows = []
    for day, positions in metrics["positions_by_day"].items():
        position_rows.append(
            f"<tr><td>{esc(day)}</td><td>{esc(', '.join(f'{sym}: {qty:g}' for sym, qty in sorted(positions.items())))}</td></tr>"
        )
    reconcile_rows = [
        f"<tr><td>{esc(row['when'])}</td><td>{esc(row['summary'])}</td><td><code>{esc(row['details'])}</code></td></tr>"
        for row in metrics["reconciles"]
    ]
    slippage_rows = [
        f"<tr><td>{esc(row['order_id'])}</td><td>{esc(row['symbol'])}</td><td>{esc(row['side'])}</td><td class='num'>{row['expected']:.4f}</td><td class='num'>{row['actual']:.4f}</td><td class='num'>{row['slippage']:+.4f}</td></tr>"
        for row in metrics["slippage_rows"]
    ]
    incident_rows = [
        f"<tr><td>{esc(row['when'])}</td><td><span class='tag'>{esc(row['type'])}</span> {esc(row['description'])}</td><td>{esc(row['recovery'])}</td></tr>"
        for row in metrics["incident_log"]
    ]

    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>nanobook paper-live soak report</title>
<style>
body{{margin:0;background:#f8f8f6;color:#1a1a1a;font-family:-apple-system,BlinkMacSystemFont,"Helvetica Neue",Arial,sans-serif;line-height:1.5}}main{{width:min(1040px,calc(100% - 40px));margin:0 auto;padding:36px 0 56px}}h1{{font-size:36px;line-height:1.1;margin:8px 0}}h2{{font-size:19px;margin:34px 0 8px}}.meta,.caption{{color:#666}}.grid{{display:grid;grid-template-columns:repeat(4,1fr);border:1px solid #ddd;background:#fff;margin:24px 0}}.cell{{padding:16px;border-right:1px solid #ddd}}.cell:last-child{{border-right:0}}.num{{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-variant-numeric:tabular-nums}}.big{{font-size:24px;font-weight:700}}table{{border-collapse:collapse;width:100%;background:#fff;margin-top:10px}}th,td{{border:1px solid #ddd;padding:8px 10px;vertical-align:top;text-align:left}}th{{background:#f1f1ef;font-size:12px;text-transform:uppercase;letter-spacing:.04em}}code{{font-size:12px;white-space:pre-wrap}}.tag{{font-size:11px;text-transform:uppercase;border:1px solid currentColor;padding:2px 5px;margin-right:5px}}#chart{{height:260px;border:1px solid #ddd;background:#fff;padding:12px}}.bar{{height:18px;background:#4a6fa5;margin:6px 0}}@media(max-width:760px){{.grid{{grid-template-columns:1fr 1fr}}}}
</style>
</head>
<body><main>
<p class="meta">Reconstructable from sanitized audit JSONL · {esc(config.get('label', 'v0.15 paper-live soak'))}</p>
<h1>Paper-live soak report</h1>
<p class="caption">{esc(metrics['start_date'])} to {esc(metrics['end_date'])} · account {esc(metrics['account'])} · {esc(metrics['universe'])}</p>
<div class="grid">
  <div class="cell"><div class="big num">{metrics['duration_days']}</div><div class="meta">equity days</div></div>
  <div class="cell"><div class="big num">{metrics['orders_sent']} / {metrics['orders_filled']}</div><div class="meta">orders / fills</div></div>
  <div class="cell"><div class="big num">{metrics['reconcile_count']}</div><div class="meta">reconcile mismatches</div></div>
  <div class="cell"><div class="big num">{metrics['incidents']}</div><div class="meta">incidents</div></div>
</div>
<h2>Daily equity curve</h2><p class="caption">Return {metrics['return_pct']:+.2f}%; max drawdown {metrics['max_drawdown_pct']:+.2f}%.</p>
<div id="chart"></div>
<h2>Position evolution</h2><table><thead><tr><th>Day</th><th>Positions</th></tr></thead><tbody>{rows_html(position_rows)}</tbody></table>
<h2>Reconcile mismatches</h2><table><thead><tr><th>When</th><th>Summary</th><th>Details</th></tr></thead><tbody>{rows_html(reconcile_rows)}</tbody></table>
<h2>Realized vs expected slippage</h2><table><thead><tr><th>Order</th><th>Symbol</th><th>Side</th><th>Expected</th><th>Actual</th><th>Slippage</th></tr></thead><tbody>{rows_html(slippage_rows)}</tbody></table>
<h2>Operational incidents</h2><table><thead><tr><th>When</th><th>What</th><th>Recovery</th></tr></thead><tbody>{rows_html(incident_rows)}</tbody></table>
<script>
const labels = {json.dumps(equity_labels)};
const values = {json.dumps(equity_values)};
const max = Math.max(...values, 1);
document.getElementById('chart').innerHTML = values.map((v,i)=>`<div class="num">${{labels[i]}} $${{v.toFixed(2)}}</div><div class="bar" style="width:${{Math.max(2, v / max * 100)}}%"></div>`).join('');
</script>
</main></body></html>"""


def generate_report(audit_log_path: str, output_path: str, config: dict[str, Any]) -> None:
    audit_data = read_audit_log(audit_log_path)
    metrics = extract_metrics(audit_data, config)
    Path(output_path).write_text(render_html(metrics, config), encoding="utf-8")
    print(f"Report generated: {output_path}")


def load_config(argv: list[str]) -> dict[str, Any]:
    config: dict[str, Any] = {}
    if "--config" in argv:
        index = argv.index("--config")
        if index + 1 >= len(argv):
            raise SystemExit("--config requires a JSON file")
        config.update(json.loads(Path(argv[index + 1]).read_text(encoding="utf-8")))
    return config


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python report.py <audit_log.jsonl> <output.html> [--config config.json]")
        raise SystemExit(1)
    generate_report(sys.argv[1], sys.argv[2], load_config(sys.argv))
