// Copyright 2023 Helsing GmbH
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{fmt, ops::Deref, str::FromStr};

use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};

/// A `buffrs` package directory for parsing and type safety
#[derive(Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct PackageDirectory(String);

/// Errors that can be generated parsing [`PackageDirectory`], see [`PackageDirectory::new()`].
#[derive(thiserror::Error, Debug, PartialEq)]
pub enum PackageDirectoryError {
    /// Empty package directory.
    #[error("package directory must be at least one character long, but was empty")]
    Empty,
    /// Too long.
    #[error("package directories must be at most 128 characters long, but was {0:}")]
    TooLong(usize),
    /// Invalid start character.
    #[error("package directory must start with alphabetic character, but was {0:}")]
    InvalidStart(char),
    /// Invalid character.
    #[error("package directory must consist of only ASCII lowercase and dashes (-, _), but contains {0:} at position {1:}")]
    InvalidCharacter(char, usize),
}

impl super::ParseError for PackageDirectoryError {
    #[inline]
    fn empty() -> Self {
        Self::Empty
    }

    #[inline]
    fn too_long(current_length: usize) -> Self {
        Self::TooLong(current_length)
    }

    #[inline]
    fn invalid_start(first: char) -> Self {
        Self::InvalidStart(first)
    }

    #[inline]
    fn invalid_character(found: char, pos: usize) -> Self {
        Self::InvalidCharacter(found, pos)
    }
}

impl PackageDirectory {
    const MAX_LENGTH: usize = 128;

    /// New package directory from string.
    pub fn new<S: Into<String>>(value: S) -> Result<Self, PackageDirectoryError> {
        let value = value.into();
        Self::validate(&value)?;
        Ok(Self(value))
    }

    /// Validate a package directory.
    pub fn validate(directory: impl AsRef<str>) -> Result<(), PackageDirectoryError> {
        super::validate(directory.as_ref(), &[b'-', b'_'], Self::MAX_LENGTH)
    }
}

impl TryFrom<String> for PackageDirectory {
    type Error = PackageDirectoryError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for PackageDirectory {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::new(input).into_diagnostic()
    }
}

impl From<PackageDirectory> for String {
    fn from(s: PackageDirectory) -> Self {
        s.to_string()
    }
}

impl Deref for PackageDirectory {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for PackageDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ascii_lowercase() {
        assert_eq!(
            PackageDirectory::new("abc"),
            Ok(PackageDirectory("abc".into()))
        );
    }

    #[test]
    fn short() {
        assert_eq!(PackageDirectory::new("a"), Ok(PackageDirectory("a".into())));
        assert_eq!(
            PackageDirectory::new("ab"),
            Ok(PackageDirectory("ab".into()))
        );
    }

    #[test]
    fn long() {
        assert_eq!(
            PackageDirectory::new("a".repeat(PackageDirectory::MAX_LENGTH)),
            Ok(PackageDirectory("a".repeat(PackageDirectory::MAX_LENGTH)))
        );

        assert_eq!(
            PackageDirectory::new("a".repeat(PackageDirectory::MAX_LENGTH + 1)),
            Err(PackageDirectoryError::TooLong(
                PackageDirectory::MAX_LENGTH + 1
            ))
        );
    }

    #[test]
    fn empty() {
        assert_eq!(PackageDirectory::new(""), Err(PackageDirectoryError::Empty));
    }

    #[test]
    fn numeric_start() {
        assert_eq!(
            PackageDirectory::new("4abc"),
            Err(PackageDirectoryError::InvalidStart('4'))
        );
    }

    #[test]
    fn underscore_and_dash() {
        assert_eq!(
            PackageDirectory::new("with_underscore-and-dash"),
            Ok(PackageDirectory("with_underscore-and-dash".into())),
        );
    }
}
