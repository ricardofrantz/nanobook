(* Price type - fixed-point representation in smallest units (e.g., cents) *)
type price = int64

let zero = Int64.zero
let max = Int64.max_int
let min = Int64.min_int

(* Convert to display format (dollars.cents) *)
let to_string p =
  let dollars = Int64.div p 100L in
  let cents = Int64.rem (Int64.abs p) 100L in
  if p < 0L then
    Printf.sprintf "-$%Ld.%02Ld" (Int64.abs dollars) cents
  else
    Printf.sprintf "$%Ld.%02Ld" dollars cents
