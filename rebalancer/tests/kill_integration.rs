//! Integration tests for the kill switch functionality.

use std::fs;

#[test]
fn test_verify_no_dangling_orders_integration() {
    // Test order verification with a realistic audit log
    let temp_dir = tempfile::tempdir().unwrap();
    let audit_path = temp_dir.path().join("audit.jsonl");

    // Create an audit log with submitted and filled orders
    let audit_log = r#"{"event":"order_submitted","ts":"2024-01-01T00:00:00Z","symbol":"AAPL","action":"Buy","shares":100,"limit":150.00,"ibkr_id":1}
{"event":"order_filled","ts":"2024-01-01T00:00:01Z","symbol":"AAPL","ibkr_id":1,"filled":100,"avg_price":150.00}
{"event":"order_submitted","ts":"2024-01-01T00:00:02Z","symbol":"MSFT","action":"Sell","shares":50,"limit":400.00,"ibkr_id":2}
{"event":"order_filled","ts":"2024-01-01T00:00:03Z","symbol":"MSFT","ibkr_id":2,"filled":50,"avg_price":400.00}"#;
    fs::write(&audit_path, audit_log).unwrap();

    // Verify no dangling orders
    let result = nanobook_rebalancer::kill::verify_no_dangling_orders(&audit_path);
    assert!(result.is_ok());
    let dangling = result.unwrap();
    assert!(dangling.is_empty());
}

#[test]
fn test_verify_dangling_orders_integration() {
    // Test order verification with dangling orders
    let temp_dir = tempfile::tempdir().unwrap();
    let audit_path = temp_dir.path().join("audit.jsonl");

    // Create an audit log with a dangling order (submitted but not filled)
    let audit_log = r#"{"event":"order_submitted","ts":"2024-01-01T00:00:00Z","symbol":"AAPL","action":"Buy","shares":100,"limit":150.00,"ibkr_id":1}
{"event":"order_submitted","ts":"2024-01-01T00:00:01Z","symbol":"MSFT","action":"Sell","shares":50,"limit":400.00,"ibkr_id":2}
{"event":"order_filled","ts":"2024-01-01T00:00:02Z","symbol":"AAPL","ibkr_id":1,"filled":100,"avg_price":150.00}"#;
    fs::write(&audit_path, audit_log).unwrap();

    // Verify dangling orders
    let result = nanobook_rebalancer::kill::verify_no_dangling_orders(&audit_path);
    assert!(result.is_ok());
    let dangling = result.unwrap();
    assert_eq!(dangling.len(), 1);
    assert_eq!(dangling[0].symbol, "MSFT");
    assert_eq!(dangling[0].ibkr_id, 2);
}
