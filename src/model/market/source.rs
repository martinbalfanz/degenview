use serde::{Deserialize, Serialize};

/// A market data provider. The symbol alone never identifies a market: `BTCUSDT`
/// on Binance and Bybit (the same name on both: they are still two markets), `BTC-USD` on Coinbase, `BTC-USDT` on OKX and `BTC/USD` on Kraken `BTC_USDT` on Gate.io and `BTCUSDT` on MEXC are different
/// instruments.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    Binance,
    Coinbase,
    Okx,
    Kraken,
    Bybit,
    Gate,
    Mexc,
    CoinGecko,
    DexScreener,
    Alpaca,
    Yahoo,
    Polymarket,
    Kalshi,
    CoinMarketCap,
}

/// Supported non-US Yahoo exchange profiles. The suffix is only a candidate;
/// chart and quote metadata must agree before prices are accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YahooMarket {
    Xetra,
    Paris,
    Amsterdam,
    Brussels,
    Milan,
    Madrid,
    Zurich,
    London,
    Toronto,
}

impl YahooMarket {
    pub fn for_symbol(symbol: &str) -> Option<Self> {
        let upper = symbol.trim().to_ascii_uppercase();
        match upper.as_str() {
            "^GDAXI" => return Some(Self::Xetra),
            "^FCHI" => return Some(Self::Paris),
            "^AEX" => return Some(Self::Amsterdam),
            "^BFX" => return Some(Self::Brussels),
            "FTSEMIB.MI" => return Some(Self::Milan),
            "^IBEX" => return Some(Self::Madrid),
            "^SSMI" => return Some(Self::Zurich),
            "^FTSE" => return Some(Self::London),
            "^GSPTSE" => return Some(Self::Toronto),
            _ => {}
        }
        let (root, suffix) = upper.rsplit_once('.')?;
        if root.is_empty() || !root.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        Some(match suffix {
            "DE" => Self::Xetra,
            "PA" => Self::Paris,
            "AS" => Self::Amsterdam,
            "BR" => Self::Brussels,
            "MI" => Self::Milan,
            "MC" => Self::Madrid,
            "SW" => Self::Zurich,
            "L" => Self::London,
            "TO" => Self::Toronto,
            _ => return None,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Xetra => "Xetra",
            Self::Paris => "Euronext Paris",
            Self::Amsterdam => "Euronext Amsterdam",
            Self::Brussels => "Euronext Brussels",
            Self::Milan => "Euronext Milan",
            Self::Madrid => "Madrid",
            Self::Zurich => "SIX Swiss Exchange",
            Self::London => "London Stock Exchange",
            Self::Toronto => "Toronto Stock Exchange",
        }
    }

    pub const fn exchange(self) -> &'static str {
        match self {
            Self::Xetra => "GER",
            Self::Paris => "PAR",
            Self::Amsterdam => "AMS",
            Self::Brussels => "BRU",
            Self::Milan => "MIL",
            Self::Madrid => "MCE",
            Self::Zurich => "EBS",
            Self::London => "LSE",
            Self::Toronto => "TOR",
        }
    }

    pub const fn currency(self) -> &'static str {
        match self {
            Self::London => "GBP",
            Self::Zurich => "CHF",
            Self::Toronto => "CAD",
            _ => "EUR",
        }
    }

    pub const fn timezone(self) -> &'static str {
        match self {
            Self::Xetra => "Europe/Berlin",
            Self::Paris => "Europe/Paris",
            Self::Amsterdam => "Europe/Amsterdam",
            Self::Brussels => "Europe/Brussels",
            Self::Milan => "Europe/Rome",
            Self::Madrid => "Europe/Madrid",
            Self::Zurich => "Europe/Zurich",
            Self::London => "Europe/London",
            Self::Toronto => "America/Toronto",
        }
    }

    /// Core weekday session, excluding auctions and special/holiday hours.
    pub const fn regular_session(self) -> &'static str {
        match self {
            Self::London => "0800-1630:23456",
            Self::Toronto => "0930-1600:23456",
            Self::Zurich => "0900-1720:23456",
            _ => "0900-1730:23456",
        }
    }

    /// Scale a vendor price to the profile's displayed currency. GBp is
    /// pence, not pounds. Do not case-fold currency units or accept other
    /// currency listings on the same exchange.
    pub fn price_scale(self, vendor_currency: &str) -> Option<f64> {
        if self == Self::London && matches!(vendor_currency, "GBp" | "GBX") {
            Some(0.01)
        } else if vendor_currency == self.currency() {
            Some(1.0)
        } else {
            None
        }
    }
}

