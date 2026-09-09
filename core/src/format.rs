use core::{
    fmt,
    ops::{Div, Neg},
};
use std::time::Duration;

/// A numeric prefix, either binary or decimal.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Prefix {
    /// _kilo_, 10<sup>3</sup> or 1000<sup>1</sup>.
    /// From the Greek ‘χίλιοι’ (‘chilioi’), meaning ‘thousand’.
    Kilo,

    /// _mega_, 10<sup>6</sup> or 1000<sup>2</sup>.
    /// From the Ancient Greek ‘μέγας’ (‘megas’), meaning ‘great’.
    Mega,

    /// _giga_, 10<sup>9</sup> or 1000<sup>3</sup>.
    /// From the Greek ‘γίγας’ (‘gigas’), meaning ‘giant’.
    Giga,

    /// _tera_, 10<sup>12</sup> or 1000<sup>4</sup>.
    /// From the Greek ‘τέρας’ (‘teras’), meaning ‘monster’.
    Tera,

    /// _peta_, 10<sup>15</sup> or 1000<sup>5</sup>.
    /// From the Greek ‘πέντε’ (‘pente’), meaning ‘five’.
    Peta,

    /// _exa_, 10<sup>18</sup> or 1000<sup>6</sup>.
    /// From the Greek ‘ἕξ’ (‘hex’), meaning ‘six’.
    Exa,

    /// _zetta_, 10<sup>21</sup> or 1000<sup>7</sup>.
    /// From the Latin ‘septem’, meaning ‘seven’.
    Zetta,

    /// _yotta_, 10<sup>24</sup> or 1000<sup>8</sup>.
    /// From the Green ‘οκτώ’ (‘okto’), meaning ‘eight’.
    Yotta,

    /// _kibi_, 2<sup>10</sup> or 1024<sup>1</sup>.
    /// The binary version of _kilo_.
    Kibi,

    /// _mebi_, 2<sup>20</sup> or 1024<sup>2</sup>.
    /// The binary version of _mega_.
    Mebi,

    /// _gibi_, 2<sup>30</sup> or 1024<sup>3</sup>.
    /// The binary version of _giga_.
    Gibi,

    /// _tebi_, 2<sup>40</sup> or 1024<sup>4</sup>.
    /// The binary version of _tera_.
    Tebi,

    /// _pebi_, 2<sup>50</sup> or 1024<sup>5</sup>.
    /// The binary version of _peta_.
    Pebi,

    /// _exbi_, 2<sup>60</sup> or 1024<sup>6</sup>.
    /// The binary version of _exa_.
    Exbi,
    // you can download exa binaries at https://exa.website/#installation
    /// _zebi_, 2<sup>70</sup> or 1024<sup>7</sup>.
    /// The binary version of _zetta_.
    Zebi,

    /// _yobi_, 2<sup>80</sup> or 1024<sup>8</sup>.
    /// The binary version of _yotta_.
    Yobi,
}

/// The result of trying to apply a prefix to a floating-point value.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum NumberPrefix<F> {
    /// A **standalone** value is returned when the number is too small to
    /// have any prefixes applied to it. This is commonly a special case, so
    /// is handled separately.
    Standalone(F),

    /// A **prefixed** value *is* large enough for prefixes. This holds the
    /// prefix, as well as the resulting value.
    Prefixed(Prefix, F),
}

impl<F: Amounts> NumberPrefix<F> {
    /// Formats the given floating-point number using **decimal** prefixes.
    ///
    /// This function accepts both `f32` and `f64` values. If you’re trying to
    /// format an integer, you’ll have to cast it first.
    ///
    /// # Examples
    ///
    /// ```
    /// use unit_prefix::{NumberPrefix, Prefix};
    ///
    /// assert_eq!(
    ///     NumberPrefix::decimal(1_000_000_000_f32),
    ///     NumberPrefix::Prefixed(Prefix::Giga, 1_f32)
    /// );
    /// ```
    pub fn decimal(amount: F) -> Self {
        use self::Prefix::*;
        Self::format_number(
            amount,
            Amounts::NUM_1000,
            [Kilo, Mega, Giga, Tera, Peta, Exa, Zetta, Yotta],
        )
    }

