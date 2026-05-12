#!/usr/bin/env python3
"""
Python scenario runner for failure injection tests.

This script reads YAML scenario definitions and generates Rust test code
that exercises the MockTws with the specified failure sequences.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict, List


def parse_scenario(yaml_path: Path) -> Dict[str, Any]:
    """Parse a YAML scenario file."""
    try:
        import yaml
    except ImportError:
        print("Error: PyYAML is required. Install with: uv add pyyaml")
        sys.exit(1)

    with open(yaml_path, 'r') as f:
        return yaml.safe_load(f)


def generate_rust_test(scenario: Dict[str, Any]) -> str:
    """Generate Rust test code from a scenario definition."""
    name = scenario.get('name', 'unnamed_scenario')
    steps = scenario.get('steps', [])

    # Generate test code
    test_code = f"""//! Auto-generated test for scenario: {name}

mod mock_tws;

use mock_tws::{{MockTws, FailureMode, FailureTiming}};

#[test]
fn scenario_{name.replace(' ', '_').replace('-', '_').lower()}() {{
    let mock = MockTws::new();
"""

    for step in steps:
        action = step.get('action')

        if action == 'connect':
            test_code += "    mock.connect().unwrap();\n"

        elif action == 'disconnect':
            test_code += "    mock.disconnect().unwrap();\n"

        elif action == 'submit_order':
            symbol = step.get('symbol', 'AAPL')
            quantity = step.get('quantity', 100)
            test_code += f"    let order_id = mock.submit_order(\"{symbol}\", {quantity}).unwrap();\n"

        elif action == 'fill_order':
            order_id = step.get('order_id', 1)
            quantity = step.get('quantity', 100)
            test_code += f"    mock.fill_order({order_id}, {quantity}).unwrap();\n"

        elif action == 'cancel_order':
            order_id = step.get('order_id', 1)
            test_code += f"    mock.cancel_order({order_id}).unwrap();\n"

        elif action == 'inject_failure':
            mode = step.get('mode', 'F1')
            timing = step.get('timing', 'PreSubmit')
            test_code += f"    mock.inject_failure(FailureMode::{mode}, FailureTiming::{timing});\n"

        elif action == 'assert':
            condition = step.get('condition')

            if condition == 'disconnected':
                test_code += "    assert!(!mock.is_connected());\n"
            elif condition == 'connected':
                test_code += "    assert!(mock.is_connected());\n"
            elif condition == 'error':
                test_code += "    // Expected error condition\n"
            elif condition == 'callback_count':
                count = step.get('count', 0)
                test_code += f"    assert!(mock.callbacks().len() >= {count});\n"
            else:
                test_code += f"    // Unknown assertion: {condition}\n"

        elif action == 'clear_failure':
            test_code += "    mock.clear_failure();\n"

        elif action == 'clear_callbacks':
            test_code += "    mock.clear_callbacks();\n"

    test_code += "}\n"
    return test_code


def run_scenario(yaml_path: Path) -> bool:
    """Run a single scenario by generating and executing Rust test."""
    print(f"Running scenario: {yaml_path}")

    # Parse scenario
    scenario = parse_scenario(yaml_path)

    # Generate Rust test code
    test_code = generate_rust_test(scenario)

    # Write to a temporary test file in the tests/ directory
    test_name = yaml_path.stem
    # scenarios/ is inside tests/failure_injection/, so we need to go up two levels to tests/
    tests_dir = yaml_path.parent.parent.parent  # broker/tests/
    test_file = tests_dir / f"scenario_{test_name}.rs"

    try:
        with open(test_file, 'w') as f:
            f.write(test_code)
        print(f"Generated test file: {test_file}")
        print(f"Test file exists: {test_file.exists()}")
    except Exception as e:
        print(f"Error writing test file: {e}", file=sys.stderr)
        return False

    # Run the test from workspace root
    # tests_dir is broker/tests/, so workspace_root is nanobook/
    workspace_root = tests_dir.parent.parent  # nanobook/

    # Generate the test function name from scenario name
    scenario_name = scenario.get('name', test_name)
    test_fn_name = scenario_name.replace(' ', '_').replace('-', '_').lower()

    result = subprocess.run(
        ['cargo', 'test', '-p', 'nanobook-broker', test_fn_name],
        cwd=workspace_root,
        capture_output=True,
        text=True
    )

    print(result.stdout)
    if result.stderr:
        print(result.stderr, file=sys.stderr)

    # Clean up the generated file
    test_file.unlink()

    return result.returncode == 0


def list_scenarios(scenarios_dir: Path) -> List[Path]:
    """List all scenario YAML files in the directory."""
    return list(scenarios_dir.glob('*.yaml')) + list(scenarios_dir.glob('*.yml'))


def main():
    parser = argparse.ArgumentParser(
        description='Run failure injection scenarios'
    )
    parser.add_argument(
        'scenario',
        nargs='?',
        help='Specific scenario file to run (runs all if not specified)'
    )
    parser.add_argument(
        '--list',
        action='store_true',
        help='List available scenarios'
    )
    parser.add_argument(
        '--scenarios-dir',
        type=Path,
        default=Path(__file__).parent,
        help='Directory containing scenario files'
    )

    args = parser.parse_args()

    if args.list:
        scenarios = list_scenarios(args.scenarios_dir)
        print('Available scenarios:')
        for s in scenarios:
            print(f"  - {s.name}")
        return 0

    if args.scenario:
        scenario_path = Path(args.scenario)
        if not scenario_path.is_absolute():
            scenario_path = args.scenarios_dir / scenario_path
        success = run_scenario(scenario_path)
        return 0 if success else 1
    else:
        # Run all scenarios
        scenarios = list_scenarios(args.scenarios_dir)
        if not scenarios:
            print('No scenarios found')
            return 0

        print(f'Found {len(scenarios)} scenario(s)')
        all_passed = True
        for scenario_path in scenarios:
            print()
            if not run_scenario(scenario_path):
                all_passed = False

        return 0 if all_passed else 1


if __name__ == '__main__':
    sys.exit(main())