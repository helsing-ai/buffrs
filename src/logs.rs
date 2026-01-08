use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use rand::{Rng, rng};
use tracing::Level;

/// BuffrsEventFormatter applies common formatting to tracing logs.
/// In verbose mode it outputs debug information to the terminal.
pub struct BuffrsEventFormatter {
    prefix: String,
    verbose: bool,
}

impl BuffrsEventFormatter {
    /// Create a new buffrs event formatter, optionally in verbose mode
    pub fn new(verbose: bool) -> Self {
        let mut rng = rng();

        let prefix = BASE64_STANDARD.encode(rng.random::<[u8; 6]>());

        Self { prefix, verbose }
    }
}

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for BuffrsEventFormatter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let metadata = event.metadata();

        if !self.verbose {
            match *metadata.level() {
                Level::INFO | Level::WARN | Level::ERROR => write!(writer, ":: ")?,
                Level::DEBUG | Level::TRACE => {}
            }

            ctx.field_format().format_fields(writer.by_ref(), event)?;

            return writeln!(writer);
        }

        write!(writer, "[{}] ", self.prefix)?;
        write!(writer, "{:5} ", metadata.level())?;

        if let Some(file) = metadata.file() {
            write!(writer, "{}:", file)?;
            if let Some(line) = metadata.line() {
                write!(writer, "{} ", line)?;
            } else {
                write!(writer, " ")?;
            }
        }
        write!(writer, "{}: ", metadata.target())?;

        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}
