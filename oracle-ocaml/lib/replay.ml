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

(* Helper: add order to appropriate side of book *)
let add_order_to_book book order =
  let level = {
    Book.price = order.Order.price;
    Book.quantity = order.Order.remaining_quantity;
    Book.orders = [order.Order.id];
  } in
  match order.Order.side with
  | Side.Buy -> book.Book.bids <- Book.insert_bid book.Book.bids level
  | Side.Sell -> book.Book.asks <- Book.insert_ask book.Book.asks level

(* Helper: remove order from book *)
let remove_order_from_book book order =
  match order.Order.side with
  | Side.Buy -> book.Book.bids <- Book.remove_bid book.Book.bids order.Order.price order.Order.remaining_quantity
  | Side.Sell -> book.Book.asks <- Book.remove_ask book.Book.asks order.Order.price order.Order.remaining_quantity

(* Replay events through the book *)
let replay_events events =
  let book = Book.create () in
  let all_trades = ref [] in
  
  List.iter (fun event ->
    match event with
    | SubmitLimit { side; price; quantity; time_in_force } ->
        let order_id = Book.next_order_id book in
        let timestamp = Book.next_timestamp book in
        let order = Order.create ~id:order_id ~side ~price ~quantity ~timestamp ~time_in_force in
        
        (* Match the order against the book *)
        let match_result = Matching.match_order book order Matching.Off in
        
        (* Store the potentially modified order *)
        Book.store_order book order;
        
        (* Collect trades *)
        all_trades := !all_trades @ match_result.trades;
        
        (* If order has remaining quantity and can rest, add to book *)
        if match_result.remaining_quantity > 0L && Order.tif_can_rest time_in_force then
          add_order_to_book book order
        else if match_result.remaining_quantity > 0L && not (Order.tif_can_rest time_in_force) then
          (* IOC/FOK remainder - cancel it *)
          let cancelled_order = fst (Order.cancel order) in
          Book.store_order book cancelled_order
          
    | SubmitMarket { side; quantity } ->
        (* Market order - match immediately at best prices *)
        let order_id = Book.next_order_id book in
        let timestamp = Book.next_timestamp book in
        (* Use max price for buy (sweep asks), min price for sell (sweep bids) *)
        let price = 
          match side with
          | Side.Buy -> Price.max  (* Will match any ask *)
          | Side.Sell -> Price.min  (* Will match any bid *)
        in
        let order = Order.create ~id:order_id ~side ~price ~quantity ~timestamp ~time_in_force:Order.IOC in
        
        (* Match the market order *)
        let match_result = Matching.match_order book order Matching.Off in
        
        (* Store the order (should be fully filled or cancelled) *)
        Book.store_order book order;
        
        (* Collect trades *)
        all_trades := !all_trades @ match_result.trades
        
    | Cancel { order_id } ->
        (* Cancel the order *)
        match Book.get_order book order_id with
        | None -> ()  (* Order not found *)
        | Some order ->
            if Order.is_active order then
              begin
                let cancelled_order, _cancelled_qty = Order.cancel order in
                Book.store_order book cancelled_order;
                (* Remove from book *)
                remove_order_from_book book cancelled_order
              end
  ) events;
  
  !all_trades
