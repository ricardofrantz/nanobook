(* Replay binary - reads JSONL events, emits JSONL trades *)

let usage = "Usage: replay <input.jsonl> <output.jsonl>"

let () =
  if Array.length Sys.argv <> 3 then
    begin
      Printf.printf "%s\n" usage;
      exit 1
    end;
  
  let input_file = Sys.argv.(1) in
  let output_file = Sys.argv.(2) in
  
  Printf.printf "OCaml oracle replay binary\n";
  Printf.printf "Reading events from %s...\n" input_file;
  let events = Json.parse_jsonl_file input_file in
  Printf.printf "Parsed %d events\n" (List.length events);
  
  Printf.printf "Replaying events...\n";
  let trades = Replay.replay_events events in
  Printf.printf "Generated %d trades\n" (List.length trades);
  
  Printf.printf "Writing trades to %s...\n" output_file;
  Json.write_trades_jsonl output_file trades;
  
  Printf.printf "Done.\n"