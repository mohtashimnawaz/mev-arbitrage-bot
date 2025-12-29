use crate::data::Quote;

/// Simple scanner that keeps a sliding window of recent prices for a pair and
/// signals an "opportunity" when the latest price deviates from the simple
/// moving average by more than a configured factor.
pub struct Scanner {
    window_size: usize,
    threshold_pct: f64,
    prices: Vec<f64>,
}

impl Scanner {
    pub fn new(window_size: usize, threshold_pct: f64) -> Self {
        Self { window_size, threshold_pct, prices: Vec::with_capacity(window_size) }
    }

    /// Process a new quote; returns Some(opportunity_description) if a
    /// deviation is detected.
    pub fn process_quote(&mut self, q: &Quote) -> Option<String> {
        if self.prices.len() == self.window_size {
            self.prices.remove(0);
        }
        self.prices.push(q.price);

        if self.prices.len() < self.window_size {
            return None;
        }

        let avg: f64 = self.prices.iter().sum::<f64>() / (self.prices.len() as f64);
        let pct = (q.price - avg) / avg;
        if pct.abs() >= self.threshold_pct {
            Some(format!("opportunity:{} price {:.4} avg {:.4} pct {:+.3}%", q.pair, q.price, avg, pct * 100.0))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Quote;

    #[test]
    fn detects_positive_deviation() {
        let mut s = Scanner::new(3, 0.02);
        let qs = vec![100.0, 101.0, 100.5];
        for p in qs {
            let q = Quote { pair: "ETH/USDC".to_string(), price: p, timestamp_ms: 0 };
            let _ = s.process_quote(&q);
        }
        let q = Quote { pair: "ETH/USDC".to_string(), price: 104.0, timestamp_ms: 0 };
        let res = s.process_quote(&q);
        assert!(res.is_some());
    }

    #[test]
    fn ignores_small_fluctuations() {
        let mut s = Scanner::new(3, 0.05);
        let qs = vec![100.0, 101.0, 100.5];
        for p in qs {
            let q = Quote { pair: "ETH/USDC".to_string(), price: p, timestamp_ms: 0 };
            let _ = s.process_quote(&q);
        }
        let q = Quote { pair: "ETH/USDC".to_string(), price: 102.0, timestamp_ms: 0 };
        let res = s.process_quote(&q);
        assert!(res.is_none());
    }
}