/// A deliberately limited set of Yahoo currency pairs and rolling futures.
/// These are charting instruments, not executable contracts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YahooDerivative {
    Forex { base: &'static str, quote: &'static str },
    Future { root: &'static str, exchange: &'static str },
}

impl YahooDerivative {
    pub fn for_symbol(symbol: &str) -> Option<Self> {
        let upper = symbol.trim().to_ascii_uppercase();
        let (base, quote) = match upper.as_str() {
            "EURUSD=X" => ("EUR", "USD"),
            "GBPUSD=X" => ("GBP", "USD"),
            "USDJPY=X" | "JPY=X" => ("USD", "JPY"),
            "USDCHF=X" | "CHF=X" => ("USD", "CHF"),
            "USDCAD=X" | "CAD=X" => ("USD", "CAD"),
            "AUDUSD=X" => ("AUD", "USD"),
            "NZDUSD=X" => ("NZD", "USD"),
            "EURGBP=X" => ("EUR", "GBP"),
            "EURJPY=X" => ("EUR", "JPY"),
            "ES=F" => return Some(Self::Future { root: "ES", exchange: "CME" }),
            "NQ=F" => return Some(Self::Future { root: "NQ", exchange: "CME" }),
            "GC=F" => return Some(Self::Future { root: "GC", exchange: "CMX" }),
            "CL=F" => return Some(Self::Future { root: "CL", exchange: "NYM" }),
            _ => return None,
        };
        Some(Self::Forex { base, quote })
    }

    pub const fn kind(self) -> &'static str {
        match self {
            Self::Forex { .. } => "forex",
            Self::Future { .. } => "futures",
        }
    }

    pub const fn currency(self) -> &'static str {
        match self {
            Self::Forex { quote, .. } => quote,
            Self::Future { .. } => "USD",
        }
    }

    pub const fn base_currency(self) -> Option<&'static str> {
        match self {
            Self::Forex { base, .. } => Some(base),
            Self::Future { .. } => None,
        }
    }

    pub const fn has_volume(self) -> bool {
        matches!(self, Self::Future { .. })
    }

    /// Session conventions used for grouping bars (not a holiday calendar).
    pub const fn timezone(self) -> &'static str {
        match self {
            Self::Forex { .. } => "America/New_York",
            Self::Future { .. } => "America/Chicago",
        }
    }

    pub const fn regular_session(self) -> &'static str {
        match self {
            Self::Forex { .. } => "1700-2400:12345,0000-1700:23456",
            Self::Future { .. } => "1700-1600:12345",
        }
    }

    /// Date of the trading session, with the evening assigned to the next day.
    /// Return a UTC-midnight date label, not the actual session open instant.
    pub fn trading_day_ms(self, timestamp_ms: i64) -> i64 {
        let offset = crate::pine::time::zone_offset_secs(Some(self.timezone()), timestamp_ms);
        let local = timestamp_ms + offset * 1_000;
        (local + 7 * 3_600_000).div_euclid(86_400_000) * 86_400_000
    }
}

