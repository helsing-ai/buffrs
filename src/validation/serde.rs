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

use serde::{Deserialize, Deserializer, de};
use std::collections::BTreeMap;
use std::fmt::Display;
use std::str::FromStr;

/// Allow deserializing an integer key from a string representation.
///
/// This is needed because when serializing to JSON, serde will serialize maps with integer keys
/// as strings, but will not allow deserializing from this encoding.
pub(crate) fn de_int_key<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Eq + Ord + FromStr,
    K::Err: Display,
    V: Deserialize<'de>,
{
    let string_map = <BTreeMap<String, V>>::deserialize(deserializer)?;
    let mut map = BTreeMap::default();
    for (s, v) in string_map {
        let k = K::from_str(&s).map_err(de::Error::custom)?;
        map.insert(k, v);
    }
    Ok(map)
}
