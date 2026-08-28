mod args;
mod output;
mod search;
mod simple_regex;
mod updater;

use std::borrow::Cow;
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use args::{Cli, LineRange, LineReadCli, OutputFormat, ParsedArgs, UpdateCli};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::MemHistory;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Context, Editor, Helper};
use v_concat::*;

const ANSI_BLUE: &str = "\x1b[1;34m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_WHITE: &str = "\x1b[37m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ERROR_FG: &str = "\x1b[31m";
const LOADING_FG: &str = "\x1b[38;2;220;204;117m";
const RESULTS_FG: &str = "\x1b[38;2;22;140;200m";
const SUMMARY_FG: &str = "\x1b[38;2;253;219;51m";
const RESET_COLOR: &str = "\x1b[0m";
const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";
const INTERACTIVE_PROMPT: &str = "v_fs_sniffer> ";
const BANNER_SEARCH_INPUT: &str = r#"   .==============================.
  || FILES / DIRS / TEXT / REGEX  ||
  || ???????????????????????????? ||
   '==============\/=============='
                  \/
"#;
const BANNER_SEARCH_OUTPUT: &str = r#"           .--------------.
           |  deep clues  |
           '--------------' "#;
const BANNER_WORDMARK: &str = r#"
 __     __        _____ ____          ____   _   _ ___ _____ _____ _____ ____
 \ \   / /       |  ___/ ___|        / ___| | \ | |_ _|  ___|  ___| ____|  _ \
  \ \ / /        | |_  \___ \        \___ \ |  \| || || |_  | |_  |  _| | |_) |
   \ V /  ______ |  _|  ___) | ______  ___) | |\  || ||  _| |  _| | |___|  _ <
    \_/  |______||_|   |____/ |______||____/|_| \_|___|_|   |_|   |_____|_| \_\