impl DataSource {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Binance => "Binance",
            Self::Coinbase => "Coinbase",
            Self::Okx => "OKX",
            Self::Kraken => "Kraken",
            Self::Bybit => "Bybit",
            Self::Gate => "Gate.io",
            Self::Mexc => "MEXC",
            Self::CoinGecko => "CoinGecko",
            Self::DexScreener => "DEXScreener",
            Self::Alpaca => "Alpaca (IEX)",
            Self::Yahoo => "Yahoo Finance",
            Self::Polymarket => "Polymarket",
            Self::Kalshi => "Kalshi",
            Self::CoinMarketCap => "CoinMarketCap",
        }
    }

    /// Upper-case provider prefix in the import/export text format.
    pub fn watchlist_slug(self) -> &'static str {
        match self {
            Self::Binance => "BINANCE",
            Self::Coinbase => "COINBASE",
            Self::Okx => "OKX",
            Self::Kraken => "KRAKEN",
            Self::Bybit => "BYBIT",
            Self::Gate => "GATEIO",
            Self::Mexc => "MEXC",
            Self::CoinGecko => "COINGECKO",
            Self::DexScreener => "DEXSCREENER",
            Self::Alpaca => "ALPACA",
            Self::Yahoo => "YAHOO",
            Self::Polymarket => "POLYMARKET",
            Self::Kalshi => "KALSHI",
            Self::CoinMarketCap => "CMC",
        }
    }

    /// The source a `watchlist_slug` names, in any case.
    pub fn from_watchlist_slug(slug: &str) -> Option<Self> {
        const ALL: [DataSource; 14] = [
            DataSource::Binance,
            DataSource::Coinbase,
            DataSource::Okx,
            DataSource::Kraken,
            DataSource::Bybit,
            DataSource::Gate,
            DataSource::Mexc,
            DataSource::CoinGecko,
            DataSource::DexScreener,
            DataSource::Alpaca,
            DataSource::Yahoo,
            DataSource::Polymarket,
            DataSource::Kalshi,
            DataSource::CoinMarketCap,
        ];
        ALL.into_iter()
            .find(|source| source.watchlist_slug().eq_ignore_ascii_case(slug))
    }

    /// The sources whose forming candle a chart takes from a socket instead of waiting for the
    /// next refresh.
    pub const CANDLE_STREAMS: [DataSource; 6] = [
        DataSource::Binance,
        DataSource::Okx,
        DataSource::Kraken,
        DataSource::Bybit,
        DataSource::Gate,
        DataSource::Mexc,
    ];

    /// The sources whose watchlist prices come from a shared ticker socket, REST filling in.
    pub const QUOTE_STREAMS: [DataSource; 6] = [
        DataSource::Coinbase,
        DataSource::Okx,
        DataSource::Kraken,
        DataSource::Bybit,
        DataSource::Gate,
        DataSource::Yahoo,
    ];

    /// First Yahoo streaming scope: USD equities and ETFs, not indexes, FX,
    /// futures or non-US listings. REST metadata still has to validate them.
    pub fn yahoo_stream_symbol(self, symbol: &str) -> bool {
        self == Self::Yahoo && !symbol.is_empty() && symbol.len() <= 12
            && symbol.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }

    /// Whether watchlist prices of this source come from a ticker socket.
    pub fn streams_quotes(self) -> bool {
        Self::QUOTE_STREAMS.contains(&self)
    }

    /// Whether this source quotes probabilities in 0…1, charted as lines.
    pub fn is_prediction_market(self) -> bool {
        matches!(self, Self::Polymarket | Self::Kalshi)
    }

    /// Providers listed in the stock/index picker (not a session or currency test).
    pub fn is_us_equity_source(self) -> bool {
        matches!(self, Self::Alpaca | Self::Yahoo)
    }

    /// Index tickers supported by Yahoo; validated against vendor metadata.
    pub fn is_yahoo_index(self, symbol: &str) -> bool {
        self == Self::Yahoo
            && matches!(
                symbol.trim().to_ascii_uppercase().as_str(),
                "^GSPC" | "^IXIC" | "^NDX" | "^DJI" | "^RUT" | "^VIX" | "^GDAXI"
                    | "^FCHI" | "^AEX" | "^BFX" | "FTSEMIB.MI" | "^IBEX" | "^SSMI" | "^FTSE" | "^GSPTSE"
            )
    }

    pub fn yahoo_market(self, symbol: &str) -> Option<YahooMarket> {
        if self == Self::Yahoo {
            YahooMarket::for_symbol(symbol)
        } else {
            None
        }
    }

    pub fn yahoo_derivative(self, symbol: &str) -> Option<YahooDerivative> {
        if self == Self::Yahoo {
            YahooDerivative::for_symbol(symbol)
        } else {
            None
        }
    }

    pub fn yahoo_portfolio_excluded(self, symbol: &str) -> bool {
        self.is_yahoo_index(symbol) || self.yahoo_market(symbol).is_some()
            || self.yahoo_derivative(symbol).is_some()
    }

    /// Currency after normalizing the vendor's quote units.
    pub fn equity_currency(self, symbol: &str) -> Option<&'static str> {
        if let Some(derivative) = self.yahoo_derivative(symbol) {
            Some(derivative.currency())
        } else if let Some(market) = self.yahoo_market(symbol) {
            Some(market.currency())
        } else if self.is_us_equity_source() {
            Some("USD")
        } else {
            None
        }
    }

    /// Index levels are not traded shares. Do not present index feed volume
    /// (which may be absent or an aggregate) as ordinary share volume.
    pub fn provides_market_volume(self, symbol: &str) -> bool {
        self.provides_volume() && !self.is_yahoo_index(symbol)
            && self.yahoo_derivative(symbol).is_none_or(|market| market.has_volume())
    }

    /// Whether this source reports per-candle turnover for the volume bars to draw. CoinGecko's
    /// OHLC endpoint has no volume column, and the line charts report none at all.
    pub fn provides_volume(self) -> bool {
        matches!(
            self,
            Self::Binance
                | Self::Coinbase
                | Self::Okx
                | Self::Kraken
                | Self::Bybit
                | Self::Gate
                | Self::Mexc
                | Self::DexScreener
                | Self::Alpaca
                | Self::Yahoo
        )
    }

    /// Whether this source's data is drawn as a line rather than candles.
    pub fn is_line_chart(self) -> bool {
        self.is_prediction_market() || matches!(self, Self::CoinMarketCap)
    }

    /// Whether a chart of this source can carry indicators and scripts. A prediction market or a
    /// CoinMarketCap gauge is a probability or an index line with no price to study.
    pub fn takes_indicators(self) -> bool {
        !self.is_line_chart()
    }

    /// The market's name as its exchange writes it: `BTCUSDT` on Binance, `BTC-USD` on Coinbase,
    /// `AAPL` on Alpaca. Nothing is reformatted, only upper-cased where the exchange's own ids are
    /// (CoinGecko ids and Alpaca tickers). A chart, a search hit and a watchlist row all use it.
    pub fn display_symbol(self, symbol: &str) -> String {
        let raw = symbol.trim();
        match self {
            Self::Binance
            | Self::Coinbase
            | Self::Okx
            | Self::Kraken
            | Self::Bybit
            | Self::Gate
            | Self::Mexc
            | Self::CoinGecko
            | Self::Alpaca
            | Self::Yahoo => raw.to_uppercase(),
            Self::CoinMarketCap => super::cmc::title(raw).to_owned(),
            _ => raw.to_owned(),
        }
    }

    /// The asset a market trades, for artwork and tickers: `BTC` for `BTCUSDT`, `BTC-USD`,
    /// `BTC/USDT` and `BTC — Grayscale Bitcoin Mini Trust`. A symbol that names no pair is returned
    /// as it is. Never shown as a market's name: use [`Self::display_symbol`] for that.
    pub fn base_asset(self, symbol: &str) -> String {
        let upper = symbol.trim().to_uppercase();
        let head = upper.split(['/', '-', ' ', '_']).next().unwrap_or(&upper);
        match self {
            Self::Yahoo => upper.split(" — ").next().unwrap_or(&upper),
            // A one-letter "base" is a bare wrapped asset (WBTC, WETH), not a pair.
            Self::Binance | Self::Bybit => split_binance_pair(head)
                .map(|(base, _)| base)
                .filter(|base| base.len() > 1)
                .unwrap_or(head),
            Self::Mexc => split_mexc_pair(head).map(|(base, _)| base).unwrap_or(head),
            _ => head,
        }
        .to_owned()
    }
}

