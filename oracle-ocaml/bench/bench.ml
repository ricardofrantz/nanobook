(* Performance benchmarks for OCaml oracle *)
open Oracle_lib

(* Timing helper *)
let time f =
  let start = Unix.gettimeofday () in
  let result = f () in
  let finish = Unix.gettimeofday () in
  (result, finish -. start)

(* Benchmark: Simple cross throughput *)
let bench_simple_cross n =
  Printf.printf "Benchmark: Simple cross (n=%d)\n" n;
  let book = Book.create () in
  
  let _, setup_time = time (fun () ->
    (* Add resting sell orders *)
    for i = 0 to n - 1 do
      let event = Replay.SubmitLimit {
        side = Side.Sell;
        price = 10000L;
        quantity = 100L;
        time_in_force = Order.GTC;
        owner = None;
        stp_policy = Matching.Off;
      } in
      ignore (Replay.replay_events book [event])
    done
  ) in
  
  Printf.printf "  Setup time: %.4f s\n" setup_time;
  
  let _, match_time = time (fun () ->
    (* Match against buy orders *)
    for i = 0 to n - 1 do
      let event = Replay.SubmitLimit {
        side = Side.Buy;
        price = 10000L;
        quantity = 50L;
        time_in_force = Order.GTC;
        owner = None;
        stp_policy = Matching.Off;
      } in
      ignore (Replay.replay_events book [event])
    done
  ) in
  
  Printf.printf "  Match time: %.4f s\n" match_time;
  Printf.printf "  Throughput: %.0f orders/sec\n" (float_of_int (2 * n) /. match_time);
  print_newline ()

(* Benchmark: Market order sweep *)
let bench_market_sweep n =
  Printf.printf "Benchmark: Market order sweep (n=%d levels)\n" n;
  let book = Book.create () in
  
  let _, setup_time = time (fun () ->
    (* Add sell orders at different price levels *)
    for i = 0 to n - 1 do
      let price = Int64.of_int (10000 + i) in
      let event = Replay.SubmitLimit {
        side = Side.Sell;
        price = price;
        quantity = 100L;
        time_in_force = Order.GTC;
        owner = None;
        stp_policy = Matching.Off;
      } in
      ignore (Replay.replay_events book [event])
    done
  ) in
  
  Printf.printf "  Setup time: %.4f s\n" setup_time;
  
  let _, sweep_time = time (fun () ->
    (* Sweep with market order *)
    let event = Replay.SubmitMarket {
      side = Side.Buy;
      quantity = Int64.of_int (100 * n);
      owner = None;
    } in
    ignore (Replay.replay_events book [event])
  ) in
  
  Printf.printf "  Sweep time: %.4f s\n" sweep_time;
  Printf.printf "  Levels/sec: %.0f\n" (float_of_int n /. sweep_time);
  print_newline ()

(* Benchmark: Large order book *)
let bench_large_book n =
  Printf.printf "Benchmark: Large order book (n=%d orders)\n" n;
  let book = Book.create () in
  
  let _, setup_time = time (fun () ->
    (* Add many orders at random prices *)
    for i = 0 to n - 1 do
      let price = Int64.of_int (9500 + Random.int 1000) in
      let side = if Random.int 2 = 0 then Side.Buy else Side.Sell in
      let event = Replay.SubmitLimit {
        side = side;
        price = price;
        quantity = Int64.of_int (10 + Random.int 90);
        time_in_force = Order.GTC;
        owner = None;
        stp_policy = Matching.Off;
      } in
      ignore (Replay.replay_events book [event])
    done
  ) in
  
  Printf.printf "  Setup time: %.4f s\n" setup_time;
  Printf.printf "  Orders/sec: %.0f\n" (float_of_int n /. setup_time);
  print_newline ()

(* Benchmark: Order cancellation *)
let bench_cancel n =
  Printf.printf "Benchmark: Order cancellation (n=%d)\n" n;
  let book = Book.create () in
  
  (* Add orders and collect IDs *)
  let order_ids = ref [] in
  let _, setup_time = time (fun () ->
    for i = 0 to n - 1 do
      let event = Replay.SubmitLimit {
        side = Side.Sell;
        price = 10000L;
        quantity = 100L;
        time_in_force = Order.GTC;
        owner = None;
        stp_policy = Matching.Off;
      } in
      let trades = Replay.replay_events book [event] in
      (* Get the order ID from the book - this is a simplification *)
      order_ids := (Int64.of_int i) :: !order_ids
    done
  ) in
  
  Printf.printf "  Setup time: %.4f s\n" setup_time;
  
  let _, cancel_time = time (fun () ->
    (* Cancel orders *)
    List.iter (fun id ->
      let event = Replay.Cancel (Int64.to_int id) in
      ignore (Replay.replay_events book [event])
    ) !order_ids
  ) in
  
  Printf.printf "  Cancel time: %.4f s\n" cancel_time;
  Printf.printf "  Cancels/sec: %.0f\n" (float_of_int n /. cancel_time);
  print_newline ()

(* Run all benchmarks *)
let () =
  Random.self_init ();
  print_endline "OCaml Oracle Performance Benchmarks";
  print_endline "====================================";
  print_newline ();
  
  (* Run benchmarks with different sizes *)
  bench_simple_cross 1000;
  bench_simple_cross 10000;
  
  bench_market_sweep 100;
  bench_market_sweep 1000;
  
  bench_large_book 1000;
  bench_large_book 10000;
  
  bench_cancel 1000;
  bench_cancel 10000;
  
  print_endline "Benchmark complete."