"#;
const INTERACTIVE_COMMANDS: &[&str] = &[
    "--file",
    "--dir",
    "--str",
    "--regex",
    "-rx",
    "--replace-with",
    "--lines",
    "--check-update",
    "--update",
    "--github-repo",
    "--no-recursive",
    "-nr",
    "--case-sensitive",
    "-cs",
    "--no-follow-symlinks",
    "--output",
    "-o",
    "--output-format",
    "--json",
    "--text",
    "--quiet",
    "-q",
    "--exclude-dir",
    "-ex",
    "-ed",
    "--exclude-file",
    "-ef",
    "--exclude-extensions",
    "-ee",
    "--exclude-line",
    "-el",
    "--exclude-regex",
    "-er",
    "help",
    "clear",
    "version",
    "check-update",
    "update",
    "--version",
    "-V",
    "exit",
    "quit",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VFsSnifferError {
    message: String,
}

impl VFsSnifferError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for VFsSnifferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for VFsSnifferError {}

impl From<std::io::Error> for VFsSnifferError {
    fn from(value: std::io::Error) -> Self {
        Self::new(value.to_string())
    }
}

impl From<simple_regex::RegexCompileError> for VFsSnifferError {
    fn from(value: simple_regex::RegexCompileError) -> Self {
        Self::new(value.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct RunOutput {
    pub report: Option<search::SearchReport>,
    pub stdout: String,
    pub stderr: String,
}

fn main() {
    enable_virtual_terminal_colors();

    match run_from_env() {
        Ok(Some(output)) => {
            if !output.stdout.is_empty() {
                print_results(&output.stdout);
            }
            if !output.stderr.is_empty() {
                v_concat_eprint!("{}", output.stderr);
            }
        }
        Ok(None) => {}
        Err(err) => {
            print_error(&err.to_string());
            std::process::exit(2);
        }
    }
}

fn run_from_env() -> Result<Option<RunOutput>, VFsSnifferError> {
    match args::parse(env::args())? {
        ParsedArgs::Run(cli) => {
            let mut progress = StderrProgress::new();
            let result = run(cli, &mut progress);
            progress.clear();
            result.map(Some)
        }
        ParsedArgs::ReadLines(cli) => run_line_read(cli).map(Some),
        ParsedArgs::CheckUpdate(cli) => check_update(cli).map(Some),
        ParsedArgs::Update(cli) => install_update(cli).map(Some),
        ParsedArgs::Uninstall => {
            uninstall()?;
            Ok(None)
        }
        ParsedArgs::Interactive => {
            interactive_shell()?;
            Ok(None)
        }
        ParsedArgs::Help(text) | ParsedArgs::Version(text) => {
            v_concat_println!("{}", text);
            Ok(None)
        }
    }
}

fn run(
    cli: Cli,
    progress: &mut dyn search::ProgressReporter,
) -> Result<RunOutput, VFsSnifferError> {
    let report = search::search_with_progress(&cli, progress)?;
    let format = cli.effective_output_format();
    let rendered = output::render(&report, format);
    let stderr = output::render_warnings(&report);

    if let Some(path) = &cli.output {
        fs::write(path, rendered.as_bytes()).map_err(|err| {
            VFsSnifferError::new(format!(
                "failed to write output file '{}': {err}",
                path.display()
            ))
        })?;
    }

    let stdout = if cli.quiet { String::new() } else { rendered };

    Ok(RunOutput {
        report: Some(report),
        stdout,
        stderr,
    })
}

fn run_line_read(cli: LineReadCli) -> Result<RunOutput, VFsSnifferError> {
    let path = absolute_existing_file(&cli.file)?;
    let lines = read_file_lines(&path, cli.lines)?;
    let rendered = render_file_lines(&path, cli.lines, &lines, cli.effective_output_format());

    if let Some(path) = &cli.output {
        fs::write(path, rendered.as_bytes()).map_err(|err| {
            VFsSnifferError::new(format!(
                "failed to write output file '{}': {err}",
                path.display()
            ))
        })?;
    }

    let stdout = if cli.quiet { String::new() } else { rendered };

    Ok(RunOutput {
        report: None,
        stdout,
        stderr: String::new(),
    })
}

fn check_update(cli: UpdateCli) -> Result<RunOutput, VFsSnifferError> {
    let stdout = updater::check_update(cli.github_repo.as_deref())?;

    Ok(RunOutput {
        report: None,
        stdout,
        stderr: String::new(),
    })
}

fn install_update(cli: UpdateCli) -> Result<RunOutput, VFsSnifferError> {
    let stdout = updater::install_update(cli.github_repo.as_deref())?;

    Ok(RunOutput {
        report: None,
        stdout,
        stderr: String::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileLine {
    number: usize,
    text: String,
}

fn read_file_lines(path: &Path, range: LineRange) -> Result<Vec<FileLine>, VFsSnifferError> {
    let metadata = fs::metadata(path).map_err(|err| {
        VFsSnifferError::new(format!(
            "failed to read metadata for '{}': {err}",
            path.display()
        ))
    })?;

    if !metadata.is_file() {
        return Err(VFsSnifferError::new(format!(
            "'{}' is not a file",
            path.display()
        )));
    }

    let file = fs::File::open(path).map_err(|err| {
        VFsSnifferError::new(format!("failed to open file '{}': {err}", path.display()))
    })?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut current_line = 0usize;
    let mut lines = Vec::new();

    loop {
        buffer.clear();
        let bytes_read = reader.read_until(b'\n', &mut buffer).map_err(|err| {
            VFsSnifferError::new(format!("failed to read file '{}': {err}", path.display()))
        })?;

        if bytes_read == 0 {
            break;
        }

        current_line += 1;
        if current_line > range.end {
            break;
        }

        if current_line >= range.start {
            lines.push(FileLine {
                number: current_line,
                text: String::from_utf8_lossy(&buffer)
                    .trim_end_matches(['\r', '\n'])
                    .to_owned(),
            });
        }
    }

    Ok(lines)
}

fn render_file_lines(
    path: &Path,
    range: LineRange,
    lines: &[FileLine],
    format: OutputFormat,
) -> String {
    match format {
        OutputFormat::Text => render_file_lines_text(path, range, lines),
        OutputFormat::Json => render_file_lines_json(path, range, lines),
    }
}

fn render_file_lines_text(path: &Path, range: LineRange, lines: &[FileLine]) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n", path.display()));

    if lines.is_empty() {
        out.push_str(&format!(
            "No lines found in range {}:{}\n",
            range.start, range.end
        ));
        return out;
    }

    let width = range.end.to_string().len();
    for line in lines {
        out.push_str(&format!(
            "{:>width$} | {}\n",
            line.number,
            line.text,
            width = width
        ));
    }

    out
}

fn render_file_lines_json(path: &Path, range: LineRange, lines: &[FileLine]) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!(
        "  \"path\": \"{}\",\n",
        json_escape(&path.display().to_string())
    ));
    out.push_str(&format!(
        "  \"range\": {{ \"start\": {}, \"end\": {} }},\n",
        range.start, range.end
    ));
    out.push_str("  \"lines\": [\n");

    for (index, line) in lines.iter().enumerate() {
        out.push_str(&format!(
            "    {{ \"line\": {}, \"text\": \"{}\" }}",
            line.number,
            json_escape(&line.text)
        ));
        if index + 1 != lines.len() {
            out.push(',');
        }
        out.push('\n');
    }

    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

fn absolute_existing_file(path: &Path) -> Result<PathBuf, VFsSnifferError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };

    fs::canonicalize(&absolute).map_err(|err| {
        VFsSnifferError::new(format!(
            "failed to resolve file '{}': {err}",
            path.display()
        ))
    })
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }

    escaped
}

struct StderrProgress {
    frame_index: usize,
    last_width: usize,
    color_enabled: bool,
    started: bool,
}

impl StderrProgress {
    fn new() -> Self {
        Self {
            frame_index: 0,
            last_width: 0,
            color_enabled: io::stderr().is_terminal(),
            started: false,
        }
    }

    fn clear(&mut self) {
        if self.last_width == 0 {
            return;
        }

        v_concat_eprint!("\r{}\r", " ".repeat(self.last_width));
        let _ = io::stderr().flush();
        self.last_width = 0;
    }
}

impl search::ProgressReporter for StderrProgress {
    fn reading(&mut self, path: &Path) {
        const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

        if !self.started {
            v_concat_eprint!("\n");
            self.started = true;
        }

        let frame = FRAMES[self.frame_index % FRAMES.len()];
        self.frame_index += 1;

        let line = format!("{frame} reading {}", path.display());
        let width = line.chars().count();
        let padding = self.last_width.saturating_sub(width);
        let padding_spaces = " ".repeat(padding);

        if self.color_enabled {
            v_concat_eprint!("\r{}{}{}{}", LOADING_FG, line, RESET_COLOR, padding_spaces);
        } else {
            v_concat_eprint!("\r{}{}", line, padding_spaces);
        }
        let _ = io::stderr().flush();
        self.last_width = width;
    }
}

fn print_results(text: &str) {
    if io::stdout().is_terminal() {
        v_concat_println!("\n{}{}", colorize_results(text), RESET_COLOR);
    } else {
        v_concat_println!("\n{}", text);
    }
}

fn colorize_results(text: &str) -> String {
    let mut out = String::new();
    out.push_str(RESULTS_FG);

    for line in text.split_inclusive('\n') {
        if line.starts_with("Summary:") {
            out.push_str(SUMMARY_FG);
            out.push_str(line);
            out.push_str(RESULTS_FG);
        } else {
            out.push_str(line);
        }
    }

    out
}

fn print_error(message: &str) {
    if io::stderr().is_terminal() {
        v_concat_eprintln!("\n{}error: {}{}\n", ERROR_FG, message, RESET_COLOR);
    } else {
        v_concat_eprintln!("\nerror: {}\n", message);
    }
}

fn print_banner() {
    if io::stdout().is_terminal() {
        v_concat_println!(
            "\n{}{}{}{}{}{}{}v_fs_sniffer: deep filesystem searches for files, directories, text, regexes, and clues.\n\nType help for commands, clear to redraw this screen, press Tab to autocomplete commands and paths, or exit to quit.\n",
            ANSI_CYAN,
            BANNER_SEARCH_INPUT,
            ANSI_YELLOW,
            BANNER_SEARCH_OUTPUT,
            ANSI_BLUE,
            BANNER_WORDMARK,
            RESET_COLOR
        );
    } else {
        v_concat_println!(
            "\n{}{}{}v_fs_sniffer: deep filesystem searches for files, directories, text, regexes, and clues.\n\nType help for commands, clear to redraw this screen, press Tab to autocomplete commands and paths, or exit to quit.\n",
            BANNER_SEARCH_INPUT,
            BANNER_SEARCH_OUTPUT,
            BANNER_WORDMARK
        );
    }
}

fn clear_console_and_print_banner() -> Result<(), VFsSnifferError> {
    v_concat_print!("{}", CLEAR_SCREEN);
    io::stdout().flush()?;
    print_banner();
    Ok(())
}

fn interactive_shell() -> Result<(), VFsSnifferError> {
    print_banner();

    let config = Config::builder()
        .color_mode(rustyline::ColorMode::Forced)
        .completion_type(CompletionType::List)
        .completion_show_all_if_ambiguous(true)
        .build();
    let mut editor =
        Editor::<VFsSnifferLineHelper, MemHistory>::with_history(config, MemHistory::new())
            .map_err(|err| {
                VFsSnifferError::new(format!("failed to start interactive editor: {err}"))
            })?;
    editor.set_helper(Some(VFsSnifferLineHelper::new()));

    loop {
        match editor.readline(INTERACTIVE_PROMPT) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let _ = editor.add_history_entry(trimmed);
                if !run_interactive_line(trimmed)? {
                    v_concat_println!();
                    return Ok(());
                }
            }
            Err(ReadlineError::Interrupted) => {
                v_concat_println!();
                continue;
            }
            Err(ReadlineError::Eof) => {
                v_concat_println!();
                return Ok(());
            }
            Err(err) => {
                return Err(VFsSnifferError::new(format!(
                    "interactive input failed: {err}"
                )));
            }
        }
    }
}

