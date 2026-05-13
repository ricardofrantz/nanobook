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

(* Calculate total available quantity at crossing prices *)
let calculate_available_liquidity book incoming_side incoming_price =
  (* For a BUY order, check ASK side (sells). For a SELL order, check BID side (buys). *)
  let levels = 
    match incoming_side with
    | Side.Buy -> book.Book.asks
    | Side.Sell -> book.Book.bids
  in
  let rec sum_liquidity acc = function
    | [] -> acc
    | level :: rest ->
        if prices_cross incoming_side incoming_price level.Book.price then
          sum_liquidity (Int64.add acc level.Book.quantity) rest
        else
          acc  (* Price doesn't cross, stop here *)
  in
  sum_liquidity 0L levels

(* Match an incoming order against the book *)
let rec match_order book incoming policy =
  let result = ref { trades = []; remaining_quantity = incoming.Order.remaining_quantity; stp_cancelled = false } in
  let incoming_ref = ref incoming in
  let continue_matching = ref true in
  
  (* Check FOK: if order cannot be fully filled, cancel without trades *)
  if incoming.Order.time_in_force = Order.FOK then
    begin
      let available = calculate_available_liquidity book incoming.Order.side incoming.Order.price in
      if available < incoming.Order.remaining_quantity then
        begin
          (* Cannot fill fully - cancel without trades *)
          incoming_ref := fst (Order.cancel incoming);
          result := { !result with remaining_quantity = 0L; stp_cancelled = true };
          continue_matching := false
        end
    end;
  
  (* Match until no more crosses or order is filled *)
  while !incoming_ref.Order.remaining_quantity > 0L && !continue_matching do
    (* Get the best price on the opposite side *)
    let opposite = opposite_side !incoming_ref.Order.side in
    let best_price = 
      match opposite with
      | Side.Buy -> Book.best_bid book
      | Side.Sell -> Book.best_ask book
    in
    
    match best_price with
    | None -> 
        (* No liquidity - stop matching *)
        continue_matching := false
    | Some resting_price ->
        if not (prices_cross !incoming_ref.Order.side !incoming_ref.Order.price resting_price) then
          (* No match at this price - stop matching *)
          continue_matching := false
        else
          (* Match against orders at this price level *)
          match_at_price book incoming_ref resting_price policy result
  done;
  
  { !result with remaining_quantity = !incoming_ref.Order.remaining_quantity }

(* Match at a specific price level *)
and match_at_price book incoming_ref price policy result =
  (* Get the price level from the appropriate side *)
  let levels = 
    match !incoming_ref.Order.side with
    | Side.Buy -> book.Book.asks
    | Side.Sell -> book.Book.bids
  in
  
  (* Find the level at the given price *)
  let level_opt = List.find_opt (fun l -> l.Book.price = price) levels in
  
  match level_opt with
  | None -> ()  (* Level not found *)
  | Some level ->
      (* Process orders at this price level FIFO *)
      let rec process_orders order_ids =
        match order_ids with
        | [] -> ()  (* No more orders at this level *)
        | _order_id :: _rest when !incoming_ref.Order.remaining_quantity = 0L -> ()  (* Incoming filled *)
        | order_id :: rest ->
            match Book.get_order book order_id with
            | None -> process_orders rest  (* Orphaned order ID - skip *)
            | Some resting ->
                if not (Order.is_active resting) then
                  process_orders rest  (* Resting order not active - skip *)
                else
                  let resting_qty = resting.Order.remaining_quantity in
                  let incoming_qty = !incoming_ref.Order.remaining_quantity in
                  
                  (* Check STP conflict *)
                  if stp_conflict !incoming_ref.Order.owner resting.Order.owner policy then
                    begin
                      match policy with
                      | Off -> ()  (* Should not happen due to stp_conflict check *)
                      | CancelNewest ->
                          (* Cancel incoming remainder; leave resting intact *)
                          let new_incoming = Order.fill !incoming_ref incoming_qty in
                          let cancelled_incoming = fst (Order.cancel new_incoming) in
                          incoming_ref := cancelled_incoming;
                          result := { !result with stp_cancelled = true }
                      | CancelOldest ->
                          (* Cancel resting; incoming continues matching *)
                          let cancelled_resting = fst (Order.cancel resting) in
                          Book.store_order book cancelled_resting;
                          (* Remove from level *)
                          (match !incoming_ref.Order.side with
                           | Side.Buy -> book.Book.asks <- Book.remove_ask book.Book.asks price resting_qty
                           | Side.Sell -> book.Book.bids <- Book.remove_bid book.Book.bids price resting_qty);
                          process_orders rest
                      | DecrementAndCancel ->
                          if incoming_qty < resting_qty then
                            begin
                              (* Smaller = incoming: cancel incoming, leave resting *)
                              let new_incoming = Order.fill !incoming_ref incoming_qty in
                              let cancelled_incoming = fst (Order.cancel new_incoming) in
                              incoming_ref := cancelled_incoming;
                              result := { !result with stp_cancelled = true }
                            end
                          else
                            begin
                              (* Smaller (or equal) = resting: cancel resting, continue *)
                              let cancelled_resting = fst (Order.cancel resting) in
                              Book.store_order book cancelled_resting;
                              (* Remove from level *)
                              (match !incoming_ref.Order.side with
                               | Side.Buy -> book.Book.asks <- Book.remove_ask book.Book.asks price resting_qty
                               | Side.Sell -> book.Book.bids <- Book.remove_bid book.Book.bids price resting_qty);
                              process_orders rest
                            end
                    end
                  else
                    begin
                      (* Calculate fill quantity *)
                      let qty = fill_qty incoming_qty resting_qty in
                      
                      (* Create the trade *)
                      let trade = create_trade
                        ~id:(Book.next_trade_id book)
                        ~price  (* Trade at resting order's price *)
                        ~quantity:qty
                        ~aggressor_order_id:!incoming_ref.Order.id
                        ~passive_order_id:resting.Order.id
                        ~aggressor_side:!incoming_ref.Order.side
                        ~timestamp:(Book.next_timestamp book) in
                      
                      result := { !result with trades = !result.trades @ [trade] };
                      
                      (* Update the incoming order *)
                      incoming_ref := Order.fill !incoming_ref qty;
                      
                      (* Update the resting order *)
                      let new_resting = Order.fill resting qty in
                      Book.store_order book new_resting;
                      
                      (* Update the price level quantity *)
                      if new_resting.Order.remaining_quantity = 0L then
                        begin
                          (* Remove fully filled order from level *)
                          (match !incoming_ref.Order.side with
                           | Side.Buy -> book.Book.asks <- Book.remove_ask book.Book.asks price qty
                           | Side.Sell -> book.Book.bids <- Book.remove_bid book.Book.bids price qty)
                        end
                      else
                        begin
                          (* Just decrease the level's quantity *)
                          (match !incoming_ref.Order.side with
                           | Side.Buy -> book.Book.asks <- Book.remove_ask book.Book.asks price qty
                           | Side.Sell -> book.Book.bids <- Book.remove_bid book.Book.bids price qty)
                        end;
                      
                      (* Continue with next order if incoming still has quantity *)
                      if !incoming_ref.Order.remaining_quantity > 0L then
                        process_orders rest
                    end
      in
      
      process_orders level.Book.orders
