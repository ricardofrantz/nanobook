#!/usr/bin/env python3
from __future__ import annotations

import argparse
import html
import json
import math
from collections import Counter
from pathlib import Path
from typing import Any

import nanobook


ROOT = Path(__file__).resolve().parent


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build the v0.11 ITCH replay report skeleton.")
    parser.add_argument(
        "--input",
        type=Path,
        default=ROOT / "data" / "replay" / "event-log.jsonl",
        help="Replay event-log.jsonl path.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "data" / "replay" / "report.html",
        help="Output report HTML path.",
    )
    parser.add_argument("--limit", type=int, default=None, help="Optional max events to load.")
    return parser.parse_args()


def load_events(path: Path, limit: int | None = None) -> list[dict[str, Any]]:
    events: list[dict[str, Any]] = []
    with path.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            event = json.loads(line)
            if not isinstance(event, dict):
                raise ValueError(f"{path}:{line_number}: expected JSON object")
            events.append(event)
            if limit is not None and len(events) >= limit:
                break
    return events


def summarize(events: list[dict[str, Any]]) -> dict[str, Any]:
    event_counts = Counter(str(event.get("type", "unknown")) for event in events)
    timestamps = [event["timestamp"] for event in events if isinstance(event.get("timestamp"), int)]
    symbols = {str(event["stock"]) for event in events if event.get("stock")}
    total_volume = sum(int(event.get("qty", event.get("shares", 0))) for event in events)
    return {
        "events": len(events),
        "event_counts": event_counts,
        "symbols": len(symbols),
        "total_volume": total_volume,
        "first_timestamp": min(timestamps) if timestamps else "none",
        "last_timestamp": max(timestamps) if timestamps else "none",
        "nanobook_capabilities": len(nanobook.capabilities()),
    }


def latency_data_for_histogram(events: list[dict[str, Any]]) -> dict[str, Any]:
    # `isinstance(value, int)` skips JSON nulls written by the replay harness
    # for warmup events (`--warmup N`) and for non-cross-trade rows that
    # carry no book update. Bools count as int in Python but the harness
    # never emits true/false here.
    parse_latencies: list[int] = []
    book_latencies: list[int] = []
    for event in events:
        parse_value = event.get("parse_latency_ns")
        if isinstance(parse_value, int) and not isinstance(parse_value, bool):
            parse_latencies.append(parse_value)
        book_value = event.get("book_update_latency_ns")
        if isinstance(book_value, int) and not isinstance(book_value, bool):
            book_latencies.append(book_value)

    def percentile(values: list[int], p: float) -> int:
        if not values:
            return 0
        sorted_values = sorted(values)
        idx = int(len(sorted_values) * p / 100)
        return sorted_values[min(idx, len(sorted_values) - 1)]

    return {
        "parse": sorted(parse_latencies),
        "book": sorted(book_latencies),
        "parse_count": len(parse_latencies),
        "book_count": len(book_latencies),
        "total_events": len(events),
        "parse_p50": percentile(parse_latencies, 50),
        "parse_p95": percentile(parse_latencies, 95),
        "parse_p99": percentile(parse_latencies, 99),
        "book_p50": percentile(book_latencies, 50),
        "book_p95": percentile(book_latencies, 95),
        "book_p99": percentile(book_latencies, 99),
    }