fn run_interactive_line(trimmed: &str) -> Result<bool, VFsSnifferError> {
    if trimmed.eq_ignore_ascii_case("exit") || trimmed.eq_ignore_ascii_case("quit") {
        return Ok(false);
    }
    if trimmed.eq_ignore_ascii_case("clear") {
        clear_console_and_print_banner()?;
        return Ok(true);
    }
    if trimmed.eq_ignore_ascii_case("help") {
        v_concat_println!("\n{}", args::usage());
        return Ok(true);
    }
    if trimmed.eq_ignore_ascii_case("version") {
        v_concat_println!("\n{}", args::version_text());
        return Ok(true);
    }
    if trimmed.eq_ignore_ascii_case("check-update") {
        match check_update(UpdateCli { github_repo: None }) {
            Ok(output) => {
                if !output.stdout.is_empty() {
                    print_results(&output.stdout);
                }
            }
            Err(err) => print_error(&err.to_string()),
        }
        return Ok(true);
    }
    if trimmed.eq_ignore_ascii_case("update") {
        match install_update(UpdateCli { github_repo: None }) {
            Ok(output) => {
                if !output.stdout.is_empty() {
                    print_results(&output.stdout);
                }
            }
            Err(err) => print_error(&err.to_string()),
        }
        return Ok(true);
    }

    let tokens = match split_interactive_line(trimmed) {
        Ok(tokens) => tokens,
        Err(err) => {
            print_error(&err.to_string());
            return Ok(true);
        }
    };
    if tokens.is_empty() {
        return Ok(true);
    }

    let tokens = strip_program_name(tokens);
    let parse_args = std::iter::once("v_fs_sniffer".to_owned()).chain(tokens);
    match args::parse(parse_args) {
        Ok(ParsedArgs::Run(cli)) => {
            let mut progress = StderrProgress::new();
            let result = run(cli, &mut progress);
            progress.clear();
            match result {
                Ok(output) => {
                    if !output.stdout.is_empty() {
                        print_results(&output.stdout);
                    }
                    if !output.stderr.is_empty() {
                        v_concat_eprint!("{}", output.stderr);
                    }
                }
                Err(err) => print_error(&err.to_string()),
            }
        }
        Ok(ParsedArgs::ReadLines(cli)) => match run_line_read(cli) {
            Ok(output) => {
                if !output.stdout.is_empty() {
                    print_results(&output.stdout);
                }
                if !output.stderr.is_empty() {
                    v_concat_eprint!("{}", output.stderr);
                }
            }
            Err(err) => print_error(&err.to_string()),
        },
        Ok(ParsedArgs::CheckUpdate(cli)) => match check_update(cli) {
            Ok(output) => {
                if !output.stdout.is_empty() {
                    print_results(&output.stdout);
                }
            }
            Err(err) => print_error(&err.to_string()),
        },
        Ok(ParsedArgs::Update(cli)) => match install_update(cli) {
            Ok(output) => {
                if !output.stdout.is_empty() {
                    print_results(&output.stdout);
                }
            }
            Err(err) => print_error(&err.to_string()),
        },
        Ok(ParsedArgs::Help(text) | ParsedArgs::Version(text)) => {
            v_concat_println!("\n{}", text);
        }
        Ok(ParsedArgs::Uninstall) => {
            if let Err(err) = uninstall() {
                print_error(&err.to_string());
            }
        }
        Ok(ParsedArgs::Interactive) => {}
        Err(err) => print_error(&err.to_string()),
    }

    Ok(true)
}

