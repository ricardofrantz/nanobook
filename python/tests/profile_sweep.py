import nanobook
import time
import random

def generate_price_series(n_bars, n_stocks):
    symbols = [f"S{i:03}" for i in range(n_stocks)]
    series = []
    prices = [10000] * n_stocks
    for _ in range(n_bars):
        bar = []
        for i in range(n_stocks):
            prices[i] = int(prices[i] * (1 + random.uniform(-0.02, 0.02)))
            bar.append((symbols[i], prices[i]))
        series.append(bar)
    return series

def profile_sweep():
    n_bars = 10000
    n_stocks = 100
    n_params = 10
    
    print(f"Generating {n_bars} bars for {n_stocks} stocks...")
    start_gen = time.perf_counter()
    price_series = generate_price_series(n_bars, n_stocks)
    end_gen = time.perf_counter()
    print(f"Generation time: {end_gen - start_gen:.4f}s")
    
    print(f"Starting sweep with {n_params} params...")
    start = time.perf_counter()
    results = nanobook.sweep_equal_weight(
        n_params=n_params,
        price_series=price_series,
        initial_cash=1_000_000_00,
        periods_per_year=252.0,
        risk_free=0.0
    )
    end = time.perf_counter()
    
    total_time = end - start
    print(f"Total sweep time: {total_time:.4f}s")
    print(f"Time per backtest: {total_time/n_params*1000:.4f}ms")

if __name__ == "__main__":
    profile_sweep()