    /// Formats the given floating-point number using **binary** prefixes.
    ///
    /// This function accepts both `f32` and `f64` values. If you’re trying to
    /// format an integer, you’ll have to cast it first.
    ///
    /// # Examples
    ///
    /// ```
    /// use unit_prefix::{NumberPrefix, Prefix};
    ///
    /// assert_eq!(
    ///     NumberPrefix::binary(1_073_741_824_f64),
    ///     NumberPrefix::Prefixed(Prefix::Gibi, 1_f64)
    /// );
    /// ```
    pub fn binary(amount: F) -> Self {
        use self::Prefix::*;
        Self::format_number(
            amount,
            Amounts::NUM_1024,
            [Kibi, Mebi, Gibi, Tebi, Pebi, Exbi, Zebi, Yobi],
        )
    }

    fn format_number(mut amount: F, kilo: F, prefixes: [Prefix; 8]) -> Self {
        // For negative numbers, flip it to positive, do the processing, then
        // flip it back to negative again afterwards.
        let was_negative = if amount.is_negative() {
            amount = -amount;
            true
        } else {
            false
        };

        let mut prefix = 0;
        while amount >= kilo && prefix < 8 {
            amount = amount / kilo;
            prefix += 1;
        }

        if was_negative {
            amount = -amount;
        }

        if prefix == 0 {
            NumberPrefix::Standalone(amount)
        } else {
            NumberPrefix::Prefixed(prefixes[prefix - 1], amount)
        }
    }
}

impl fmt::Display for Prefix {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.symbol())
    }
}

impl Prefix {
    /// Returns the name in uppercase, such as “KILO”.
    ///
    /// # Examples
    ///
    /// ```
    /// use unit_prefix::Prefix;
    ///
    /// assert_eq!("GIGA", Prefix::Giga.upper());
    /// assert_eq!("GIBI", Prefix::Gibi.upper());
    /// ```
    pub fn upper(self) -> &'static str {
        use self::Prefix::*;
        match self {
            Kilo => "KILO",
            Mega => "MEGA",
            Giga => "GIGA",
            Tera => "TERA",
            Peta => "PETA",
            Exa => "EXA",
            Zetta => "ZETTA",
            Yotta => "YOTTA",
            Kibi => "KIBI",
            Mebi => "MEBI",
            Gibi => "GIBI",
            Tebi => "TEBI",
            Pebi => "PEBI",
            Exbi => "EXBI",
            Zebi => "ZEBI",
            Yobi => "YOBI",
        }
    }

    /// Returns the name with the first letter capitalised, such as “Mega”.
    ///
    /// # Examples
    ///
    /// ```
    /// use unit_prefix::Prefix;
    ///
    /// assert_eq!("Giga", Prefix::Giga.caps());
    /// assert_eq!("Gibi", Prefix::Gibi.caps());
    /// ```
    pub fn caps(self) -> &'static str {
        use self::Prefix::*;
        match self {
            Kilo => "Kilo",
            Mega => "Mega",
            Giga => "Giga",
            Tera => "Tera",
            Peta => "Peta",
            Exa => "Exa",
            Zetta => "Zetta",
            Yotta => "Yotta",
            Kibi => "Kibi",
            Mebi => "Mebi",
            Gibi => "Gibi",
            Tebi => "Tebi",
            Pebi => "Pebi",
            Exbi => "Exbi",
            Zebi => "Zebi",
            Yobi => "Yobi",
        }
    }

    /// Returns the name in lowercase, such as “giga”.
    ///
    /// # Examples
    ///
    /// ```
    /// use unit_prefix::Prefix;
    ///
    /// assert_eq!("giga", Prefix::Giga.lower());
    /// assert_eq!("gibi", Prefix::Gibi.lower());
    /// ```
    pub fn lower(self) -> &'static str {
        use self::Prefix::*;
        match self {
            Kilo => "kilo",
            Mega => "mega",
            Giga => "giga",
            Tera => "tera",
            Peta => "peta",
            Exa => "exa",
            Zetta => "zetta",
            Yotta => "yotta",
            Kibi => "kibi",
            Mebi => "mebi",
            Gibi => "gibi",
            Tebi => "tebi",
            Pebi => "pebi",
            Exbi => "exbi",
            Zebi => "zebi",
            Yobi => "yobi",
        }
    }

    /// Returns the short-hand symbol, such as “T” (for “tera”).
    ///
    /// # Examples
    ///
    /// ```
    /// use unit_prefix::Prefix;
    ///
    /// assert_eq!("G", Prefix::Giga.symbol());
    /// assert_eq!("Gi", Prefix::Gibi.symbol());
    /// ```
    pub fn symbol(self) -> &'static str {
        use self::Prefix::*;
        match self {
            Kilo => "k",
            Mega => "M",
            Giga => "G",
            Tera => "T",
            Peta => "P",
            Exa => "E",
            Zetta => "Z",
            Yotta => "Y",
            Kibi => "Ki",
            Mebi => "Mi",
            Gibi => "Gi",
            Tebi => "Ti",
            Pebi => "Pi",
            Exbi => "Ei",
            Zebi => "Zi",
            Yobi => "Yi",
        }
    }
}

