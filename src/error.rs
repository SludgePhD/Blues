use std::fmt;

use crate::{address::ParseAddressError, uuid::ParseUuidError};

/// A result type hardwired to use [`Error`] as its error type.
pub type Result<T> = std::result::Result<T, Error>;

/// The primary error type used throughout this library.
#[derive(Debug)]
pub struct Error {
    inner: ErrorKind,
}

impl Error {
    pub(crate) fn from(e: impl Into<ErrorKind>) -> Self {
        Self { inner: e.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            ErrorKind::Zbus(e) => e.fmt(f),
            ErrorKind::Fdo(e) => e.fmt(f),
            ErrorKind::ParseAddressError(e) => e.fmt(f),
            ErrorKind::ParseUuidError(e) => e.fmt(f),
            ErrorKind::Other(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub(crate) enum ErrorKind {
    Zbus(zbus::Error),
    Fdo(zbus::fdo::Error),
    ParseAddressError(ParseAddressError),
    ParseUuidError(ParseUuidError),
    Other(String),
}

impl From<zbus::Error> for ErrorKind {
    fn from(value: zbus::Error) -> Self {
        Self::Zbus(value)
    }
}

impl From<zbus::fdo::Error> for ErrorKind {
    fn from(value: zbus::fdo::Error) -> Self {
        Self::Fdo(value)
    }
}

impl From<ParseAddressError> for ErrorKind {
    fn from(value: ParseAddressError) -> Self {
        Self::ParseAddressError(value)
    }
}

impl From<ParseUuidError> for ErrorKind {
    fn from(value: ParseUuidError) -> Self {
        Self::ParseUuidError(value)
    }
}

impl From<String> for ErrorKind {
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

impl From<&str> for ErrorKind {
    fn from(value: &str) -> Self {
        Self::Other(value.to_string())
    }
}
