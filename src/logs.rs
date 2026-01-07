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
        let uuid = uuid::Uuid::new_v4();
        let prefix = uuid.to_string()[..8].to_string();

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
