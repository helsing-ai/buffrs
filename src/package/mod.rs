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

mod compressed;
mod directory;
mod name;
mod store;
mod r#type;

pub use self::{
    compressed::Package, directory::PackageDirectory, name::PackageName, r#type::PackageType,
    store::PackageStore,
};

trait ParseError {
    fn empty() -> Self;
    fn too_long(current_length: usize) -> Self;
    fn invalid_start(first: char) -> Self;
    fn invalid_character(found: char, pos: usize) -> Self;
}

/// Validation function for both package name and directories. They have very similar rules with
/// just extra allowed characters being different at the moment.
///
/// Shared allowed characters are `a-z` for the first and `a-z0-9` + extras for the rest.
fn validate<E>(raw: &str, extra_allowed_chars: &[u8], max_len: usize) -> Result<(), E>
where
    E: ParseError,
{
    let (first, rest) = match raw.as_bytes() {
        [] => return Err(E::empty()),
        x if x.len() > max_len => return Err(E::too_long(x.len())),
        [first, rest @ ..] => (first, rest),
    };

    if !first.is_ascii_lowercase() {
        // Handle UTF-8 chars correctly
        return Err(E::invalid_start(raw.chars().next().unwrap()));
    }

    let is_disallowed = |&(_, c): &(usize, &u8)| {
        !(c.is_ascii_lowercase() || c.is_ascii_digit() || extra_allowed_chars.contains(c))
    };

    match rest.iter().enumerate().find(is_disallowed) {
        // We need the +1 since the first character has been checked separately
        Some((pos, _)) => Err(E::invalid_character(
            // Handle UTF-8 chars correctly
            raw.chars().nth(pos + 1).unwrap(),
            pos + 1,
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ParserError {
        Empty,
        TooLong(usize),
        InvalidStart(char),
        InvalidCharacter(char, usize),
    }

    impl ParseError for ParserError {
        fn empty() -> Self {
            Self::Empty
        }

        fn too_long(current_length: usize) -> Self {
            Self::TooLong(current_length)
        }

        fn invalid_start(first: char) -> Self {
            Self::InvalidStart(first)
        }

        fn invalid_character(found: char, pos: usize) -> Self {
            Self::InvalidCharacter(found, pos)
        }
    }

    #[track_caller]
    fn validate(raw: &str, extra_allowed_chars: &[u8], max_len: usize) -> Result<(), ParserError> {
        super::validate(raw, extra_allowed_chars, max_len)
    }

    #[test]
    fn empty_fails() {
        let res = validate("", &[], 10);
        assert_eq!(res, Err(ParserError::Empty));
    }

    #[test]
    fn length_check() {
        let res = validate("abcdefghijklm", &[], 5);
        assert_eq!(res, Err(ParserError::TooLong(13)));

        let res = validate("abcdefghijklm", &[], 10);
        assert_eq!(res, Err(ParserError::TooLong(13)));

        let res = validate("abcdefghijklm", &[], 15);
        assert_eq!(res, Ok(()));
    }

    #[test]
    fn invalid_start() {
        let res = validate("Ab", &[b'A'], 5);
        assert_eq!(res, Err(ParserError::InvalidStart('A')));

        let res = validate("5b", &[b'5'], 5);
        assert_eq!(res, Err(ParserError::InvalidStart('5')));

        let res = validate("-b", &[b'_'], 5);
        assert_eq!(res, Err(ParserError::InvalidStart('-')));

        let res = validate("_b", &[b'_'], 5);
        assert_eq!(res, Err(ParserError::InvalidStart('_')));

        let res = validate("🦀b", &('🦀' as u32).to_ne_bytes(), 10);
        assert_eq!(res, Err(ParserError::InvalidStart('🦀')));
    }

    #[test]
    fn invalid_character() {
        let res = validate("bAc", &[], 5);
        assert_eq!(res, Err(ParserError::InvalidCharacter('A', 1)));

        let res = validate("bowl-", &[], 5);
        assert_eq!(res, Err(ParserError::InvalidCharacter('-', 4)));

        let res = validate("bob_", &[], 5);
        assert_eq!(res, Err(ParserError::InvalidCharacter('_', 3)));

        let res = validate("bo🦀", &[], 10);
        assert_eq!(res, Err(ParserError::InvalidCharacter('🦀', 2)));
    }

    #[test]
    fn basic_format() {
        let res = validate("abcdefghijklmnopqrstuvwxyz0123456789", &[], 36);
        assert_eq!(res, Ok(()));
    }

    #[test]
    fn extra_allowed_chars() {
        let res = validate("b0A_2d-c", &[b'A', b'_', b'-'], 10);
        assert_eq!(res, Ok(()));
    }
}