/// Traits for floating-point values for both the possible multipliers. They
/// need to be Copy, have defined 1000 and 1024s, and implement a bunch of
/// operators.
pub trait Amounts: Copy + Sized + PartialOrd + Div<Output = Self> + Neg<Output = Self> {
    /// The constant representing 1000, for decimal prefixes.
    const NUM_1000: Self;

    /// The constant representing 1024, for binary prefixes.
    const NUM_1024: Self;

    /// Whether this number is negative.
    /// This is used internally.
    fn is_negative(self) -> bool;
}

impl Amounts for f32 {
    const NUM_1000: Self = 1000_f32;
    const NUM_1024: Self = 1024_f32;

    fn is_negative(self) -> bool {
        self.is_sign_negative()
    }
}

impl Amounts for f64 {
    const NUM_1000: Self = 1000_f64;
    const NUM_1024: Self = 1024_f64;

    fn is_negative(self) -> bool {
        self.is_sign_negative()
    }
}

#[cfg(test)]
mod test {
    use super::{NumberPrefix, Prefix};

    #[test]
    fn decimal_minus_one_billion() {
        assert_eq!(
            NumberPrefix::decimal(-1_000_000_000_f64),
            NumberPrefix::Prefixed(Prefix::Giga, -1f64)
        )
    }

    #[test]
    fn decimal_minus_one() {
        assert_eq!(
            NumberPrefix::decimal(-1f64),
            NumberPrefix::Standalone(-1f64)
        )
    }

    #[test]
    fn decimal_0() {
        assert_eq!(NumberPrefix::decimal(0f64), NumberPrefix::Standalone(0f64))
    }

    #[test]
    fn decimal_999() {
        assert_eq!(
            NumberPrefix::decimal(999f32),
            NumberPrefix::Standalone(999f32)
        )
    }

    #[test]
    fn decimal_1000() {
        assert_eq!(
            NumberPrefix::decimal(1000f32),
            NumberPrefix::Prefixed(Prefix::Kilo, 1f32)
        )
    }

    #[test]
    fn decimal_1030() {
        assert_eq!(
            NumberPrefix::decimal(1030f32),
            NumberPrefix::Prefixed(Prefix::Kilo, 1.03f32)
        )
    }

    #[test]
    fn decimal_1100() {
        assert_eq!(
            NumberPrefix::decimal(1100f64),
            NumberPrefix::Prefixed(Prefix::Kilo, 1.1f64)
        )
    }

    #[test]
    fn decimal_1111() {
        assert_eq!(
            NumberPrefix::decimal(1111f64),
            NumberPrefix::Prefixed(Prefix::Kilo, 1.111f64)
        )
    }

    #[test]
    fn binary_126456() {
        assert_eq!(
            NumberPrefix::binary(126_456f32),
            NumberPrefix::Prefixed(Prefix::Kibi, 123.492_19f32)
        )
    }

    #[test]
    fn binary_1048576() {
        assert_eq!(
            NumberPrefix::binary(1_048_576f64),
            NumberPrefix::Prefixed(Prefix::Mebi, 1f64)
        )
    }

    #[test]
    fn binary_1073741824() {
        assert_eq!(
            NumberPrefix::binary(2_147_483_648f32),
            NumberPrefix::Prefixed(Prefix::Gibi, 2f32)
        )
    }

    #[test]
    fn giga() {
        assert_eq!(
            NumberPrefix::decimal(1_000_000_000f64),
            NumberPrefix::Prefixed(Prefix::Giga, 1f64)
        )
    }

