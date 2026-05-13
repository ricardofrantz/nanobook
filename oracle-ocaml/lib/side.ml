(* Order side: Buy or Sell *)
type side = Buy | Sell

(* Returns the opposite side *)
let opposite = function
  | Buy -> Sell
  | Sell -> Buy

(* Convert to string representation *)
let to_string = function
  | Buy -> "BUY"
  | Sell -> "SELL"
