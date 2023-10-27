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

use std::{fmt, str::FromStr};

use serde::{de::Visitor, Deserialize, Serialize};
use thiserror::Error;

/// Supported types of digest algorithms.
// Do not reorder variants; the ordering is significant, see #38 and #106.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DigestAlgorithm {
    /// SHA-2 with 256 bits
    #[serde(rename = "sha256")]
    SHA256,
}

/// Error parsing a [`DigestAlgorithm`].
#[derive(Error, Debug)]
pub enum DigestAlgorithmError {
    /// Represents a ring digest algorithm that isn't supported by Buffrs
    #[error("unsupported digest algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

impl FromStr for DigestAlgorithm {
    type Err = DigestAlgorithmError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match serde_typename::from_str(input) {
            Ok(value) => Ok(value),
            _other => Err(DigestAlgorithmError::UnsupportedAlgorithm(input.into())),
        }
    }
}

#[test]
fn can_parse_digest_algorithm() {
    assert!(matches!("sha256".parse(), Ok(DigestAlgorithm::SHA256)));
    assert!("md5".parse::<DigestAlgorithm>().is_err());
}

impl fmt::Display for DigestAlgorithm {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_typename::to_str(self) {
            Ok(name) => fmt.write_str(name),
            Err(error) => unreachable!("cannot convert DigestAlgorithm to string: {error}"),
        }
    }
}

#[test]
fn can_display_digest_algorithm() {
    assert_eq!(DigestAlgorithm::SHA256.to_string(), "sha256");
}

/// A representation of a cryptographic digest for data integrity validation
// Do not reorder fields: the ordering is significant, see #38 and #106.
#[derive(Clone, PartialEq, Eq, Ord, PartialOrd, Debug)]
pub struct Digest {
    /// Algorithm used to create digest.
    algorithm: DigestAlgorithm,
    /// Digest value.
    digest: Vec<u8>,
}

impl Digest {
    /// Algorithm used to create this digest.
    pub fn algorithm(&self) -> DigestAlgorithm {
        self.algorithm
    }

    /// Digest as raw byte data.
    pub fn as_bytes(&self) -> &[u8] {
        &self.digest
    }
}

impl TryFrom<ring::digest::Digest> for Digest {
    type Error = DigestAlgorithmError;

    fn try_from(value: ring::digest::Digest) -> Result<Self, Self::Error> {
        let algorithm = if value.algorithm() == &ring::digest::SHA256 {
            DigestAlgorithm::SHA256
        } else {
            return Err(DigestAlgorithmError::UnsupportedAlgorithm(format!(
                "{:?}",
                value.algorithm()
            )));
        };

        Ok(Self {
            digest: value.as_ref().to_vec(),
            algorithm,
        })
    }
}

/// Error parsing a [`DigestAlgorithm`].
#[derive(Error, Debug)]
#[allow(missing_docs)]
pub enum DigestError {
    #[error("missing delimiter")]
    MissingDelimiter,
    #[error(transparent)]
    Algorithm(#[from] DigestAlgorithmError),
    #[error(transparent)]
    Digest(#[from] hex::FromHexError),
}

impl FromStr for Digest {
    type Err = DigestError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let Some((algorithm_str, digest_str)) = input.split_once(':') else {
            return Err(DigestError::MissingDelimiter);
        };
        let algorithm: DigestAlgorithm = algorithm_str.parse()?;
        let digest = hex::decode(digest_str)?;
        Ok(Self { algorithm, digest })
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, hex::encode(&self.digest))
    }
}

// FIXME(xfbs): we should almost never manually implement serde's serialize or deserialize.  it is
// usually better to use something like `serde_with` to achieve this.

impl Serialize for Digest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

struct DigestVisitor;

impl<'de> Visitor<'de> for DigestVisitor {
    type Value = Digest;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a hexadecimal encoded cryptographic digest")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        value.parse().map_err(E::custom)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(DigestVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_test::{assert_tokens, Token};

    const HELLO_DIGEST: &str =
        "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn can_parse_digest() {
        let digest: Digest = HELLO_DIGEST.parse().unwrap();
        assert_eq!(digest.algorithm(), DigestAlgorithm::SHA256);
        assert_eq!(
            digest.as_bytes(),
            &hex::decode(&HELLO_DIGEST[7..]).unwrap()[..]
        );
    }

    #[test]
    fn can_convert_digest() {
        let digest = ring::digest::digest(&ring::digest::SHA256, "hello".as_bytes());
        let digest: Digest = digest.try_into().unwrap();
        assert_eq!(digest.to_string(), HELLO_DIGEST);
    }

    #[test]
    fn cannot_parse_invalid_digest() {
        assert!(matches!(
            "md5:abc".parse::<Digest>(),
            Err(DigestError::Algorithm(_))
        ));
        assert!(matches!(
            "".parse::<Digest>(),
            Err(DigestError::MissingDelimiter)
        ));
        assert!(matches!(
            "sha256:xxx".parse::<Digest>(),
            Err(DigestError::Digest(_))
        ));
    }

    #[test]
    fn can_roundtrip_digest() {
        let digest: Digest = HELLO_DIGEST.parse().unwrap();
        assert_eq!(digest.to_string(), HELLO_DIGEST);
    }

    #[test]
    fn can_serialize() {
        let digest: Digest = HELLO_DIGEST.parse().unwrap();
        assert_tokens(&digest, &[Token::Str(HELLO_DIGEST)]);
    }
}