    #[test]
    fn tera() {
        assert_eq!(
            NumberPrefix::decimal(1_000_000_000_000f64),
            NumberPrefix::Prefixed(Prefix::Tera, 1f64)
        )
    }

    #[test]
    fn peta() {
        assert_eq!(
            NumberPrefix::decimal(1_000_000_000_000_000f64),
            NumberPrefix::Prefixed(Prefix::Peta, 1f64)
        )
    }

    #[test]
    fn exa() {
        assert_eq!(
            NumberPrefix::decimal(1_000_000_000_000_000_000f64),
            NumberPrefix::Prefixed(Prefix::Exa, 1f64)
        )
    }

    #[test]
    fn zetta() {
        assert_eq!(
            NumberPrefix::decimal(1_000_000_000_000_000_000_000f64),
            NumberPrefix::Prefixed(Prefix::Zetta, 1f64)
        )
    }

    #[test]
    fn yotta() {
        assert_eq!(
            NumberPrefix::decimal(1_000_000_000_000_000_000_000_000f64),
            NumberPrefix::Prefixed(Prefix::Yotta, 1f64)
        )
    }

    #[test]
    fn and_so_on() {
        // When you hit yotta, don't keep going
        assert_eq!(
            NumberPrefix::decimal(1_000_000_000_000_000_000_000_000_000f64),
            NumberPrefix::Prefixed(Prefix::Yotta, 1000f64)
        )
    }
}

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(60 * 60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const YEAR: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// Wraps an std duration for human basic formatting.
#[derive(Debug)]
pub struct FormattedDuration(pub Duration);

/// Wraps an std duration for human readable formatting.
#[derive(Debug)]
pub struct HumanDuration(pub Duration);

/// Formats bytes for human readability
///
/// # Examples
/// ```rust
/// # use indicatif::HumanBytes;
/// assert_eq!("15 B",     format!("{}", HumanBytes(15)));
/// assert_eq!("1.46 KiB", format!("{}", HumanBytes(1_500)));
/// assert_eq!("1.43 MiB", format!("{}", HumanBytes(1_500_000)));
/// assert_eq!("1.40 GiB", format!("{}", HumanBytes(1_500_000_000)));
/// assert_eq!("1.36 TiB", format!("{}", HumanBytes(1_500_000_000_000)));
/// assert_eq!("1.33 PiB", format!("{}", HumanBytes(1_500_000_000_000_000)));
/// ```
#[derive(Debug)]
pub struct HumanBytes(pub u64);

/// Formats bytes for human readability using SI prefixes
///
/// # Examples
/// ```rust
/// # use indicatif::DecimalBytes;
/// assert_eq!("15 B",    format!("{}", DecimalBytes(15)));
/// assert_eq!("1.50 kB", format!("{}", DecimalBytes(1_500)));
/// assert_eq!("1.50 MB", format!("{}", DecimalBytes(1_500_000)));
/// assert_eq!("1.50 GB", format!("{}", DecimalBytes(1_500_000_000)));
/// assert_eq!("1.50 TB", format!("{}", DecimalBytes(1_500_000_000_000)));
/// assert_eq!("1.50 PB", format!("{}", DecimalBytes(1_500_000_000_000_000)));
/// ```
#[derive(Debug)]
pub struct DecimalBytes(pub u64);

/// Formats bytes for human readability using ISO/IEC prefixes
///
/// # Examples
/// ```rust
/// # use indicatif::BinaryBytes;
/// assert_eq!("15 B",     format!("{}", BinaryBytes(15)));
/// assert_eq!("1.46 KiB", format!("{}", BinaryBytes(1_500)));
/// assert_eq!("1.43 MiB", format!("{}", BinaryBytes(1_500_000)));
/// assert_eq!("1.40 GiB", format!("{}", BinaryBytes(1_500_000_000)));
/// assert_eq!("1.36 TiB", format!("{}", BinaryBytes(1_500_000_000_000)));
/// assert_eq!("1.33 PiB", format!("{}", BinaryBytes(1_500_000_000_000_000)));
/// ```
#[derive(Debug)]
pub struct BinaryBytes(pub u64);

/// Formats counts for human readability using commas
#[derive(Debug)]
pub struct HumanCount(pub u64);

/// Formats counts for human readability using commas for floats
#[derive(Debug)]
pub struct HumanFloatCount(pub f64);

impl fmt::Display for FormattedDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut t = self.0.as_secs();
        let seconds = t % 60;
        t /= 60;
        let minutes = t % 60;
        t /= 60;
        let hours = t % 24;
        t /= 24;
        if t > 0 {
            let days = t;
            write!(f, "{days}d {hours:02}:{minutes:02}:{seconds:02}")
        } else {
            write!(f, "{hours:02}:{minutes:02}:{seconds:02}")
        }
    }
}