fn split_interactive_line(line: &str) -> Result<Vec<String>, VFsSnifferError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;

    for ch in line.chars() {
        match ch {
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            ch => current.push(ch),
        }
    }

    if let Some(ch) = quote {
        return Err(VFsSnifferError::new(format!("unterminated {ch} quote")));
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

fn strip_program_name(mut tokens: Vec<String>) -> Vec<String> {
    if tokens
        .first()
        .is_some_and(|token| token == "v_fs_sniffer" || token == "v_fs_sniffer.exe")
    {
        tokens.remove(0);
    }
    tokens
}

#[derive(Debug, Clone, Copy)]
struct VFsSnifferLineHelper;

impl VFsSnifferLineHelper {
    fn new() -> Self {
        Self
    }
}

impl Helper for VFsSnifferLineHelper {}

impl Hinter for VFsSnifferLineHelper {
    type Hint = String;
}

impl Highlighter for VFsSnifferLineHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        let _ = default;
        if prompt == INTERACTIVE_PROMPT {
            Cow::Owned(format!("{ANSI_CYAN}{prompt}{ANSI_WHITE}"))
        } else {
            Cow::Borrowed(prompt)
        }
    }
}

impl Validator for VFsSnifferLineHelper {}

impl Completer for VFsSnifferLineHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let token = completion_token(line, pos);
        let pairs = if should_complete_command(line, &token) {
            command_completion_pairs(&token.unquoted)
        } else {
            path_completion_pairs(&token)
        };

        Ok((token.start, pairs))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionToken {
    start: usize,
    raw: String,
    unquoted: String,
    quote: Option<char>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathCompletionPrefix {
    parent: PathBuf,
    partial: String,
    display_prefix: String,
    separator: char,
}

fn completion_token(line: &str, pos: usize) -> CompletionToken {
    let pos = previous_char_boundary(line, pos.min(line.len()));
    let before = &line[..pos];
    let mut token_start = 0;
    let mut content_start = 0;
    let mut quote = None;
    let mut token_quote = None;

    for (index, ch) in before.char_indices() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => {}
            None if ch.is_whitespace() || ch == '=' => {
                token_start = index + ch.len_utf8();
                content_start = token_start;
                token_quote = None;
            }
            None if ch == '\'' || ch == '"' => {
                if index == token_start {
                    content_start = index + ch.len_utf8();
                    token_quote = Some(ch);
                }
                quote = Some(ch);
            }
            None => {}
        }
    }

    let raw = before[token_start..].to_owned();
    let unquoted = if let Some(open) = token_quote {
        let mut value = before[content_start..].to_owned();
        if value.ends_with(open) {
            value.pop();
        }
        value
    } else {
        raw.clone()
    };

    CompletionToken {
        start: token_start,
        raw,
        unquoted,
        quote: token_quote,
    }
}

