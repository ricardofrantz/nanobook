import nanobook


def _checks_by_name(report):
    return {check["name"]: check for check in report}


def test_risk_engine_accepts_new_cap_defaults():
    risk = nanobook.RiskEngine(
        max_order_value_cents=10_000,
        max_batch_value_cents=10_000,
    )
    report = risk.check_order("AAPL", "buy", 50, 200, 100_000_000, [])
    assert isinstance(report, list)
    assert len(report) > 0


def test_risk_order_value_boundary():
    risk = nanobook.RiskEngine(
        max_order_value_cents=10_000,
        max_position_pct=1.0,
        max_batch_value_cents=100_000_000,
    )
    report = risk.check_order("AAPL", "buy", 50, 200, 100_000_000, [])
    checks = _checks_by_name(report)
    assert checks["Max order value"]["status"] == "PASS"

    fail_report = risk.check_order("AAPL", "buy", 51, 200, 100_000_000, [])
    fail_checks = _checks_by_name(fail_report)
    assert fail_checks["Max order value"]["status"] == "FAIL"


def test_risk_batch_report_includes_cap_checks():
    risk = nanobook.RiskEngine(
        max_batch_value_cents=10_000,
        max_order_value_cents=10_000,
        max_position_pct=1.0,
    )
    report = risk.check_batch(
        orders=[("AAPL", "buy", 100, 100), ("MSFT", "buy", 0, 0)],
        equity_cents=100_000_000,
        positions=[],
        target_weights=[("AAPL", 0.5), ("MSFT", 0.5)],
    )
    checks = _checks_by_name(report)
    assert "Max batch value" not in checks
    assert "Max order value" not in checks

    fail_report = risk.check_batch(
        orders=[("AAPL", "buy", 30, 400), ("MSFT", "buy", 30, 400)],
        equity_cents=100_000_000,
        positions=[],
        target_weights=[("AAPL", 0.5), ("MSFT", 0.5)],
    )
    fail_checks = _checks_by_name(fail_report)
    assert fail_checks["Max batch value"]["status"] == "FAIL"