// `HumanDuration` should be as intuitively understandable as possible.
// So we want to round, not truncate: otherwise 1 hour and 59 minutes
// would display an ETA of "1 hour" which underestimates the time
// remaining by a factor 2.
//
// To make the precision more uniform, we avoid displaying "1 unit"
// (except for seconds), because it would be displayed for a relatively
// long duration compared to the unit itself. Instead, when we arrive
// around 1.5 unit, we change from "2 units" to the next smaller unit
// (e.g. "89 seconds").
//
// Formally:
// * for n >= 2, we go from "n+1 units" to "n units" exactly at (n + 1/2) units
// * we switch from "2 units" to the next smaller unit at (1.5 unit minus half of the next smaller unit)

impl fmt::Display for HumanDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut idx = 0;
        for (i, &(cur, _, _)) in UNITS.iter().enumerate() {
            idx = i;
            match UNITS.get(i + 1) {
                Some(&next) if self.0.saturating_add(next.0 / 2) >= cur + cur / 2 => break,
                _ => continue,
            }
        }

        let (unit, name, alt) = UNITS[idx];
        // FIXME when `div_duration_f64` is stable
        let mut t = (self.0.as_secs_f64() / unit.as_secs_f64()).round() as usize;
        if idx < UNITS.len() - 1 {
            t = Ord::max(t, 2);
        }

        match (f.alternate(), t) {
            (true, _) => write!(f, "{t}{alt}"),
            (false, 1) => write!(f, "{t} {name}"),
            (false, _) => write!(f, "{t} {name}s"),
        }
    }
}

const UNITS: &[(Duration, &str, &str)] = &[
    (YEAR, "year", "y"),
    (WEEK, "week", "w"),
    (DAY, "day", "d"),
    (HOUR, "hour", "h"),
    (MINUTE, "minute", "m"),
    (SECOND, "second", "s"),
];

impl fmt::Display for HumanBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match NumberPrefix::binary(self.0 as f64) {
            NumberPrefix::Standalone(number) => write!(f, "{number:.0} B"),
            NumberPrefix::Prefixed(prefix, number) => write!(f, "{number:.2} {prefix}B"),
        }
    }
}

impl fmt::Display for DecimalBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match NumberPrefix::decimal(self.0 as f64) {
            NumberPrefix::Standalone(number) => write!(f, "{number:.0} B"),
            NumberPrefix::Prefixed(prefix, number) => write!(f, "{number:.2} {prefix}B"),
        }
    }
}

impl fmt::Display for BinaryBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match NumberPrefix::binary(self.0 as f64) {
            NumberPrefix::Standalone(number) => write!(f, "{number:.0} B"),
            NumberPrefix::Prefixed(prefix, number) => write!(f, "{number:.2} {prefix}B"),
        }
    }
}

