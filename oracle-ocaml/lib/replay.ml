(* Event types for replay *)
type event =
  | SubmitLimit of {
      side: Side.side;
      price: Price.price;
      quantity: Order.quantity;
      time_in_force: Order.time_in_force;
    }
  | SubmitMarket of {
      side: Side.side;
      quantity: Order.quantity;
    }
  | Cancel of {
      order_id: Order.order_id;
    }

(* Replay events through the book *)
let replay_events events =
  let book = Book.create () in
  let trades = ref [] in
  List.iter (fun event ->
    match event with
    | SubmitLimit { side; price; quantity; time_in_force } ->
        let order_id = Book.next_order_id book in
        let timestamp = Book.next_timestamp book in
        let order = Order.create ~id:order_id ~side ~price ~quantity ~timestamp ~time_in_force in
        Book.store_order book order;
        (* For now, just add to book without matching - will implement full matching later *)
        ()
    | SubmitMarket { side = _side; quantity = _quantity } ->
        (* Market orders would match immediately - simplified for now *)
        ()
    | Cancel { order_id = _order_id } ->
        (* Cancel logic - simplified for now *)
        ()
  ) events;
  !trades
