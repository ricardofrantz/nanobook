(* JSON serialization/deserialization for events and trades *)
open Yojson.Basic

(* Helper functions for JSON conversion *)
let int64_to_json i = `Int (Int64.to_int i)
let json_to_int64 = function
  | `Int i -> Int64.of_int i
  | `String s -> Int64.of_string s
  | `Float f -> Int64.of_int (int_of_float f)
  | _ -> invalid_arg "invalid int64"

let side_to_json = function
  | Side.Buy -> `String "BUY"
  | Side.Sell -> `String "SELL"

let json_to_side = function
  | `String "BUY" -> Side.Buy
  | `String "SELL" -> Side.Sell
  | _ -> invalid_arg "invalid side"

let tif_to_json = function
  | Order.GTC -> `String "GTC"
  | Order.IOC -> `String "IOC"
  | Order.FOK -> `String "FOK"

let json_to_tif = function
  | `String "GTC" -> Order.GTC
  | `String "IOC" -> Order.IOC
  | `String "FOK" -> Order.FOK
  | _ -> invalid_arg "invalid time_in_force"

(* Event serialization *)
let event_to_json = function
  | Replay.SubmitLimit { side; price; quantity; time_in_force } ->
      `Assoc [
        ("type", `String "SubmitLimit");
        ("side", side_to_json side);
        ("price", int64_to_json price);
        ("quantity", int64_to_json quantity);
        ("time_in_force", tif_to_json time_in_force);
      ]
  | Replay.SubmitMarket { side; quantity } ->
      `Assoc [
        ("type", `String "SubmitMarket");
        ("side", side_to_json side);
        ("quantity", int64_to_json quantity);
      ]
  | Replay.Cancel { order_id } ->
      `Assoc [
        ("type", `String "Cancel");
        ("order_id", int64_to_json order_id);
      ]

(* Event deserialization *)
let json_to_event json =
  match json with
  | `Assoc fields ->
      (let event_type = List.assoc "type" fields in
       match event_type with
       | `String "SubmitLimit" ->
           let side = json_to_side (List.assoc "side" fields) in
           let price = json_to_int64 (List.assoc "price" fields) in
           let quantity = json_to_int64 (List.assoc "quantity" fields) in
           let time_in_force = json_to_tif (List.assoc "time_in_force" fields) in
           Replay.SubmitLimit { side; price; quantity; time_in_force }
       | `String "SubmitMarket" ->
           let side = json_to_side (List.assoc "side" fields) in
           let quantity = json_to_int64 (List.assoc "quantity" fields) in
           Replay.SubmitMarket { side; quantity }
       | `String "Cancel" ->
           let order_id = json_to_int64 (List.assoc "order_id" fields) in
           Replay.Cancel { order_id }
       | _ -> invalid_arg "invalid event type")
  | _ -> invalid_arg "invalid event format"

(* Trade serialization *)
let trade_to_json trade =
  `Assoc [
    ("id", int64_to_json trade.Matching.id);
    ("price", int64_to_json trade.Matching.price);
    ("quantity", int64_to_json trade.Matching.quantity);
    ("aggressor_order_id", int64_to_json trade.Matching.aggressor_order_id);
    ("passive_order_id", int64_to_json trade.Matching.passive_order_id);
    ("aggressor_side", side_to_json trade.Matching.aggressor_side);
    ("timestamp", int64_to_json trade.Matching.timestamp);
  ]

(* Trade deserialization *)
let json_to_trade json =
  match json with
  | `Assoc fields ->
      let id = json_to_int64 (List.assoc "id" fields) in
      let price = json_to_int64 (List.assoc "price" fields) in
      let quantity = json_to_int64 (List.assoc "quantity" fields) in
      let aggressor_order_id = json_to_int64 (List.assoc "aggressor_order_id" fields) in
      let passive_order_id = json_to_int64 (List.assoc "passive_order_id" fields) in
      let aggressor_side = json_to_side (List.assoc "aggressor_side" fields) in
      let timestamp = json_to_int64 (List.assoc "timestamp" fields) in
      Matching.create_trade ~id ~price ~quantity ~aggressor_order_id ~passive_order_id ~aggressor_side ~timestamp
  | _ -> invalid_arg "invalid trade format"

(* JSONL parsing *)
let from_string = from_string
let parse_jsonl_line line =
  try Some (json_to_event (from_string line))
  with _ -> None

(* JSONL serialization *)
let to_string = to_string
let event_to_jsonl_string event =
  to_string (event_to_json event)

let trade_to_jsonl_string trade =
  to_string (trade_to_json trade)

(* Parse entire JSONL file *)
let parse_jsonl_file filename =
  let channel = open_in filename in
  let rec read_lines acc =
    try
      let line = input_line channel in
      if String.trim line = "" then
        read_lines acc
      else
        match parse_jsonl_line line with
        | Some event -> read_lines (event :: acc)
        | None -> read_lines acc
    with End_of_file -> List.rev acc
  in
  let events = read_lines [] in
  close_in channel;
  events

(* Write events to JSONL file *)
let write_jsonl_file filename events =
  let channel = open_out filename in
  List.iter (fun event ->
    output_string channel (event_to_jsonl_string event);
    output_string channel "\n"
  ) events;
  close_out channel

(* Write trades to JSONL file *)
let write_trades_jsonl filename trades =
  let channel = open_out filename in
  List.iter (fun trade ->
    output_string channel (trade_to_jsonl_string trade);
    output_string channel "\n"
  ) trades;
  close_out channel