def svg_latency_histogram(latency_data: dict[str, Any], title: str) -> str:
    safe_title = html.escape(title)
    parse_latencies = latency_data["parse"]
    book_latencies = latency_data["book"]

    if not parse_latencies and not book_latencies:
        return placeholder(title)
    
    width = 920
    height = 320
    pad_left = 54
    pad_right = 18
    pad_top = 22
    pad_bottom = 42
    
    # Create buckets for histogram (log scale for latency)
    def bucket_values(values: list[int]) -> list[tuple[int, int]]:
        if not values:
            return []
        buckets: Counter[int] = Counter()
        for v in values:
            # Bucket by powers of 10 (1ns, 10ns, 100ns, 1us, 10us, etc.)
            exp = max(0, int(math.log10(max(1, v))))
            buckets[10 ** exp] += 1
        return sorted(buckets.items())
    
    parse_buckets = bucket_values(parse_latencies)
    book_buckets = bucket_values(book_latencies)
    
    max_count = max(
        [count for _, count in parse_buckets] + [count for _, count in book_buckets],
        default=1
    )
    
    max_latency = max(
        [lat for lat, _ in parse_buckets] + [lat for lat, _ in book_buckets],
        default=1000
    )
    
    span_y = max(1, max_count)
    span_x = max(1, max_latency)
    
    def x_pos(latency: int) -> float:
        return pad_left + math.log10(max(1, latency)) / math.log10(span_x) * (width - pad_left - pad_right)
    
    def bar_height(count: int) -> float:
        return count / span_y * (height - pad_top - pad_bottom)
    
    # Draw parse latency bars (blue)
    parse_bars = []
    for latency, count in parse_buckets:
        x = x_pos(latency)
        h = bar_height(count)
        y = height - pad_bottom - h
        bar_w = 8.0
        parse_bars.append(
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_w:.1f}" height="{h:.1f}" fill="#4a6fa5" opacity="0.78"/>'
        )
    
    # Draw book latency bars (green)
    book_bars = []
    for latency, count in book_buckets:
        x = x_pos(latency)
        h = bar_height(count)
        y = height - pad_bottom - h
        bar_w = 8.0
        book_bars.append(
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_w:.1f}" height="{h:.1f}" fill="#2a7f5f" opacity="0.78"/>'
        )
    
    # Format latency numbers nicely
    def format_ns(ns: int) -> str:
        if ns >= 1_000_000:
            return f"{ns / 1_000_000:.1f}µs"
        elif ns >= 1_000:
            return f"{ns / 1_000:.1f}ns"
        else:
            return f"{ns}ns"
    
    parse_count = latency_data.get("parse_count", len(parse_latencies))
    book_count = latency_data.get("book_count", len(book_latencies))
    total_events = latency_data.get("total_events", 0)
    excluded_note = ""
    if total_events and parse_count < total_events:
        excluded = total_events - parse_count
        excluded_note = f" — {excluded} event(s) excluded from pool (warmup or no-op)"

    return f"""
      <section>
        <h2>{safe_title}</h2>
        <p class="caption">
          Parse (blue): p50={format_ns(latency_data["parse_p50"])}, p95={format_ns(latency_data["parse_p95"])}, p99={format_ns(latency_data["parse_p99"])} (N={parse_count})<br>
          Book update (green): p50={format_ns(latency_data["book_p50"])}, p95={format_ns(latency_data["book_p95"])}, p99={format_ns(latency_data["book_p99"])} (N={book_count}){excluded_note}
        </p>
        <svg class="plot-svg" viewBox="0 0 {width} {height}" role="img" aria-label="{safe_title}">
          <rect x="0" y="0" width="{width}" height="{height}" fill="#fff" stroke="#ddd"/>
          <line x1="{pad_left}" y1="{height - pad_bottom}" x2="{width - pad_right}" y2="{height - pad_bottom}" stroke="#aaa"/>
          <line x1="{pad_left}" y1="{pad_top}" x2="{pad_left}" y2="{height - pad_bottom}" stroke="#aaa"/>
          {"".join(parse_bars)}
          {"".join(book_bars)}
          <text x="{pad_left}" y="{height - 14}" font-size="12" fill="#666">1ns</text>
          <text x="{width - 72}" y="{height - 14}" font-size="12" fill="#666">{format_ns(max_latency)}</text>
          <text x="10" y="{pad_top + 6}" font-size="12" fill="#666">{max_count}</text>
        </svg>
      </section>
    """


