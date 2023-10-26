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

use std::fmt::{self, Display, Formatter};

use miette::{
    Diagnostic, LabeledSpan, MietteError, MietteSpanContents, Severity, SourceCode, SourceSpan,
    SpanContents,
};

/// Severity level of violation.
#[derive(Clone, Debug, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// Location of violation.
#[derive(Default, PartialEq, Clone, Eq, Debug)]
pub struct Location {
    /// File that contains violation
    pub file: Option<String>,
    /// Package name of file containing violation
    pub package: Option<String>,
    /// Entity name containing the violation
    pub entity: Option<String>,
}

/// Violation message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct Message {
    /// Message describing violation
    pub message: String,
    /// Information on what went wrong
    pub help: String,
}

impl Diagnostic for Message {}

/// Rule violation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Rule name that was violated
    pub rule: String,
    /// Level of violation
    pub level: Level,
    /// Message
    pub message: Message,
    /// Location where violation occured
    pub location: Location,
    /// Help text
    pub info: String,
}

/// Alias for list of [`Violation`].
pub type Violations = Vec<Violation>;

impl std::error::Error for Violation {}

impl Display for Violation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.info)
    }
}

impl Diagnostic for Violation {
    fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        Some(Box::new(self.rule.split("::").last().unwrap_or(&self.rule)))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        Some(Box::new(format!(
            "https://helsing-ai.github.io/buffrs/rules/{}",
            self.rule
        )))
    }

    fn severity(&self) -> Option<Severity> {
        let level = match self.level {
            Level::Info => Severity::Advice,
            Level::Warning => Severity::Warning,
            Level::Error => Severity::Error,
        };

        Some(level)
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.location)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(
            [LabeledSpan::new(Some("file".into()), 0, 0)].into_iter(),
        ))
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        Some(&self.message)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        Some(Box::new(&self.message.help))
    }
}

impl SourceCode for Location {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        _context_lines_before: usize,
        _context_lines_after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
        Ok(Box::new(MietteSpanContents::new_named(
            self.file.clone().unwrap_or_default(),
            &[],
            *span,
            0,
            0,
            0,
        )))
    }
}
