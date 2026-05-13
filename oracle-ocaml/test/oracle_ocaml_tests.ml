(* Unit tests for OCaml oracle *)
open Oracle_lib

(* Test counter *)
let test_count = ref 0
let pass_count = ref 0

let assert_eq (name : string) (expected : 'a) (actual : 'a) =
  test_count := !test_count + 1;
  if expected = actual then (
    pass_count := !pass_count + 1;
    Printf.printf "✓ %s\n" name
  ) else (
    Printf.printf "✗ %s: expected %v, got %v\n" name expected actual
  )

let assert_true (name : string) (condition : bool) =
  test_count := !test_count + 1;
  if condition then (
    pass_count := !pass_count + 1;
    Printf.printf "✓ %s\n" name
  ) else (
    Printf.printf "✗ %s: condition failed\n" name
  )

(* Helper: Create a simple book *)
let create_test_book () =
  Book.create ()

(* Test: Simple cross produces a trade *)
let test_simple_cross () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 50L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  assert_eq "simple_cross produces trade" 1 (List.length trades)

(* Test: No cross produces no trades *)
let test_no_cross () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 9999L; quantity = 50L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  assert_eq "no_cross produces no trades" 0 (List.length trades)

(* Test: Trade quantity is correct *)
let test_trade_quantity () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 50L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  let trade = List.hd trades in
  assert_eq "trade_quantity is correct" 50L trade.Matching.quantity

(* Test: Trade price is correct *)
let test_trade_price () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 50L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  let trade = List.hd trades in
  assert_eq "trade_price is correct" 10000L trade.Matching.price

(* Test: No negative quantities *)
let test_no_negative_quantities () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 50L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  assert_true "no negative quantities" (List.for_all (fun t -> t.Matching.quantity >= 0L) trades)

(* Test: Valid trade prices *)
let test_valid_trade_prices () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 50L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  assert_true "valid trade prices" (List.for_all (fun t -> t.Matching.price >= 0L) trades)

(* Test: Order IDs are unique *)
let test_unique_order_ids () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 50L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  let aggressor_ids = List.map (fun t -> t.Matching.aggressor_order_id) trades in
  let passive_ids = List.map (fun t -> t.Matching.passive_order_id) trades in
  let all_ids = aggressor_ids @ passive_ids in
  (* Check for duplicates *)
  let unique_ids = List.fold_left (fun acc id ->
    if List.mem id acc then acc else id :: acc
  ) [] all_ids in
  assert_eq "unique order IDs" (List.length all_ids) (List.length unique_ids)

(* Test: FOK with insufficient liquidity produces no trades *)
let test_fok_no_liquidity () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 10L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 100L; time_in_force = Order.FOK; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  assert_eq "FOK with insufficient liquidity produces no trades" 0 (List.length trades)

(* Test: FOK with sufficient liquidity produces trades *)
let test_fok_with_liquidity () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 50L; time_in_force = Order.FOK; owner = None; stp_policy = Matching.Off };
  ] in
  let trades = Replay.replay_events book events in
  assert_eq "FOK with sufficient liquidity produces trades" 1 (List.length trades)

(* Test: IOC with partial fill *)
let test_ioc_partial_fill () =
  let book = create_test_book () in
  let events = [
    Replay.SubmitLimit { side = Side.Sell; price = 10000L; quantity = 100L; time_in_force = Order.GTC; owner = None; stp_policy = Matching.Off };
    Replay.SubmitLimit { side = Side.Buy; price = 10000L; quantity = 50L; time_in_force = Order.IOC; owner = None; stp_policy = Matching.Off },
  ] in
  let trades = Replay.replay_events book events in
  assert_eq "IOC partial fill produces trade" 1 (List.length trades)

(* Run all tests *)
let () =
  print_endline "Running OCaml oracle unit tests...";
  print_newline ();
  
  test_simple_cross ();
  test_no_cross ();
  test_trade_quantity ();
  test_trade_price ();
  test_no_negative_quantities ();
  test_valid_trade_prices ();
  test_unique_order_ids ();
  test_fok_no_liquidity ();
  test_fok_with_liquidity ();
  test_ioc_partial_fill ();
  
  print_newline ();
  Printf.printf "Results: %d/%d tests passed\n" !pass_count !test_count;
  if !pass_count = !test_count then
    exit 0
  else
    exit 1
