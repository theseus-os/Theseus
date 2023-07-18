use alloc::vec::Vec;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ParsedLine<'a> {
    /// The backgrounded tasks.
    ///
    /// Contains the raw job strings and the parsed jobs.
    pub(crate) background: Vec<(&'a str, ParsedJob<'a>)>,
    /// The foreground job.
    ///
    /// I.e. the final job in the line if it is not followed by a background
    /// operator.
    ///
    /// Contains the raw job string and the parsed job.
    pub(crate) foreground: Option<(&'a str, ParsedJob<'a>)>,
}

impl<'a> ParsedLine<'a> {
    pub(crate) fn is_empty(&self) -> bool {
        self.background.is_empty() && self.foreground.is_none()
    }
}

impl<'a> From<&'a str> for ParsedLine<'a> {
    fn from(line: &'a str) -> Self {
        let mut iter = line.split('&');

        // Iterator contains at least one element.
        let last = iter.next_back().unwrap();
        let trimmed = last.trim();
        let foreground = if trimmed.is_empty() {
            None
        } else {
            Some((last, parse_job(trimmed)))
        };

        ParsedLine {
            background: iter
                .clone()
                .zip(iter.map(str::trim).map(parse_job))
                .collect(),
            foreground,
        }
    }
}

/// A list of piped tasks.
///
/// # Examples
/// ```sh
/// sleep 1 | sleep 2 | sleep 3
/// ```
pub(crate) type ParsedJob<'a> = Vec<ParsedTask<'a>>;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ParsedTask<'a> {
    pub(crate) command: &'a str,
    pub(crate) args: Vec<&'a str>,
}

fn parse_job(job: &str) -> ParsedJob<'_> {
    job.split('|').map(str::trim).map(parse_task).collect()
}

fn parse_task(task: &str) -> ParsedTask {
    // TODO: Handle backslashes and quotes.
    if let Some((command, args_str)) = task.split_once(' ') {
        let args = args_str.split(' ').collect();
        ParsedTask { command, args }
    } else {
        ParsedTask {
            command: task,
            args: Vec::new(),
        }
    }
}
