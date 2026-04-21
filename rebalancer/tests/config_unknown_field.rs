use nanobook_rebalancer::config::Config;

fn valid_config_toml() -> &'static str {
    r#"
[connection]
host = "127.0.0.1"
port = 4002
client_id = 100

[account]
id = "DU123456"
type = "margin"

[execution]
order_interval_ms = 100
limit_offset_bps = 5
order_timeout_secs = 300
max_orders_per_run = 50

[risk]
max_position_pct = 0.25
max_leverage = 1.5
min_trade_usd = 100.0
max_trade_usd = 100000.0
allow_short = true
max_short_pct = 0.30

[cost]
commission_per_share = 0.0035
commission_min = 0.35
slippage_bps = 5

[logging]
dir = "./logs"
audit_file = "audit.jsonl"
"#
}

#[test]
fn typo_in_risk_config_is_rejected() {
    let toml = valid_config_toml().replace(
        "max_leverage = 1.5",
        "max_leverage = 1.5\nmax_leverage_pct = 1.5",
    );

    let err = toml::from_str::<Config>(&toml).expect_err("typo must error");
    let msg = err.to_string();
    assert!(
        msg.contains("max_leverage_pct") || msg.contains("unknown field"),
        "got: {msg}"
    );
}

#[test]
fn typo_in_execution_config_is_rejected() {
    let toml = valid_config_toml().replace(
        "max_orders_per_run = 50",
        "max_orders_per_run = 50\nmax_order_per_run = 50",
    );

    let err = toml::from_str::<Config>(&toml).expect_err("typo must error");
    let msg = err.to_string();
    assert!(
        msg.contains("max_order_per_run") || msg.contains("unknown field"),
        "got: {msg}"
    );
}

#[test]
fn valid_config_still_parses() {
    let config = toml::from_str::<Config>(valid_config_toml()).unwrap();

    assert_eq!(config.connection.port, 4002);
    assert_eq!(config.risk.max_leverage, 1.5);
    assert_eq!(config.execution.max_orders_per_run, 50);
}
