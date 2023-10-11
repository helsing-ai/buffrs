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

use buffrs::validation::*;
use paste::paste;

macro_rules! parse_test {
    ($name:ident) => {
        paste! {
            #[test]
            fn [< can_parse_ $name >]() {
                use std::path::Path;
                let mut parser = Parser::new(Path::new("tests/data/parsing"));
                parser.input(std::path::Path::new(concat!("tests/data/parsing/", stringify!($name), ".proto")));
                let packages = parser.parse().unwrap();
                let parsed_file = concat!("tests/data/parsing/", stringify!($name), ".json");
                let expected = std::fs::read_to_string(parsed_file).unwrap();
                let expected = serde_json::from_str(&expected).unwrap();
                similar_asserts::assert_eq!(packages, expected);
            }
        }
    };
}

parse_test!(books);
parse_test!(addressbook);
