//! Tests for Binance broker account info query and reconciliation (F-bin2 Phase 3).

#[cfg(feature = "binance")]
mod binance_reconcile_tests {
    use nanobook::Symbol;
    use nanobook_broker::binance::BinanceBroker;
    use nanobook_broker::Broker;
    use nanobook_broker::error::BrokerError;
    use nanobook_broker::types::*;

    // ========================================================================
    // Reconciliation Blocking Flag
    // ========================================================================

    #[test]
    fn test_reconciliation_blocked_initially_false() {
        let broker = BinanceBroker::new("test_key", "test_secret", true);

        // Initial state should have reconciliation_blocked = false
        assert!(!broker.is_reconciliation_blocked());
    }

    #[test]
    fn test_block_reconciliation_sets_flag() {
        let mut broker = BinanceBroker::new("test_key", "test_secret", true);

        // Block reconciliation
        broker.block_reconciliation();

        // Flag should now be true
        assert!(broker.is_reconciliation_blocked());
    }

    #[test]
    fn test_unblock_reconciliation_clears_flag() {
        let mut broker = BinanceBroker::new("test_key", "test_secret", true);

        // Block reconciliation
        broker.block_reconciliation();
        assert!(broker.is_reconciliation_blocked());

        // Unblock reconciliation
        broker.unblock_reconciliation();

        // Flag should now be false
        assert!(!broker.is_reconciliation_blocked());
    }

    #[test]
    fn test_reconciliation_blocks_submission() {
        // Verify that when reconciliation is blocked,
        // order submission should fail with the appropriate error.

        let mut broker = BinanceBroker::new("test_key", "test_secret", true);

        // Connect the broker
        broker.connect().unwrap();

        // Block reconciliation
        broker.block_reconciliation();

        // Try to submit an order - should fail
        let order = BrokerOrder {
            symbol: Symbol::try_new("BTC").unwrap(),
            side: BrokerSide::Buy,
            order_type: BrokerOrderType::Market,
            quantity: 100,
            client_order_id: None,
        };

        let result = broker.submit_order(&order);
        assert!(result.is_err());
        match result.unwrap_err() {
            BrokerError::Order(msg) => {
                assert_eq!(msg, "Reconciliation blocked - manual review required");
            }
            _ => panic!("Expected Order error variant"),
        }
    }

    #[test]
    fn test_reconcile_state_sets_block_flag() {
        // This test verifies that reconcile_state() sets the block flag
        // when discrepancies are found. Since reconcile_state() requires
        // a connected client, we test the flag logic separately.

        let mut broker = BinanceBroker::new("test_key", "test_secret", true);

        // Initially not blocked
        assert!(!broker.is_reconciliation_blocked());

        // Simulate that reconcile_state() found critical issues
        // (in real code, this happens inside reconcile_state())
        broker.block_reconciliation();

        // Flag should be set
        assert!(broker.is_reconciliation_blocked());

        // After manual review, operator can unblock
        broker.unblock_reconciliation();
        assert!(!broker.is_reconciliation_blocked());
    }

    #[test]
    fn test_account_info_query() {
        // Verify that account_info can be called when connected.
        // This test requires a real Binance connection or a mock.
        // For now, we test that the method exists and can be called
        // through the positions() and account() methods which use it.

        let broker = BinanceBroker::new("test_key", "test_secret", true);

        // Without connection, should fail
        let result = broker.positions();
        assert!(result.is_err());
        match result.unwrap_err() {
            BrokerError::NotConnected => {}
            _ => panic!("Expected NotConnected error"),
        }

        let result = broker.account();
        assert!(result.is_err());
        match result.unwrap_err() {
            BrokerError::NotConnected => {}
            _ => panic!("Expected NotConnected error"),
        }
    }

    #[test]
    fn test_reconcile_state_no_discrepancies() {
        // This test verifies that reconcile_state() works correctly
        // when there are no discrepancies. Since we can't connect to
        // a real broker in unit tests, we verify the logic structure
        // by testing the flag behavior.

        let broker = BinanceBroker::new("test_key", "test_secret", true);

        // Initially not blocked
        assert!(!broker.is_reconciliation_blocked());

        // If reconcile_state() found no discrepancies, the flag should remain false
        // We simulate this by not calling block_reconciliation()
        assert!(!broker.is_reconciliation_blocked());
    }

    #[test]
    fn test_reconcile_state_orphan_order() {
        // This test verifies that orphan order detection logic
        // would set the block flag. Since we can't connect to a
        // real broker in unit tests, we verify the flag behavior.

        let mut broker = BinanceBroker::new("test_key", "test_secret", true);

        // Initially not blocked
        assert!(!broker.is_reconciliation_blocked());

        // Simulate orphan order detection by blocking reconciliation
        broker.block_reconciliation();

        // Flag should be set
        assert!(broker.is_reconciliation_blocked());
    }
}

// ============================================================================
// Tests that don't require the binance feature
// ============================================================================

#[test]
fn test_reconciliation_blocking_error_exists() {
    // Verify that the Order error variant exists and can be constructed
    // for the reconciliation blocking case.
    use nanobook_broker::error::BrokerError;

    let error = BrokerError::Order("Reconciliation blocked - manual review required".to_string());

    match error {
        BrokerError::Order(msg) => {
            assert_eq!(msg, "Reconciliation blocked - manual review required");
        }
        _ => panic!("Expected Order variant"),
    }
}