impl fmt::Display for HumanCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use fmt::Write;

        let num = self.0.to_string();
        let len = num.len();
        for (idx, c) in num.chars().enumerate() {
            let pos = len - idx - 1;
            f.write_char(c)?;
            if pos > 0 && pos % 3 == 0 {
                f.write_char(',')?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for HumanFloatCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use fmt::Write;

        // Use formatter's precision if provided, otherwise default to 4
        let precision = f.precision().unwrap_or(4);
        let num = format!("{:.*}", precision, self.0);

        let (int_part, frac_part) = match num.split_once('.') {
            Some((int_str, fract_str)) => (int_str.to_string(), fract_str),
            None => (self.0.trunc().to_string(), ""),
        };
        let len = int_part.len();
        for (idx, c) in int_part.chars().enumerate() {
            let pos = len - idx - 1;
            f.write_char(c)?;
            if pos > 0 && pos % 3 == 0 {
                f.write_char(',')?;
            }
        }
        let frac_trimmed = frac_part.trim_end_matches('0');
        if !frac_trimmed.is_empty() {
            f.write_char('.')?;
            f.write_str(frac_trimmed)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MILLI: Duration = Duration::from_millis(1);

    #[test]
    fn human_duration_alternate() {
        for (unit, _, alt) in UNITS {
            assert_eq!(format!("2{alt}"), format!("{:#}", HumanDuration(2 * *unit)));
        }
    }

    #[test]
    fn human_duration_less_than_one_second() {
        assert_eq!(
            "0 seconds",
            format!("{}", HumanDuration(Duration::from_secs(0)))
        );
        assert_eq!("0 seconds", format!("{}", HumanDuration(MILLI)));
        assert_eq!("0 seconds", format!("{}", HumanDuration(499 * MILLI)));
        assert_eq!("1 second", format!("{}", HumanDuration(500 * MILLI)));
        assert_eq!("1 second", format!("{}", HumanDuration(999 * MILLI)));
    }

    #[test]
    fn human_duration_less_than_two_seconds() {
        assert_eq!("1 second", format!("{}", HumanDuration(1499 * MILLI)));
        assert_eq!("2 seconds", format!("{}", HumanDuration(1500 * MILLI)));
        assert_eq!("2 seconds", format!("{}", HumanDuration(1999 * MILLI)));
    }

    #[test]
    fn human_duration_one_unit() {
        assert_eq!("1 second", format!("{}", HumanDuration(SECOND)));
        assert_eq!("60 seconds", format!("{}", HumanDuration(MINUTE)));
        assert_eq!("60 minutes", format!("{}", HumanDuration(HOUR)));
        assert_eq!("24 hours", format!("{}", HumanDuration(DAY)));
        assert_eq!("7 days", format!("{}", HumanDuration(WEEK)));
        assert_eq!("52 weeks", format!("{}", HumanDuration(YEAR)));
    }

    #[test]
    fn human_duration_less_than_one_and_a_half_unit() {
        // this one is actually done at 1.5 unit - half of the next smaller unit - epsilon
        // and should display the next smaller unit
        let d = HumanDuration(MINUTE + MINUTE / 2 - SECOND / 2 - MILLI);
        assert_eq!("89 seconds", format!("{d}"));
        let d = HumanDuration(HOUR + HOUR / 2 - MINUTE / 2 - MILLI);
        assert_eq!("89 minutes", format!("{d}"));
        let d = HumanDuration(DAY + DAY / 2 - HOUR / 2 - MILLI);
        assert_eq!("35 hours", format!("{d}"));
        let d = HumanDuration(WEEK + WEEK / 2 - DAY / 2 - MILLI);
        assert_eq!("10 days", format!("{d}"));
        let d = HumanDuration(YEAR + YEAR / 2 - WEEK / 2 - MILLI);
        assert_eq!("78 weeks", format!("{d}"));
    }

    #[test]
    fn human_duration_one_and_a_half_unit() {
        // this one is actually done at 1.5 unit - half of the next smaller unit
        // and should still display "2 units"
        let d = HumanDuration(MINUTE + MINUTE / 2 - SECOND / 2);
        assert_eq!("2 minutes", format!("{d}"));
        let d = HumanDuration(HOUR + HOUR / 2 - MINUTE / 2);
        assert_eq!("2 hours", format!("{d}"));
        let d = HumanDuration(DAY + DAY / 2 - HOUR / 2);
        assert_eq!("2 days", format!("{d}"));
        let d = HumanDuration(WEEK + WEEK / 2 - DAY / 2);
        assert_eq!("2 weeks", format!("{d}"));
        let d = HumanDuration(YEAR + YEAR / 2 - WEEK / 2);
        assert_eq!("2 years", format!("{d}"));
    }

    #[test]
    fn human_duration_two_units() {
        assert_eq!("2 seconds", format!("{}", HumanDuration(2 * SECOND)));
        assert_eq!("2 minutes", format!("{}", HumanDuration(2 * MINUTE)));
        assert_eq!("2 hours", format!("{}", HumanDuration(2 * HOUR)));
        assert_eq!("2 days", format!("{}", HumanDuration(2 * DAY)));
        assert_eq!("2 weeks", format!("{}", HumanDuration(2 * WEEK)));
        assert_eq!("2 years", format!("{}", HumanDuration(2 * YEAR)));
    }

    #[test]
    fn human_duration_less_than_two_and_a_half_units() {
        let d = HumanDuration(2 * SECOND + SECOND / 2 - MILLI);
        assert_eq!("2 seconds", format!("{d}"));
        let d = HumanDuration(2 * MINUTE + MINUTE / 2 - MILLI);
        assert_eq!("2 minutes", format!("{d}"));
        let d = HumanDuration(2 * HOUR + HOUR / 2 - MILLI);
        assert_eq!("2 hours", format!("{d}"));
        let d = HumanDuration(2 * DAY + DAY / 2 - MILLI);
        assert_eq!("2 days", format!("{d}"));
        let d = HumanDuration(2 * WEEK + WEEK / 2 - MILLI);
        assert_eq!("2 weeks", format!("{d}"));
        let d = HumanDuration(2 * YEAR + YEAR / 2 - MILLI);
        assert_eq!("2 years", format!("{d}"));
    }

    #[test]
    fn human_duration_two_and_a_half_units() {
        let d = HumanDuration(2 * SECOND + SECOND / 2);
        assert_eq!("3 seconds", format!("{d}"));
        let d = HumanDuration(2 * MINUTE + MINUTE / 2);
        assert_eq!("3 minutes", format!("{d}"));
        let d = HumanDuration(2 * HOUR + HOUR / 2);
        assert_eq!("3 hours", format!("{d}"));
        let d = HumanDuration(2 * DAY + DAY / 2);
        assert_eq!("3 days", format!("{d}"));
        let d = HumanDuration(2 * WEEK + WEEK / 2);
        assert_eq!("3 weeks", format!("{d}"));
        let d = HumanDuration(2 * YEAR + YEAR / 2);
        assert_eq!("3 years", format!("{d}"));
    }

    #[test]
    fn human_duration_three_units() {
        assert_eq!("3 seconds", format!("{}", HumanDuration(3 * SECOND)));
        assert_eq!("3 minutes", format!("{}", HumanDuration(3 * MINUTE)));
        assert_eq!("3 hours", format!("{}", HumanDuration(3 * HOUR)));
        assert_eq!("3 days", format!("{}", HumanDuration(3 * DAY)));
        assert_eq!("3 weeks", format!("{}", HumanDuration(3 * WEEK)));
        assert_eq!("3 years", format!("{}", HumanDuration(3 * YEAR)));
    }

    #[test]
    fn human_count() {
        assert_eq!("42", format!("{}", HumanCount(42)));
        assert_eq!("7,654", format!("{}", HumanCount(7654)));
        assert_eq!("12,345", format!("{}", HumanCount(12345)));
        assert_eq!("1,234,567,890", format!("{}", HumanCount(1234567890)));
    }

    #[test]
    fn human_float_count() {
        assert_eq!("42", format!("{}", HumanFloatCount(42.0)));
        assert_eq!("7,654", format!("{}", HumanFloatCount(7654.0)));
        assert_eq!("12,345", format!("{}", HumanFloatCount(12345.0)));
        assert_eq!(
            "1,234,567,890",
            format!("{}", HumanFloatCount(1234567890.0))
        );
        assert_eq!("42.5", format!("{}", HumanFloatCount(42.5)));
        assert_eq!("42.5", format!("{}", HumanFloatCount(42.500012345)));
        assert_eq!("42.502", format!("{}", HumanFloatCount(42.502012345)));
        assert_eq!("7,654.321", format!("{}", HumanFloatCount(7654.321)));
        assert_eq!("7,654.321", format!("{}", HumanFloatCount(7654.3210123456)));
        assert_eq!("12,345.6789", format!("{}", HumanFloatCount(12345.6789)));
        assert_eq!(
            "1,234,567,890.1235",
            format!("{}", HumanFloatCount(1234567890.1234567))
        );
        assert_eq!(
            "1,234,567,890.1234",
            format!("{}", HumanFloatCount(1234567890.1234321))
        );
        assert_eq!("1,234", format!("{:.0}", HumanFloatCount(1234.1234321)));
        assert_eq!("1,234.1", format!("{:.1}", HumanFloatCount(1234.1234321)));
        assert_eq!("1,234.12", format!("{:.2}", HumanFloatCount(1234.1234321)));
        assert_eq!("1,234.123", format!("{:.3}", HumanFloatCount(1234.1234321)));
        assert_eq!(
            "1,234.1234320999999454215867445",
            format!("{:.25}", HumanFloatCount(1234.1234321))
        );
    }
}
