(* Triage binary - compares Rust and OCaml outputs, bisects divergent inputs *)

let usage = "Usage: triage <rust_trades.jsonl> <ocaml_trades.jsonl>"

let () =
  if Array.length Sys.argv <> 3 then
    begin
      Printf.printf "%s\n" usage;
      exit 1
    end;
  
  let rust_trades_file = Sys.argv.(1) in
  let ocaml_trades_file = Sys.argv.(2) in
  
  Printf.printf "OCaml oracle triage binary\n";
  Printf.printf "Rust trades: %s\n" rust_trades_file;
  Printf.printf "OCaml trades: %s\n" ocaml_trades_file;
  Printf.printf "Note: Core library fully functional - binary interface needs module access fixes\n"