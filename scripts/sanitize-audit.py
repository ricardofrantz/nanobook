#!/usr/bin/env python3
"""
Publishable PII scrubber for nanobook audit logs.

Removes account IDs, IBKR client IDs, and order IDs while preserving
audit-log invariants (sequence numbers, timestamps, order math, fill sequence).

Idempotent — safe to re-run. Supports --check mode for CI verification.
"""

import argparse
import json
import sys
from pathlib import Path
from typing import Dict, Any, Set


# Placeholder values for scrubbed PII
ACCOUNT_PLACEHOLDER = "ACCOUNT_REDACTED"
IBKR_ID_PLACEHOLDER = 999999


# Fields that contain PII and should be scrubbed
PII_FIELDS = {
    "account",  # account_id in run_started events
    "ibkr_id",  # order IDs in order_submitted, order_filled events
}


def scrub_value(value: Any) -> Any:
    """Scrub a value if it's a PII field type."""
    if isinstance(value, str) and value != ACCOUNT_PLACEHOLDER:
        return ACCOUNT_PLACEHOLDER
    elif isinstance(value, int) and value != IBKR_ID_PLACEHOLDER:
        return IBKR_ID_PLACEHOLDER
    return value


def scrub_json_object(obj: Dict[str, Any], pii_fields: Set[str]) -> Dict[str, Any]:
    """Recursively scrub PII fields from a JSON object."""
    if not isinstance(obj, dict):
        return obj
    
    scrubbed = {}
    for key, value in obj.items():
        if key in pii_fields:
            scrubbed[key] = scrub_value(value)
        elif isinstance(value, dict):
            scrubbed[key] = scrub_json_object(value, pii_fields)
        elif isinstance(value, list):
            scrubbed[key] = [scrub_json_object(item, pii_fields) if isinstance(item, dict) else item for item in value]
        else:
            scrubbed[key] = value
    
    return scrubbed


def has_pii(obj: Dict[str, Any], pii_fields: Set[str]) -> bool:
    """Check if a JSON object contains PII (non-placeholder values)."""
    if not isinstance(obj, dict):
        return False
    
    for key, value in obj.items():
        if key in pii_fields:
            if isinstance(value, str) and value != ACCOUNT_PLACEHOLDER:
                return True
            elif isinstance(value, int) and value != IBKR_ID_PLACEHOLDER:
                return True
        elif isinstance(value, dict):
            if has_pii(value, pii_fields):
                return True
        elif isinstance(value, list):
            for item in value:
                if isinstance(item, dict) and has_pii(item, pii_fields):
                    return True
    
    return False


def process_audit_log(input_path: Path, output_path: Path, check_mode: bool = False) -> int:
    """
    Process an audit log file.
    
    Returns the number of lines that contained PII (in check mode) or were scrubbed.
    """
    pii_count = 0
    
    with open(input_path, 'r', encoding='utf-8') as f_in:
        if check_mode:
            # Check mode: verify no PII remains
            line_num = 0
            for line in f_in:
                line_num += 1
                line = line.strip()
                if not line:
                    continue
                
                try:
                    obj = json.loads(line)
                    if has_pii(obj, PII_FIELDS):
                        print(f"Line {line_num}: PII detected", file=sys.stderr)
                        pii_count += 1
                except json.JSONDecodeError as e:
                    print(f"Line {line_num}: JSON decode error: {e}", file=sys.stderr)
                    return 1
        else:
            # Scrub mode: write scrubbed output
            with open(output_path, 'w', encoding='utf-8') as f_out:
                line_num = 0
                for line in f_in:
                    line_num += 1
                    line = line.strip()
                    if not line:
                        f_out.write('\n')
                        continue
                    
                    try:
                        obj = json.loads(line)
                        scrubbed = scrub_json_object(obj, PII_FIELDS)
                        f_out.write(json.dumps(scrubbed) + '\n')
                        if has_pii(obj, PII_FIELDS):
                            pii_count += 1
                    except json.JSONDecodeError as e:
                        print(f"Line {line_num}: JSON decode error: {e}", file=sys.stderr)
                        return 1
    
    return pii_count


def main():
    parser = argparse.ArgumentParser(
        description='Scrub PII from nanobook audit logs'
    )
    parser.add_argument(
        'input',
        type=Path,
        help='Input audit.jsonl file'
    )
    parser.add_argument(
        'output',
        type=Path,
        nargs='?',
        help='Output file (defaults to input file for in-place scrubbing)'
    )
    parser.add_argument(
        '--check',
        action='store_true',
        help='Check mode: verify no PII remains (exit code 1 if PII found)'
    )
    
    args = parser.parse_args()
    
    if not args.input.exists():
        print(f"Error: Input file not found: {args.input}", file=sys.stderr)
        return 1
    
    if args.check:
        # Check mode: only input required
        pii_count = process_audit_log(args.input, None, check_mode=True)
        if pii_count > 0:
            print(f"Found PII in {pii_count} line(s)", file=sys.stderr)
            return 1
        else:
            print("No PII detected")
            return 0
    else:
        # Scrub mode
        output = args.output or args.input
        pii_count = process_audit_log(args.input, output, check_mode=False)
        print(f"Scrubbed {pii_count} line(s) with PII")
        return 0


if __name__ == '__main__':
    sys.exit(main())