impl DataSource {
    /// The quote asset of a pair the exchange itself spells out: `EUR` of Coinbase's `ADA-EUR`, of
    /// Binance's `ADAEUR`. None for anything that does not name one (a stock, a prediction market,
    /// an aggregator's coin id): a pair's parts are never guessed.
    pub fn quote_asset(self, symbol: &str) -> Option<String> {
        let upper = symbol.trim().to_uppercase();
        match self {
            Self::Binance | Self::Bybit => {
                split_binance_pair(&upper).map(|(_, quote)| quote.to_owned())
            }
            Self::Mexc => split_mexc_pair(&upper).map(|(_, quote)| quote.to_owned()),
            Self::Kraken => {
                let (base, quote) = upper.split_once('/')?;
                (!base.is_empty() && !quote.is_empty() && !quote.contains(['-', '/', ' ']))
                    .then(|| quote.to_owned())
            }
            Self::Gate => {
                let (base, quote) = upper.split_once('_')?;
                (!base.is_empty() && !quote.is_empty() && !quote.contains(['-', '/', ' ', '_']))
                    .then(|| quote.to_owned())
            }
            Self::Coinbase | Self::Okx => {
                let (base, quote) = upper.split_once('-')?;
                (!base.is_empty() && !quote.is_empty() && !quote.contains(['-', '/', ' ']))
                    .then(|| quote.to_owned())
            }
            _ => None,
        }
    }
}

