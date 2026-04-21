use nanobook::Symbol;
use nanobook_rebalancer::diff::{CurrentPosition, compute_diff};
use nanobook_rebalancer::execution::derive_client_order_id;
use nanobook_rebalancer::target::TargetSpec;

fn target_spec() -> TargetSpec {
    TargetSpec::from_json(
        r#"{
            "timestamp": "2026-04-20T15:30:00Z",
            "metadata": { "id": "sched-2026-04-20" },
            "targets": [
                { "symbol": "AAPL", "weight": 0.40 },
                { "symbol": "MSFT", "weight": 0.30 }
            ]
        }"#,
    )
    .unwrap()
}

fn rebalance_ids(target: &TargetSpec) -> Vec<String> {
    let current = vec![CurrentPosition {
        symbol: Symbol::new("AAPL"),
        quantity: 100,
        avg_cost_cents: 18_000,
    }];
    let prices = vec![(Symbol::new("AAPL"), 18_500), (Symbol::new("MSFT"), 37_000)];
    let orders = compute_diff(
        100_000_000,
        &current,
        &target.as_target_pairs(),
        &prices,
        0,
        0,
    );

    orders
        .iter()
        .map(|order| {
            derive_client_order_id(target, order)
                .unwrap()
                .as_str()
                .to_string()
        })
        .collect()
}

#[test]
fn idempotency_same_target_spec_yields_same_client_order_ids() {
    let target = target_spec();

    let first = rebalance_ids(&target);
    let second = rebalance_ids(&target);

    assert!(!first.is_empty());
    assert_eq!(first, second);
}

#[test]
fn idempotency_target_metadata_id_changes_client_order_ids() {
    let a = target_spec();
    let b = TargetSpec::from_json(
        r#"{
            "timestamp": "2026-04-20T15:30:00Z",
            "metadata": { "id": "sched-2026-04-21" },
            "targets": [
                { "symbol": "AAPL", "weight": 0.40 },
                { "symbol": "MSFT", "weight": 0.30 }
            ]
        }"#,
    )
    .unwrap();

    assert_ne!(rebalance_ids(&a), rebalance_ids(&b));
}