fn previous_char_boundary(line: &str, mut pos: usize) -> usize {
    while pos > 0 && !line.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn should_complete_command(line: &str, token: &CompletionToken) -> bool {
    line[..token.start].trim().is_empty() || token.unquoted.starts_with('-')
}

fn command_completion_pairs(prefix: &str) -> Vec<Pair> {
    let mut pairs = INTERACTIVE_COMMANDS
        .iter()
        .filter(|command| command.starts_with(prefix))
        .map(|command| Pair {
            display: (*command).to_owned(),
            replacement: format!("{command} "),
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.display.cmp(&right.display));
    pairs
}

fn path_completion_pairs(token: &CompletionToken) -> Vec<Pair> {
    let prefix = split_completion_path(&token.unquoted);
    let Ok(entries) = fs::read_dir(&prefix.parent) else {
        return Vec::new();
    };

    let mut pairs = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name_matches_prefix(&name, &prefix.partial) {
                return None;
            }

            let suffix = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                prefix.separator.to_string()
            } else {
                String::new()
            };
            let path = format!("{}{}{}", prefix.display_prefix, name, suffix);
            Some(Pair {
                display: path.clone(),
                replacement: quote_path_completion(&path, token.quote),
            })
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.display.cmp(&right.display));
    pairs.truncate(64);
    pairs
}