/// Kraken's own spelling of an asset is the legacy one (`XBT`, `XDG`) in some places; its APIs take
/// the common ticker (`BTC`, `DOGE`) and refuse the legacy one.
pub fn kraken_asset(asset: &str) -> String {
    match asset.to_uppercase().as_str() {
        "XBT" => "BTC".to_owned(),
        "XDG" => "DOGE".to_owned(),
        other => other.to_owned(),
    }
}

/// Quote assets Binance pairs end in, longest first so BTCFDUSD is BTC/FDUSD instead of
/// BTCFD/USD. Matches Swift's MarketSymbol quote list.
const BINANCE_QUOTES: [&str; 16] = [
    "FDUSD", "USDT", "USDC", "BUSD", "TUSD", "USDP", "DAI", "EUR", "GBP", "TRY", "BRL", "JPY",
    "USD", "BTC", "ETH", "BNB",
];

/// Splits an upper-case Binance pair like `SOLBNB` into `("SOL", "BNB")`, or `None` when no
/// known quote ends it (a bare asset such as `SOL`).
pub fn split_binance_pair(symbol: &str) -> Option<(&str, &'static str)> {
    BINANCE_QUOTES.iter().find_map(|quote| {
        symbol
            .strip_suffix(quote)
            .filter(|base| !base.is_empty())
            .map(|base| (base, *quote))
    })
}

/// Quote assets MEXC's spot catalog ends its ids in, the dollar stablecoins first.
const MEXC_QUOTES: [&str; 9] = [
    "USDT", "USDC", "USD1", "USDE", "USDF", "EUR", "BRL", "ETH", "BTC",
];

/// How many of [`MEXC_QUOTES`] are dollar stablecoins: a one-letter base is a real asset there
/// (`SUSDT`), where before another quote it is a wrapped one (`WBTC` is not W/BTC).
const MEXC_STABLE_QUOTES: usize = 5;

