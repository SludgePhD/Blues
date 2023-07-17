//! Bluetooth device addresses.
//!
//! Device addresses are a link-layer mechanism that allows devices to identify and exchange data
//! with each other.

use core::fmt;
use std::{fmt::Write, num::ParseIntError, str::FromStr};

// TODO: renaming `Address` to `RawAddress`, and having an `Address` type that bundles a
// `RawAddress` and `AddressType` might be better than this. That way `Address` could actually parse
// the bytes properly.

/// Describes the meaning of the bytes in an [`Address`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddressType {
    /// Address follows the MAC address standard.
    ///
    /// The first 3 Bytes identify the vendor, the last 3 Bytes identify the device.
    Public,
    /// Address is randomly generated.
    ///
    /// These addresses can either be "Static Random" addresses, or they can be resolvable or
    /// non-resolvable "Private Random" addresses for use with BLE Privacy.
    Random,
}

impl AddressType {
    pub(crate) fn from_str(s: &str) -> crate::Result<Self> {
        match s {
            "public" => Ok(Self::Public),
            "random" => Ok(Self::Random),
            _ => Err(crate::Error::from(format!("invalid address type '{}'", s))),
        }
    }
}

/// A 6-Byte Bluetooth device address.
///
/// Device addresses can either follow the MAC address standard, or be randomly generated. Which one
/// applies to an [`Address`] is specified by its [`AddressType`], which can be fetched separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Address([u8; 6]);

impl Address {
    #[inline]
    pub fn from_bytes(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
}

impl From<[u8; 6]> for Address {
    #[inline]
    fn from(value: [u8; 6]) -> Self {
        Self(value)
    }
}

impl From<Address> for [u8; 6] {
    #[inline]
    fn from(value: Address) -> Self {
        value.0
    }
}

impl AsRef<[u8; 6]> for Address {
    fn as_ref(&self) -> &[u8; 6] {
        &self.0
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if i != 0 {
                f.write_char(':')?;
            }

            write!(f, "{:02X}", byte)?;
        }

        Ok(())
    }
}

/// Parses a Bluetooth [`Address`] from a colon-separated hex string.
///
/// Example: `aa:ff:00:33:22:11`
impl FromStr for Address {
    type Err = ParseAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0; 6];
        for (i, s) in s.splitn(6, ':').enumerate() {
            if s.len() != 2 {
                return Err(ParseAddressError::other());
            }
            bytes[i] = u8::from_str_radix(s, 16).map_err(ParseAddressError::parse_int)?;
            if i == bytes.len() - 1 {
                return Ok(Address(bytes));
            }
        }

        Err(ParseAddressError::other())
    }
}

/// The error type returned by the [`FromStr`] implementation of [`Address`].
#[derive(Debug)]
pub struct ParseAddressError(ParseAddressErrorKind);

impl fmt::Display for ParseAddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            ParseAddressErrorKind::ParseInt(e) => e.fmt(f),
            ParseAddressErrorKind::Other => f.write_str("invalid device address"),
        }
    }
}

impl std::error::Error for ParseAddressError {}

#[derive(Debug)]
enum ParseAddressErrorKind {
    ParseInt(ParseIntError),
    Other,
}

impl ParseAddressError {
    fn parse_int(e: ParseIntError) -> Self {
        Self(ParseAddressErrorKind::ParseInt(e))
    }

    fn other() -> Self {
        Self(ParseAddressErrorKind::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        let s = "AA:BB:CC:11:22:33";
        let addr = Address::from_str(s).unwrap();
        assert_eq!(addr.to_string(), s);
    }

    #[test]
    fn invalid() {
        Address::from_str("").unwrap_err();
        Address::from_str("aa:bb:cc:11:22:3").unwrap_err();
        Address::from_str("aa:bb:cc:11:22:333").unwrap_err();
        Address::from_str("aa:bb:cc:11:22:33:").unwrap_err();
        Address::from_str("aa:bb:cc:11:22:33:44").unwrap_err();
        Address::from_str("aa:bb:cc:11:22:33 ").unwrap_err();
        Address::from_str("za:bb:cc:11:22:33").unwrap_err();
    }
}