def message_rate(events: list[dict[str, Any]]) -> list[tuple[int, int]]:
    buckets: Counter[int] = Counter()
    for event in events:
        timestamp = event.get("timestamp")
        if isinstance(timestamp, int):
            buckets[timestamp // 1_000_000_000] += 1
    return sorted(buckets.items())


def spread_distribution(events: list[dict[str, Any]]) -> list[tuple[int, int]]:
    buckets: Counter[int] = Counter()
    for event in events:
        spread = event.get("spread")
        if isinstance(spread, int) and spread >= 0:
            buckets[spread] += 1
    return sorted(buckets.items())


def latency_values(events: list[dict[str, Any]], field: str) -> list[int]:
    # Skip JSON nulls (warmup or non-applicable rows) AND `bool` values,
    # since `isinstance(True, int)` would otherwise treat them as ints.
    return [
        value
        for event in events
        if isinstance((value := event.get(field)), int) and not isinstance(value, bool)
    ]


def percentile(values: list[int], pct: float) -> int:
    if not values:
        return 0
    ordered = sorted(values)
    index = round((len(ordered) - 1) * pct)
    return ordered[index]


def latency_summary(events: list[dict[str, Any]]) -> list[tuple[str, list[int]]]:
    return [
        ("ITCH parse", latency_values(events, "parse_latency_ns")),
        ("Book update", latency_values(events, "book_update_latency_ns")),
        ("Strategy to order", latency_values(events, "strategy_to_order_latency_ns")),
    ]


def latency_distribution(events: list[dict[str, Any]]) -> list[tuple[int, int]]:
    buckets: Counter[int] = Counter()
    for _, values in latency_summary(events):
        for value in values:
            bucket = min(10_000, (value // 100) * 100)
            buckets[bucket] += 1
    return sorted(buckets.items())


def svg_line_chart(points: list[tuple[int, int]], title: str) -> str:
    safe_title = html.escape(title)
    if not points:
        return placeholder(title)
    width = 920
    height = 320
    pad_left = 54
    pad_right = 18
    pad_top = 22
    pad_bottom = 42
    min_x = points[0][0]
    max_x = points[-1][0]
    max_y = max(count for _, count in points)
    span_x = max(1, max_x - min_x)
    span_y = max(1, max_y)

    def xy(point: tuple[int, int]) -> tuple[float, float]:
        second, count = point
        x = pad_left + (second - min_x) / span_x * (width - pad_left - pad_right)
        y = height - pad_bottom - count / span_y * (height - pad_top - pad_bottom)
        return x, y

    polyline = " ".join(f"{x:.1f},{y:.1f}" for x, y in map(xy, points))
    last_second = max_x - min_x
    return f"""
      <section>
        <h2>{safe_title}</h2>
        <p class="caption">Events per second across the loaded replay window.</p>
        <svg class="plot-svg" viewBox="0 0 {width} {height}" role="img" aria-label="{safe_title}">
          <rect x="0" y="0" width="{width}" height="{height}" fill="#fff" stroke="#ddd"/>
          <line x1="{pad_left}" y1="{height - pad_bottom}" x2="{width - pad_right}" y2="{height - pad_bottom}" stroke="#aaa"/>
          <line x1="{pad_left}" y1="{pad_top}" x2="{pad_left}" y2="{height - pad_bottom}" stroke="#aaa"/>
          <polyline points="{polyline}" fill="none" stroke="#4a6fa5" stroke-width="2.4"/>
          <text x="{pad_left}" y="{height - 14}" font-size="12" fill="#666">0s</text>
          <text x="{width - 72}" y="{height - 14}" font-size="12" fill="#666">{last_second}s</text>
          <text x="10" y="{pad_top + 6}" font-size="12" fill="#666">{max_y}/s</text>
        </svg>
      </section>
    """


def svg_bar_chart(points: list[tuple[int, int]], title: str, x_label: str) -> str:
    safe_title = html.escape(title)
    if not points:
        return placeholder(title)
    width = 920
    height = 320
    pad_left = 54
    pad_right = 18
    pad_top = 22
    pad_bottom = 42
    max_y = max(count for _, count in points)
    bar_area = width - pad_left - pad_right
    bar_width = max(2.0, bar_area / len(points) * 0.72)
    step = bar_area / max(1, len(points))
    span_y = max(1, max_y)
    bars = []
    for idx, (_, count) in enumerate(points):
        x = pad_left + idx * step + (step - bar_width) / 2
        bar_height = count / span_y * (height - pad_top - pad_bottom)
        y = height - pad_bottom - bar_height
        bars.append(
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_width:.1f}" height="{bar_height:.1f}" fill="#2a7f5f" opacity="0.78"/>'
        )
    first_label = html.escape(str(points[0][0]))
    last_label = html.escape(str(points[-1][0]))
    return f"""
      <section>
        <h2>{safe_title}</h2>
        <p class="caption">{html.escape(x_label)} histogram across book-changing events.</p>
        <svg class="plot-svg" viewBox="0 0 {width} {height}" role="img" aria-label="{safe_title}">
          <rect x="0" y="0" width="{width}" height="{height}" fill="#fff" stroke="#ddd"/>
          <line x1="{pad_left}" y1="{height - pad_bottom}" x2="{width - pad_right}" y2="{height - pad_bottom}" stroke="#aaa"/>
          <line x1="{pad_left}" y1="{pad_top}" x2="{pad_left}" y2="{height - pad_bottom}" stroke="#aaa"/>
          {"".join(bars)}
          <text x="{pad_left}" y="{height - 14}" font-size="12" fill="#666">{first_label}</text>
          <text x="{width - 72}" y="{height - 14}" font-size="12" fill="#666">{last_label}</text>
          <text x="10" y="{pad_top + 6}" font-size="12" fill="#666">{max_y}</text>
        </svg>
      </section>
    """


def render_latency(events: list[dict[str, Any]]) -> str:
    latency_data = latency_data_for_histogram(events)
    return svg_latency_histogram(latency_data, "Latency distribution")


def book_snapshots(events: list[dict[str, Any]], count: int = 3) -> list[dict[str, Any]]:
    candidates = [
        event
        for event in events
        if isinstance(event.get("book"), dict)
        and event["book"].get("bids")
        and event["book"].get("asks")
    ]
    if len(candidates) <= count:
        return candidates
    step = (len(candidates) - 1) / max(1, count - 1)
    return [candidates[round(i * step)] for i in range(count)]


def render_side(levels: list[dict[str, Any]], side: str) -> str:
    rows = "\n".join(
        "<tr>"
        f"<td>{idx}</td>"
        f"<td>{int(level['price']) / 100:.2f}</td>"
        f"<td>{int(level['shares'])}</td>"
        f"<td>{int(level['orders'])}</td>"
        "</tr>"
        for idx, level in enumerate(levels, start=1)
    )
    return f"""
      <table>
        <thead><tr><th>{html.escape(side)} level</th><th>Price</th><th>Shares</th><th>Orders</th></tr></thead>
        <tbody>{rows}</tbody>
      </table>
    """


def render_book_snapshots(events: list[dict[str, Any]]) -> str:
    snapshots = book_snapshots(events)
    if not snapshots:
        return placeholder("Book reconstruction snapshots")
    panels = []
    for snapshot in snapshots:
        book = snapshot["book"]
        stock = html.escape(str(snapshot.get("stock", "unknown")))
        timestamp = html.escape(str(snapshot.get("timestamp", "unknown")))
        spread = snapshot.get("spread")
        spread_text = "n/a" if spread is None else f"{int(spread)}¢ spread"
        panels.append(
            f"""
            <div class="book-panel">
              <h3>{stock} · {timestamp} · {html.escape(spread_text)}</h3>
              <div class="book-grid">
                {render_side(book["bids"], "Bid")}
                {render_side(book["asks"], "Ask")}
              </div>
            </div>
            """
        )
    return f"""
      <section>
        <h2>Book reconstruction snapshots</h2>
        <p class="caption">Top-five bid/ask ladders at three deterministic points in the loaded replay window.</p>
        {"".join(panels)}
      </section>
    """


def render_report(summary: dict[str, Any], events: list[dict[str, Any]]) -> str:
    counts = summary["event_counts"]
    count_rows = "\n".join(
        f"<tr><td>{html.escape(kind)}</td><td>{count}</td></tr>"
        for kind, count in sorted(counts.items())
    )
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>nanobook v0.11 — ITCH Replay Report</title>
  <style>
    body {{ background: #f8f8f6; color: #1a1a1a; font-family: -apple-system, BlinkMacSystemFont, "Helvetica Neue", Arial, sans-serif; margin: 0; }}
    main {{ margin: 0 auto; max-width: 960px; padding: 36px 20px 56px; }}
    h1 {{ font-size: 36px; line-height: 1.1; margin: 0 0 8px; }}
    h2 {{ font-size: 18px; margin-top: 34px; }}
    table {{ border-collapse: collapse; width: 100%; }}
    th, td {{ border: 1px solid #ddd; padding: 8px 10px; text-align: left; }}
    th {{ background: #f1f1ef; font-size: 12px; text-transform: uppercase; }}
    .plot {{ align-items: center; background: #fff; border: 1px solid #ddd; color: #666; display: flex; height: 320px; justify-content: center; }}
    .plot-svg {{ background: #fff; border: 1px solid #ddd; display: block; height: 320px; width: 100%; }}
    .caption {{ color: #555; font-size: 14px; margin: 0 0 12px; }}
    .book-grid {{ display: grid; gap: 14px; grid-template-columns: 1fr 1fr; }}
    .book-panel {{ margin-top: 16px; }}
    .book-panel h3 {{ font-size: 14px; font-weight: 650; margin: 0 0 8px; }}
    .metrics {{ display: grid; gap: 12px; grid-template-columns: repeat(3, 1fr); margin: 24px 0; }}
    .metric {{ background: #fff; border: 1px solid #ddd; padding: 14px 16px; }}
    .num {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 22px; font-weight: 700; }}
  </style>
</head>
<body>
  <main>
    <h1>v0.11 · ITCH Replay Report</h1>
    <p>Skeleton report generated from replay JSONL through the nanobook Python extension.</p>
    <div class="metrics">
      <div class="metric"><div class="num">{summary["events"]}</div><div>events loaded</div></div>
      <div class="metric"><div class="num">{summary["symbols"]}</div><div>symbols</div></div>
      <div class="metric"><div class="num">{summary["total_volume"]}</div><div>total volume</div></div>
    </div>
    <table>
      <thead><tr><th>Event type</th><th>Count</th></tr></thead>
      <tbody>{count_rows}</tbody>
    </table>
    {render_latency(events)}
    {svg_line_chart(message_rate(events), "Message-rate timeline")}
    {svg_bar_chart(spread_distribution(events), "Spread distribution", "Spread in cents")}
    {render_book_snapshots(events)}
  </main>
</body>
</html>
"""


def main() -> None:
    args = parse_args()
    events = load_events(args.input, args.limit)
    report = render_report(summarize(events), events)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(report, encoding="utf-8")


if __name__ == "__main__":
    main()
