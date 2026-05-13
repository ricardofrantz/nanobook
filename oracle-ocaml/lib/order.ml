(* Time-in-force: controls order lifetime and partial fill behavior *)
type time_in_force = GTC | IOC | FOK

let tif_can_rest = function
  | GTC -> true
  | IOC -> false
  | FOK -> false

let tif_allows_partial = function
  | GTC -> true
  | IOC -> true
  | FOK -> false

let tif_to_string = function
  | GTC -> "GTC"
  | IOC -> "IOC"
  | FOK -> "FOK"

(* Order status in its lifecycle *)
type order_status = New | PartiallyFilled | Filled | Cancelled

let status_is_active = function
  | New | PartiallyFilled -> true
  | Filled | Cancelled -> false

let status_is_terminal = function
  | Filled | Cancelled -> true
  | New | PartiallyFilled -> false

(* Opaque identifier for the party that submitted an order *)
type order_owner = int

(* Core types *)
type quantity = int64
type order_id = int64
type trade_id = int64
type timestamp = int64

(* An order in the order book *)
type order = {
  id: order_id;
  side: Side.side;
  price: Price.price;
  original_quantity: quantity;
  remaining_quantity: quantity;
  filled_quantity: quantity;
  timestamp: timestamp;
  time_in_force: time_in_force;
  status: order_status;
  owner: order_owner option;
  position_in_level: int;
}

(* Create a new order *)
let create ~id ~side ~price ~quantity ~timestamp ~time_in_force =
  {
    id;
    side;
    price;
    original_quantity = quantity;
    remaining_quantity = quantity;
    filled_quantity = 0L;
    timestamp;
    time_in_force;
    status = New;
    owner = None;
    position_in_level = 0;
  }

(* Attach an owner for self-trade prevention *)
let with_owner order owner =
  { order with owner = Some owner }

(* Returns true if the order can still be filled or cancelled *)
let is_active order = status_is_active order.status

(* Fill the order by the given quantity *)
let fill order quantity =
  if quantity > order.remaining_quantity then
    invalid_arg "fill quantity exceeds remaining";
  let remaining = Int64.sub order.remaining_quantity quantity in
  let filled = Int64.add order.filled_quantity quantity in
  let status = if remaining = 0L then Filled else PartiallyFilled in
  { order with remaining_quantity = remaining; filled_quantity = filled; status }

(* Cancel the order *)
let cancel order =
  if not (is_active order) then
    invalid_arg "cannot cancel order in terminal state";
  let cancelled = order.remaining_quantity in
  { order with remaining_quantity = 0L; status = Cancelled }, cancelled