fn split_completion_path(prefix: &str) -> PathCompletionPrefix {
    let separator = preferred_path_separator(prefix);
    let Some(index) = prefix.rfind(|ch| ch == '/' || ch == '\\') else {
        return PathCompletionPrefix {
            parent: PathBuf::from("."),
            partial: prefix.to_owned(),
            display_prefix: String::new(),
            separator,
        };
    };

    let display_prefix = prefix[..=index].to_owned();
    let parent = if index == 0 {
        PathBuf::from(&display_prefix)
    } else if is_windows_drive_root(&display_prefix) {
        PathBuf::from(&display_prefix)
    } else {
        expand_completion_parent(&prefix[..index])
    };

    PathCompletionPrefix {
        parent,
        partial: prefix[index + 1..].to_owned(),
        display_prefix,
        separator,
    }
}

fn preferred_path_separator(prefix: &str) -> char {
    match (prefix.rfind('/'), prefix.rfind('\\')) {
        (Some(slash), Some(backslash)) if backslash > slash => '\\',
        (Some(_), _) => '/',
        (_, Some(_)) => '\\',
        (None, None) => std::path::MAIN_SEPARATOR,
    }
}

fn is_windows_drive_root(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    bytes.len() == 3 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn expand_completion_parent(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }

    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }

    PathBuf::from(path)
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from).or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            Some(PathBuf::from(format!(
                "{}{}",
                drive.to_string_lossy(),
                path.to_string_lossy()
            )))
        })
    }

    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn name_matches_prefix(name: &str, partial: &str) -> bool {
    if cfg!(windows) {
        name.to_ascii_lowercase()
            .starts_with(&partial.to_ascii_lowercase())
    } else {
        name.starts_with(partial)
    }
}

fn quote_path_completion(path: &str, quote: Option<char>) -> String {
    let needs_quotes = quote.is_some() || path.chars().any(char::is_whitespace);
    if !needs_quotes {
        return path.to_owned();
    }

    if quote == Some('\'') && !path.contains('\'') {
        return format!("'{path}'");
    }
    if quote == Some('"') && !path.contains('"') {
        return format!("\"{path}\"");
    }
    if !path.contains('"') {
        return format!("\"{path}\"");
    }
    if !path.contains('\'') {
        return format!("'{path}'");
    }

    path.to_owned()
}

fn uninstall() -> Result<(), VFsSnifferError> {
    v_concat_println!(
        "\n{}\n{}\n{}\n{}",
        "Uninstalling v_fs_sniffer with Cargo.",
        "This removes a per-user Cargo install and its Cargo install metadata only.",
        "For system-wide packages, uninstall with your OS package manager.",
        "The source checkout is not modified."
    );

    uninstall_current_package()
}

#[cfg(not(windows))]
fn uninstall_current_package() -> Result<(), VFsSnifferError> {
    let status = Command::new("cargo")
        .args(["uninstall", "v_fs_sniffer"])
        .status()
        .map_err(|err| VFsSnifferError::new(format!("failed to run 'cargo uninstall': {err}")))?;

    if status.success() {
        Ok(())
    } else {
        Err(VFsSnifferError::new(format!(
            "'cargo uninstall v_fs_sniffer' exited with status {status}"
        )))
    }
}

#[cfg(windows)]
fn uninstall_current_package() -> Result<(), VFsSnifferError> {
    let script = env::temp_dir().join(format!("v_fs_sniffer_uninstall_{}.cmd", std::process::id()));
    let script_body = format!(
        "@echo off\r\n\
         timeout /t 1 /nobreak >nul\r\n\
         cargo uninstall v_fs_sniffer\r\n\
         set exit_code=%ERRORLEVEL%\r\n\
         del \"{}\" >nul 2>nul\r\n\
         exit /b %exit_code%\r\n",
        script.display()
    );

    fs::write(&script, script_body).map_err(|err| {
        VFsSnifferError::new(format!(
            "failed to create uninstall script '{}': {err}",
            script.display()
        ))
    })?;

    let script_command = format!("\"{}\"", script.display());
    Command::new("cmd")
        .args(["/C", &script_command])
        .spawn()
        .map_err(|err| VFsSnifferError::new(format!("failed to start uninstall script: {err}")))?;

    v_concat_println!(
        "\n{}",
        "The uninstall command has been started and will run after this process exits."
    );
    Ok(())
}

