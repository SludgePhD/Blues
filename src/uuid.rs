//! Bluetooth UUIDs.

use core::fmt;
use std::str::FromStr;

/// A 128-bit UUID, identifying a Bluetooth service or characteristic.
///
/// # Construction
///
/// This type can be constructed from a compile-time string via the [`Uuid::from_static`] function.
/// A [`FromStr`] implementation for fallible parsing is also provided.
///
/// [`Uuid`]s can also be constructed from a 16-bit "alias" assigned by the Bluetooth SIG via the
/// [`Uuid::from_u16`] function.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    const BASE: Self = Self::from_static("00000000-0000-1000-8000-00805f9b34fb");

    /// Parses a [`Uuid`] from a string.
    const fn from_str(s: &str) -> Result<Self, ParseUuidError> {
        // `const fn` is still frustratingly limited (eg. you can't even index a slice with a range)
        // so this is a bit of a macro-heavy abomination.

        const fn cvt_nibble(digit: u8) -> Result<u8, ParseUuidError> {
            Ok(match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'f' => digit - b'a' + 10,
                b'A'..=b'F' => digit - b'A' + 10,
                _ => return Err(ParseUuidError(ErrorKind::InvalidDigit)),
            })
        }

        // const-compatible `try!` / `?` operator without conversion.
        macro_rules! please {
            ($e:expr) => {
                match $e {
                    Ok(val) => val,
                    Err(e) => return Err(e),
                }
            };
        }

        // Consumes 2 hex digits from the input.
        macro_rules! next_byte {
            ($in:ident, $i:ident) => {{
                if $i >= $in.len() {
                    return Err(ParseUuidError(ErrorKind::Eof));
                }
                let high = please!(cvt_nibble($in[$i]));
                $i += 1;
                if $i >= $in.len() {
                    return Err(ParseUuidError(ErrorKind::Eof));
                }
                let low = please!(cvt_nibble($in[$i]));
                $i += 1;
                (high << 4) | low
            }};
        }

        // Consumes a `-` from the input string.
        macro_rules! dash {
            ($in:ident, $i:ident) => {{
                if $i >= $in.len() {
                    return Err(ParseUuidError(ErrorKind::Eof));
                }

                if $in[$i] != b'-' {
                    return Err(ParseUuidError(ErrorKind::InvalidDash));
                }

                $i += 1;
            }};
        }

        let mut i = 0;

        // We're parsing a UUID like 7c9ac820-0886-4e50-bcca-588b883f8649
        let mut out = [0; 16];
        let bytes = s.as_bytes();

        out[0] = next_byte!(bytes, i);
        out[1] = next_byte!(bytes, i);
        out[2] = next_byte!(bytes, i);
        out[3] = next_byte!(bytes, i);
        dash!(bytes, i);
        out[4] = next_byte!(bytes, i);
        out[5] = next_byte!(bytes, i);
        dash!(bytes, i);
        out[6] = next_byte!(bytes, i);
        out[7] = next_byte!(bytes, i);
        dash!(bytes, i);
        out[8] = next_byte!(bytes, i);
        out[9] = next_byte!(bytes, i);
        dash!(bytes, i);
        out[10] = next_byte!(bytes, i);
        out[11] = next_byte!(bytes, i);
        out[12] = next_byte!(bytes, i);
        out[13] = next_byte!(bytes, i);
        out[14] = next_byte!(bytes, i);
        out[15] = next_byte!(bytes, i);

        if i != bytes.len() {
            return Err(ParseUuidError(ErrorKind::TrailingData));
        }

        Ok(Self(out))
    }

    /// Creates a [`Uuid`] from a static string, potentially at compile time.
    ///
    /// Panics if the string is invalid.
    ///
    /// This is typically the behavior you want when defining `const` [`Uuid`]s.
    pub const fn from_static(s: &'static str) -> Self {
        match Self::from_str(s) {
            Ok(uuid) => uuid,
            Err(_) => panic!("malformed UUID"),
        }
    }

    /// Creates a [`Uuid`] from a 16-bit alias.
    pub const fn from_u16(short: u16) -> Self {
        let [hi, lo] = short.to_be_bytes();
        let mut uuid = Self::BASE;
        uuid.0[2] = hi;
        uuid.0[3] = lo;
        uuid
    }
}

impl FromStr for Uuid {
    type Err = ParseUuidError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s)
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7], self.0[8], self.0[9], self.0[10], self.0[11], self.0[12], self.0[13], self.0[14], self.0[15])
    }
}

impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// The error type returned by the [`FromStr`] implementation of [`Uuid`].
#[derive(Debug)]
pub struct ParseUuidError(ErrorKind);

#[derive(Debug)]
enum ErrorKind {
    Eof,
    InvalidDigit,
    InvalidDash,
    TrailingData,
}

impl fmt::Display for ParseUuidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match &self.0 {
            ErrorKind::Eof => "unexpected end of input",
            ErrorKind::InvalidDigit => "invalid hex digit",
            ErrorKind::InvalidDash => "invalid character (`-` expected)",
            ErrorKind::TrailingData => "invalid trailing data",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        assert_eq!(
            Uuid::BASE.to_string(),
            "00000000-0000-1000-8000-00805f9b34fb"
        );
    }
}