/// Splits an upper-case MEXC id like `BTCUSD1` into `("BTC", "USD1")`, or `None` when no quote of
/// its catalog ends it (a bare asset such as `SOL` or `WBTC`).
pub fn split_mexc_pair(symbol: &str) -> Option<(&str, &'static str)> {
    MEXC_QUOTES.iter().enumerate().find_map(|(index, quote)| {
        symbol
            .strip_suffix(quote)
            .filter(|base| base.len() > usize::from(index >= MEXC_STABLE_QUOTES))
            .map(|base| (base, *quote))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_pair_the_exchange_spells_out_has_a_quote_and_nothing_else_does() {
        use super::DataSource::*;
        assert_eq!(Coinbase.quote_asset("ADA-EUR").as_deref(), Some("EUR"));
        assert_eq!(Okx.quote_asset("sol-usdt").as_deref(), Some("USDT"));
        assert_eq!(Kraken.quote_asset("eth/eur").as_deref(), Some("EUR"));
        assert_eq!(Gate.quote_asset("sol_usdt").as_deref(), Some("USDT"));
        assert_eq!(Gate.quote_asset("SOLUSDT"), None);
        assert_eq!(Kraken.quote_asset("ETHEUR"), None);
        // A perpetual swap's id has three parts: not a spot pair.
        assert_eq!(Okx.quote_asset("BTC-USDT-SWAP"), None);
        assert_eq!(Binance.quote_asset("adaeur").as_deref(), Some("EUR"));
        assert_eq!(Bybit.quote_asset("adaeur").as_deref(), Some("EUR"));
        assert_eq!(Mexc.quote_asset("adausdc").as_deref(), Some("USDC"));
        assert_eq!(Mexc.quote_asset("BTCUSD1").as_deref(), Some("USD1"));
        assert_eq!(Mexc.quote_asset("SUSDT").as_deref(), Some("USDT"));
        assert_eq!(Mexc.quote_asset("WBTC"), None);
        assert_eq!(Bybit.quote_asset("SOL"), None);
        assert_eq!(Binance.quote_asset("SOL"), None);
        assert_eq!(Coinbase.quote_asset("BTC"), None);
        assert_eq!(Alpaca.quote_asset("BRK-B"), None);
        assert_eq!(Kalshi.quote_asset("KXFED-23DEC"), None);
    }

    use super::*;

    #[test]
    fn markets_are_named_as_their_exchange_names_them() {
        assert_eq!(DataSource::Binance.display_symbol("btcusdt"), "BTCUSDT");
        assert_eq!(DataSource::Bybit.display_symbol("btcusdt"), "BTCUSDT");
        assert_eq!(DataSource::Binance.display_symbol("BTC"), "BTC");
        assert_eq!(DataSource::Coinbase.display_symbol("btc-usd"), "BTC-USD");
        assert_eq!(DataSource::Okx.display_symbol("btc-usdt"), "BTC-USDT");
        assert_eq!(DataSource::Kraken.display_symbol("btc/usd"), "BTC/USD");
        assert_eq!(DataSource::Gate.display_symbol("btc_usdt"), "BTC_USDT");
        assert_eq!(DataSource::Mexc.display_symbol("btcusdt"), "BTCUSDT");
        assert_eq!(DataSource::Alpaca.display_symbol(" aapl "), "AAPL");
        // A DEX pair address is an id, not a name: it is left alone.
        assert_eq!(DataSource::DexScreener.display_symbol("0xAbC"), "0xAbC");
    }

    #[test]
    fn the_base_asset_comes_out_of_any_spelling_of_a_market() {
        let base = |source: DataSource, symbol| source.base_asset(symbol);
        assert_eq!(base(DataSource::Binance, "BTCUSDT"), "BTC");
        assert_eq!(base(DataSource::Binance, "BTCFDUSD"), "BTC");
        assert_eq!(base(DataSource::Bybit, "ethusdc"), "ETH");
        assert_eq!(base(DataSource::Bybit, "WBTC"), "WBTC");
        assert_eq!(base(DataSource::Binance, "ETHBTC"), "ETH");
        assert_eq!(base(DataSource::Binance, "BTC"), "BTC");
        assert_eq!(base(DataSource::Binance, "WBTC"), "WBTC");
        assert_eq!(base(DataSource::Binance, "BTC/USDT"), "BTC");
        assert_eq!(base(DataSource::Coinbase, "btc-usd"), "BTC");
        assert_eq!(base(DataSource::Okx, "eth-usdt"), "ETH");
        assert_eq!(base(DataSource::Kraken, "sol/usd"), "SOL");
        assert_eq!(base(DataSource::Gate, "eth_usdt"), "ETH");
        assert_eq!(base(DataSource::Mexc, "solusdt"), "SOL");
        assert_eq!(base(DataSource::Mexc, "BTCUSD1"), "BTC");
        assert_eq!(base(DataSource::Mexc, "susdt"), "S");
        assert_eq!(base(DataSource::Mexc, "WBTC"), "WBTC");
        assert_eq!(base(DataSource::Gate, "BTC3L_USDT"), "BTC3L");
        assert_eq!(base(DataSource::Alpaca, "AAPL — Apple Inc."), "AAPL");
        assert_eq!(base(DataSource::DexScreener, "PEPE/WETH"), "PEPE");
    }

    #[test]
    fn only_sources_with_per_candle_turnover_provide_volume() {
        assert!(DataSource::Binance.provides_volume());
        assert!(DataSource::Alpaca.provides_volume());
        assert!(DataSource::Okx.provides_volume());
        assert!(DataSource::Kraken.provides_volume());
        assert!(DataSource::Bybit.provides_volume());
        assert!(DataSource::Gate.provides_volume());
        assert!(DataSource::Mexc.provides_volume());
        assert!(!DataSource::CoinGecko.provides_volume());
        assert!(!DataSource::Polymarket.provides_volume());
        assert!(!DataSource::CoinMarketCap.provides_volume());
    }
}

#[cfg(test)]
mod yahoo_market_tests {
    use super::*;

    #[test]
    fn exchange_profiles_are_source_qualified_and_keep_currency_units() {
        assert_eq!(DataSource::Yahoo.yahoo_market("air.pa"), Some(YahooMarket::Paris));
        assert_eq!(DataSource::Yahoo.yahoo_market("^FTSE"), Some(YahooMarket::London));
        assert_eq!(DataSource::Yahoo.equity_currency("RY.TO"), Some("CAD"));
        assert_eq!(DataSource::Yahoo.equity_currency("NESN.SW"), Some("CHF"));
        assert_eq!(DataSource::Yahoo.yahoo_market("BRK-B"), None);
        assert_eq!(DataSource::Alpaca.yahoo_market("AIR.PA"), None);
        assert_eq!(YahooMarket::London.price_scale("GBp"), Some(0.01));
        assert_eq!(YahooMarket::London.price_scale("GBX"), Some(0.01));
        assert_eq!(YahooMarket::London.price_scale("GBP"), Some(1.0));
        assert_eq!(YahooMarket::London.price_scale("USD"), None);
    }
}

#[cfg(test)]
mod yahoo_derivative_tests {
    use super::*;

    #[test]
    fn fx_pairs_and_futures_have_distinct_currency_and_volume() {
        let fx = DataSource::Yahoo.yahoo_derivative("EURJPY=X").unwrap();
        assert_eq!(fx.base_currency(), Some("EUR"));
        assert_eq!(fx.currency(), "JPY");
        assert!(!fx.has_volume());
        assert_eq!(DataSource::Yahoo.yahoo_derivative("JPY=X"), DataSource::Yahoo.yahoo_derivative("USDJPY=X"));
        assert!(DataSource::Yahoo.yahoo_derivative("ES=F").unwrap().has_volume());
        assert!(!DataSource::Yahoo.provides_market_volume("EURUSD=X"));
        assert!(DataSource::Yahoo.provides_market_volume("GC=F"));
    }
}