#[cfg(windows)]
fn enable_virtual_terminal_colors() {
    enable_virtual_terminal_colors_for_handle(STD_OUTPUT_HANDLE);
    enable_virtual_terminal_colors_for_handle(STD_ERROR_HANDLE);
}

#[cfg(not(windows))]
fn enable_virtual_terminal_colors() {}

#[cfg(windows)]
fn enable_virtual_terminal_colors_for_handle(handle_id: u32) {
    use std::ffi::c_void;

    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> *mut c_void;
        fn GetConsoleMode(hConsoleHandle: *mut c_void, lpMode: *mut u32) -> i32;
        fn SetConsoleMode(hConsoleHandle: *mut c_void, dwMode: u32) -> i32;
    }

    let handle = unsafe { GetStdHandle(handle_id) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return;
    }

    let mut mode = 0u32;
    if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
        return;
    }

    let _ = unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) };
}

#[cfg(windows)]
const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;

#[cfg(windows)]
const STD_ERROR_HANDLE: u32 = -12i32 as u32;

#[cfg(test)]
mod tests {
    use super::{
        colorize_results, command_completion_pairs, completion_token, quote_path_completion,
        split_completion_path, split_interactive_line, strip_program_name, VFsSnifferLineHelper,
        ANSI_CYAN, ANSI_WHITE, CLEAR_SCREEN, INTERACTIVE_PROMPT, RESULTS_FG, SUMMARY_FG,
    };
    use rustyline::highlight::Highlighter;

    #[test]
    fn colorize_results_makes_summary_yellow() {
        let colored = colorize_results("| Kind |\nSummary: 1 matches\n");

        assert!(colored.starts_with(RESULTS_FG));
        assert!(colored.contains(&format!("{SUMMARY_FG}Summary: 1 matches\n{RESULTS_FG}")));
    }

    #[test]
    fn interactive_prompt_is_cyan_and_input_returns_to_white() {
        let helper = VFsSnifferLineHelper::new();
        let highlighted = helper.highlight_prompt(INTERACTIVE_PROMPT, true);

        assert_eq!(
            highlighted,
            format!("{ANSI_CYAN}{INTERACTIVE_PROMPT}{ANSI_WHITE}")
        );
    }

    #[test]
    fn clear_command_uses_full_screen_redraw_sequence() {
        assert_eq!(CLEAR_SCREEN, "\x1b[2J\x1b[H");
    }

    #[test]
    fn version_command_is_available_to_interactive_completion() {
        let pairs = command_completion_pairs("ver");

        assert!(pairs.iter().any(|pair| pair.replacement == "version "));
    }

    #[test]
    fn interactive_split_keeps_quoted_arguments() {
        let tokens = split_interactive_line(r#"--str "needle value" ."#).unwrap();

        assert_eq!(tokens, ["--str", "needle value", "."]);
    }

    #[test]
    fn interactive_split_keeps_windows_backslashes_inside_quotes() {
        let tokens = split_interactive_line(r#"--file needle "C:\Program Files\""#).unwrap();

        assert_eq!(tokens, ["--file", "needle", r"C:\Program Files\"]);
    }

    #[test]
    fn completion_token_keeps_quoted_path_spaces() {
        let line = r#"--str needle "C:\Program Files"#;
        let token = completion_token(line, line.len());

        assert_eq!(token.unquoted, r"C:\Program Files");
        assert_eq!(token.quote, Some('"'));
    }

    #[test]
    fn completion_quotes_paths_with_spaces() {
        assert_eq!(
            quote_path_completion(r"C:\Program Files\", None),
            r#""C:\Program Files\""#,
        );
    }

    #[test]
    fn completion_splits_windows_drive_prefix() {
        let prefix = split_completion_path(r"C:\Pro");

        assert_eq!(prefix.parent, std::path::PathBuf::from(r"C:\"));
        assert_eq!(prefix.partial, "Pro");
        assert_eq!(prefix.display_prefix, r"C:\");
        assert_eq!(prefix.separator, '\\');
    }

    #[test]
    fn interactive_strip_allows_copy_pasted_commands() {
        let tokens = strip_program_name(vec![
            "v_fs_sniffer".to_owned(),
            "--file".to_owned(),
            "Cargo.toml".to_owned(),
            ".".to_owned(),
        ]);

        assert_eq!(tokens, ["--file", "Cargo.toml", "."]);
    }
}
