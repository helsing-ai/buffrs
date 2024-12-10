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
use sha2::Digest as _;
use strum::{Display, EnumString};
use thiserror::Error;

/// Supported types of digest algorithms.
// Do not reorder variants; the ordering is significant, see #38 and #106.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumString, Display,
)]
pub enum DigestAlgorithm {
    /// SHA-2 with 256 bits
    #[serde(rename = "sha256")]
    #[strum(serialize = "sha256")]
    SHA256,
}

impl DigestAlgorithm {
    /// Create a digest of some data using this algorithm.
    pub fn digest(&self, data: &[u8]) -> Digest {
        let digest = match self {
            DigestAlgorithm::SHA256 => sha2::Sha256::new().chain_update(data).finalize().to_vec(),
        };

        Digest {
            algorithm: *self,
            digest,
        }
    }
}

/// Error parsing a [`DigestAlgorithm`].
#[derive(Error, Debug)]
pub enum DigestAlgorithmError {
    /// Represents a ring digest algorithm that isn't supported by Buffrs
    #[error("unsupported digest algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

#[test]
fn can_parse_digest_algorithm() {
    assert!(matches!("sha256".parse(), Ok(DigestAlgorithm::SHA256)));
    assert!("md5".parse::<DigestAlgorithm>().is_err());
}

#[test]
fn can_display_digest_algorithm() {
    assert_eq!(DigestAlgorithm::SHA256.to_string(), "sha256");
}

/// A representation of a cryptographic digest for data integrity validation
///
/// ```rust
/// use buffrs::lock::{Digest, DigestAlgorithm};
///
/// let algorithm = DigestAlgorithm::SHA256;
/// let hello = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
///
/// let digest = Digest::from_parts(algorithm, hello).unwrap();
/// // You can also parse `Digest` from the string representation
/// assert_eq!(digest, format!("{algorithm}:{hello}").parse().unwrap());
///
/// // Roundtripping is possible
/// assert_eq!(digest, format!("{digest}").parse().unwrap());
/// ```
// Do not reorder fields: the ordering is significant, see #38 and #106.
#[derive(Clone, PartialEq, Eq, Ord, PartialOrd, Debug)]
pub struct Digest {
    /// Algorithm used to create digest.
    algorithm: DigestAlgorithm,
    /// Digest value.
    digest: Vec<u8>,
}

impl Digest {
    /// Digest are displayed as `algorithm:digest`, this takes the two in separate variables.
    pub fn from_parts(algorithm: DigestAlgorithm, digest: &str) -> Result<Self, DigestError> {
        let digest = hex::decode(digest)?;
        Ok(Self { algorithm, digest })
    }

    /// Algorithm used to create this digest.
    pub fn algorithm(&self) -> DigestAlgorithm {
        self.algorithm
    }

    /// Digest as raw byte data.
    pub fn as_bytes(&self) -> &[u8] {
        &self.digest
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
        let algorithm: DigestAlgorithm = algorithm_str
            .parse()
            .map_err(|_| DigestAlgorithmError::UnsupportedAlgorithm(algorithm_str.into()))?;
        Self::from_parts(algorithm, digest_str)
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

impl Visitor<'_> for DigestVisitor {
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
        let digest = DigestAlgorithm::SHA256.digest("hello".as_bytes());
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

    #[test]
    fn from_parts() {
        let algorithm = DigestAlgorithm::SHA256;
        let hello = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

        let digest = Digest::from_parts(algorithm, hello).unwrap();
        assert_eq!(digest, HELLO_DIGEST.parse().unwrap());

        assert_eq!(digest, format!("{digest}").parse().unwrap());
    }
}
