(* Order book using sorted association lists for price levels *)
type price_level = {
  price: Price.price;
  quantity: Order.quantity;
  orders: Order.order_id list;  (* FIFO queue at this price level *)
}

type book = {
  mutable bids: price_level list;  (* Sorted descending: best = highest price *)
  mutable asks: price_level list;  (* Sorted ascending: best = lowest price *)
  orders: (Order.order_id, Order.order) Hashtbl.t;  (* Central order index *)
  mutable next_order_id: Order.order_id;
  mutable next_trade_id: Order.trade_id;
  mutable next_timestamp: Order.timestamp;
}

(* Create a new empty order book *)
let create () =
  {
    bids = [];
    asks = [];
    orders = Hashtbl.create 100;
    next_order_id = 1L;
    next_trade_id = 1L;
    next_timestamp = 1L;
  }

(* Generate the next order ID *)
let next_order_id book =
  let id = book.next_order_id in
  book.next_order_id <- Int64.succ book.next_order_id;
  id

(* Generate the next trade ID *)
let next_trade_id book =
  let id = book.next_trade_id in
  book.next_trade_id <- Int64.succ book.next_trade_id;
  id

(* Generate the next timestamp *)
let next_timestamp book =
  let ts = book.next_timestamp in
  book.next_timestamp <- Int64.succ book.next_timestamp;
  ts

(* Get an order by ID *)
let get_order book id =
  try Some (Hashtbl.find book.orders id)
  with Not_found -> None

(* Store an order in the central index *)
let store_order book order =
  Hashtbl.replace book.orders order.Order.id order

(* Helper: insert price level maintaining sort order *)
let insert_bid bids level =
  (* Bids sorted descending: higher prices first *)
  let rec insert = function
    | [] -> [level]
    | head :: tail when level.price > head.price -> level :: head :: tail
    | head :: tail when level.price = head.price ->
        (* Merge into existing level *)
        { head with quantity = Int64.add head.quantity level.quantity; orders = head.orders @ level.orders } :: tail
    | head :: tail -> head :: insert tail
  in
  insert bids

let insert_ask asks level =
  (* Asks sorted ascending: lower prices first *)
  let rec insert = function
    | [] -> [level]
    | head :: tail when level.price < head.price -> level :: head :: tail
    | head :: tail when level.price = head.price ->
        (* Merge into existing level *)
        { head with quantity = Int64.add head.quantity level.quantity; orders = head.orders @ level.orders } :: tail
    | head :: tail -> head :: insert tail
  in
  insert asks

(* Helper: remove or reduce quantity at a price level *)
let remove_bid bids price qty_to_remove =
  let rec remove = function
    | [] -> []
    | head :: tail when head.price = price ->
        let new_qty = Int64.sub head.quantity qty_to_remove in
        if new_qty <= 0L then
          tail  (* Remove level entirely *)
        else
          { head with quantity = new_qty } :: tail
    | head :: tail -> head :: remove tail
  in
  remove bids

let remove_ask asks price qty_to_remove =
  let rec remove = function
    | [] -> []
    | head :: tail when head.price = price ->
        let new_qty = Int64.sub head.quantity qty_to_remove in
        if new_qty <= 0L then
          tail  (* Remove level entirely *)
        else
          { head with quantity = new_qty } :: tail
    | head :: tail -> head :: remove tail
  in
  remove asks

(* Get best bid (highest price) *)
let best_bid book =
  match book.bids with
  | [] -> None
  | level :: _ -> Some level.price

(* Get best ask (lowest price) *)
let best_ask book =
  match book.asks with
  | [] -> None
  | level :: _ -> Some level.price

(* Get best bid/ask as a pair *)
let best_bid_ask book =
  (best_bid book, best_ask book)

(* Check if book is empty *)
let is_empty book =
  book.bids = [] && book.asks = []

(* Count active orders *)
let active_order_count book =
  Hashtbl.fold (fun _ order acc ->
    if Order.is_active order then acc + 1 else acc
  ) book.orders 0
