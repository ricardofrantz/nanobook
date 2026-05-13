(* Self-trade prevention policy *)
type stp_policy = Off | CancelNewest | CancelOldest | DecrementAndCancel

(* Trade type *)
type trade = {
  id: Order.trade_id;
  price: Price.price;
  quantity: Order.quantity;
  aggressor_order_id: Order.order_id;
  passive_order_id: Order.order_id;
  aggressor_side: Side.side;
  timestamp: Order.timestamp;
}

(* Match result *)
type match_result = {
  trades: trade list;
  remaining_quantity: Order.quantity;
  stp_cancelled: bool;
}

(* Create a new trade *)
let create_trade ~id ~price ~quantity ~aggressor_order_id ~passive_order_id ~aggressor_side ~timestamp =
  {
    id;
    price;
    quantity;
    aggressor_order_id;
    passive_order_id;
    aggressor_side;
    timestamp;
  }

(* Check if prices cross *)
let prices_cross incoming_side incoming_price resting_price =
  match incoming_side with
  | Side.Buy -> incoming_price >= resting_price
  | Side.Sell -> incoming_price <= resting_price

(* Get opposite side *)
let opposite_side = function
  | Side.Buy -> Side.Sell
  | Side.Sell -> Side.Buy

(* Calculate fill quantity (min of both) *)
let fill_qty incoming_qty resting_qty =
  if incoming_qty <= resting_qty then incoming_qty else resting_qty

(* Check STP conflict *)
let stp_conflict incoming_owner resting_owner policy =
  match (incoming_owner, resting_owner, policy) with
  | (Some incoming, Some resting, _) when incoming = resting && policy <> Off -> true
  | _ -> false

(* Match an incoming order against the book *)
let match_order _book incoming _policy =
  (* Simplified matching for the oracle - will be expanded *)
  { trades = []; remaining_quantity = incoming.Order.remaining_quantity; stp_cancelled = false }
