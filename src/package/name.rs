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

/// A `buffrs` package name for parsing and type safety
#[derive(Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct PackageName(String);

/// Errors that can be generated parsing [`PackageName`][], see [`PackageName::new()`][].
#[derive(thiserror::Error, Debug, PartialEq)]
pub enum PackageNameError {
    /// Empty package name.
    #[error("package name must be at least one character long, but was empty")]
    Empty,
    /// Too long.
    #[error("package names must be at most 128 characters long, but was {0:}")]
    TooLong(usize),
    /// Invalid start character.
    #[error("package name must start with alphabetic character, but was {0:}")]
    InvalidStart(char),
    /// Invalid character.
    #[error("package name must consist of only ASCII lowercase and dashes, but contains {0:} at position {1:}")]
    InvalidCharacter(char, usize),
}

impl PackageName {
    const MIN_LENGTH: usize = 1;
    const MAX_LENGTH: usize = 128;

    /// New package name from string.
    pub fn new<S: Into<String>>(value: S) -> Result<Self, PackageNameError> {
        let value = value.into();
        Self::validate(&value)?;
        Ok(Self(value))
    }

    /// Determine if this character is allowed at the start of a package name.
    fn is_allowed_start(c: char) -> bool {
        c.is_alphabetic()
    }

    /// Determine if this character is allowed anywhere in a package name.
    fn is_allowed(c: char) -> bool {
        let is_ascii_lowercase_alphanumeric =
            |c: char| c.is_ascii_alphanumeric() && !c.is_ascii_uppercase();
        match c {
            '-' => true,
            c if is_ascii_lowercase_alphanumeric(c) => true,
            _ => false,
        }
    }

    /// Validate a package name.
    pub fn validate(name: impl AsRef<str>) -> Result<(), PackageNameError> {
        let name = name.as_ref();

        // validate length
        if name.len() < Self::MIN_LENGTH {
            return Err(PackageNameError::Empty);
        }

        if name.len() > Self::MAX_LENGTH {
            return Err(PackageNameError::TooLong(name.len()));
        }

        // validate first character
        match name.chars().next() {
            Some(c) if Self::is_allowed_start(c) => {}
            Some(c) => return Err(PackageNameError::InvalidStart(c)),
            None => unreachable!(),
        }

        // validate all characters
        let illegal = name
            .chars()
            .enumerate()
            .find(|(_, c)| !Self::is_allowed(*c));

        if let Some((index, c)) = illegal {
            return Err(PackageNameError::InvalidCharacter(c, index));
        }

        Ok(())
    }
}

impl TryFrom<String> for PackageName {
    type Error = PackageNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for PackageName {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::new(input).into_diagnostic()
    }
}

impl From<PackageName> for String {
    fn from(s: PackageName) -> Self {
        s.to_string()
    }
}

impl Deref for PackageName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ascii_lowercase() {
        assert_eq!(PackageName::new("abc"), Ok(PackageName("abc".into())));
        assert_eq!(PackageName::new("abc"), Ok(PackageName("abc".into())));
    }

    #[test]
    fn short() {
        assert_eq!(PackageName::new("a"), Ok(PackageName("a".into())));
        assert_eq!(PackageName::new("ab"), Ok(PackageName("ab".into())));
    }

    #[test]
    fn long() {
        assert_eq!(
            PackageName::new("a".repeat(128)),
            Ok(PackageName("a".repeat(128)))
        );

        assert_eq!(
            PackageName::new("a".repeat(129)),
            Err(PackageNameError::TooLong(129))
        );
    }

    #[test]
    fn empty() {
        assert_eq!(PackageName::new(""), Err(PackageNameError::Empty));
    }

    #[test]
    fn numeric_start() {
        assert_eq!(
            PackageName::new("4abc"),
            Err(PackageNameError::InvalidStart('4'))
        );
    }

    #[test]
    fn snake_case() {
        assert_eq!(
            PackageName::new("serde_typename"),
            Err(PackageNameError::InvalidCharacter('_', 5))
        );
    }
}
