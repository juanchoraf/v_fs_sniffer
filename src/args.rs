use std::ffi::OsString;
use std::path::PathBuf;

use crate::VFsSnifferError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchMode {
    File(String),
    Dir(String),
    Str(String),
    Regex(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub mode: SearchMode,
    pub root: PathBuf,
    pub replace_with: Option<String>,
    pub recursive: bool,
    pub case_sensitive: bool,
    pub follow_symlinks: bool,
    pub output: Option<PathBuf>,
    pub output_format: Option<OutputFormat>,
    pub quiet: bool,
    pub exclude_dirs: Vec<String>,
    pub exclude_files: Vec<String>,
    pub exclude_extensions: Vec<String>,
    pub exclude_lines: Vec<String>,
    pub exclude_regexes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineReadCli {
    pub file: PathBuf,
    pub lines: LineRange,
    pub output: Option<PathBuf>,
    pub output_format: Option<OutputFormat>,
    pub quiet: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCli {
    pub github_repo: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    Check,
    Install,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedArgs {
    Run(Cli),
    ReadLines(LineReadCli),
    CheckUpdate(UpdateCli),
    Update(UpdateCli),
    Help(String),
    Interactive,
    Uninstall,
    Version(String),
}

impl Cli {
    pub fn effective_output_format(&self) -> OutputFormat {
        if let Some(format) = self.output_format {
            return format;
        }

        if self
            .output
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            return OutputFormat::Json;
        }

        OutputFormat::Text
    }
}

impl LineReadCli {
    pub fn effective_output_format(&self) -> OutputFormat {
        if let Some(format) = self.output_format {
            return format;
        }

        if self
            .output
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            return OutputFormat::Json;
        }

        OutputFormat::Text
    }
}

#[derive(Debug, Default)]
struct CliBuilder {
    mode: Option<SearchMode>,
    root: Option<PathBuf>,
    lines: Option<LineRange>,
    replace_with: Option<String>,
    recursive: bool,
    case_sensitive: bool,
    follow_symlinks: bool,
    output: Option<PathBuf>,
    output_format: Option<OutputFormat>,
    quiet: bool,
    update_mode: Option<UpdateMode>,
    github_repo: Option<String>,
    exclude_dirs: Vec<String>,
    exclude_files: Vec<String>,
    exclude_extensions: Vec<String>,
    exclude_lines: Vec<String>,
    exclude_regexes: Vec<String>,
}

pub fn parse<I, S>(args: I) -> Result<ParsedArgs, VFsSnifferError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args
        .into_iter()
        .map(|arg| arg.into().to_string_lossy().into_owned());

    let _program = args.next();
    let mut args = args.peekable();
    if args.peek().is_none() {
        return Ok(ParsedArgs::Interactive);
    }

    let mut builder = CliBuilder {
        recursive: true,
        follow_symlinks: true,
        ..CliBuilder::default()
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(ParsedArgs::Help(usage())),
            "--uninstall" => return Ok(ParsedArgs::Uninstall),
            "--check-update" => set_update_mode(&mut builder, UpdateMode::Check)?,
            "--update" => set_update_mode(&mut builder, UpdateMode::Install)?,
            "--github-repo" => set_github_repo(&mut builder, next_value(&mut args, &arg)?)?,
            "-V" | "--version" => return Ok(ParsedArgs::Version(version_text())),
            "--file" => set_mode(&mut builder, SearchMode::File(next_value(&mut args, &arg)?))?,
            "--dir" => set_mode(&mut builder, SearchMode::Dir(next_value(&mut args, &arg)?))?,
            "--str" => set_mode(&mut builder, SearchMode::Str(next_value(&mut args, &arg)?))?,
            "--replace-with" => set_replace_with(&mut builder, next_value(&mut args, &arg)?)?,
            "--regex" | "-rx" => set_mode(
                &mut builder,
                SearchMode::Regex(next_value(&mut args, &arg)?),
            )?,
            "-nr" | "--no-recursive" => builder.recursive = false,
            "-cs" | "--case-sensitive" => builder.case_sensitive = true,
            "--no-follow-symlinks" => builder.follow_symlinks = false,
            "--lines" => set_lines(
                &mut builder,
                parse_line_range(&next_value(&mut args, &arg)?)?,
            )?,
            "-q" | "--quiet" => builder.quiet = true,
            "-o" | "--output" | "--export" => {
                builder.output = Some(PathBuf::from(next_value(&mut args, &arg)?));
            }
            "--output-format" => {
                builder.output_format = Some(parse_output_format(&next_value(&mut args, &arg)?)?);
            }
            "--json" => builder.output_format = Some(OutputFormat::Json),
            "--text" => builder.output_format = Some(OutputFormat::Text),
            "--exclude-dir" | "-ed" | "-ex" => {
                builder.exclude_dirs.push(next_value(&mut args, &arg)?);
            }
            "--exclude-file" | "-ef" => builder.exclude_files.push(next_value(&mut args, &arg)?),
            "--exclude-extensions" | "-ee" => {
                push_exclude_extensions(&mut builder, &next_value(&mut args, &arg)?);
            }
            "--exclude-line" | "-el" => builder.exclude_lines.push(next_value(&mut args, &arg)?),
            "--exclude-regex" | "-er" => {
                builder.exclude_regexes.push(next_value(&mut args, &arg)?);
            }
            "--" => {
                let value = args
                    .next()
                    .ok_or_else(|| VFsSnifferError::new("expected a search root after '--'"))?;
                set_root(&mut builder, PathBuf::from(value))?;
                for trailing in args {
                    set_root(&mut builder, PathBuf::from(trailing))?;
                }
                break;
            }
            _ => {
                if let Some((name, value)) = split_assignment(&arg) {
                    match name {
                        "--file" => set_mode(&mut builder, SearchMode::File(value.to_owned()))?,
                        "--dir" => set_mode(&mut builder, SearchMode::Dir(value.to_owned()))?,
                        "--str" => set_mode(&mut builder, SearchMode::Str(value.to_owned()))?,
                        "--replace-with" => set_replace_with(&mut builder, value.to_owned())?,
                        "--check-update" => {
                            if !value.is_empty() {
                                return Err(VFsSnifferError::new(
                                    "--check-update does not take a value",
                                ));
                            }
                            set_update_mode(&mut builder, UpdateMode::Check)?
                        }
                        "--update" => {
                            if !value.is_empty() {
                                return Err(VFsSnifferError::new("--update does not take a value"));
                            }
                            set_update_mode(&mut builder, UpdateMode::Install)?
                        }
                        "--github-repo" => set_github_repo(&mut builder, value.to_owned())?,
                        "--regex" | "-rx" => {
                            set_mode(&mut builder, SearchMode::Regex(value.to_owned()))?
                        }
                        "-o" | "--output" | "--export" => {
                            builder.output = Some(PathBuf::from(value));
                        }
                        "--output-format" => {
                            builder.output_format = Some(parse_output_format(value)?);
                        }
                        "--lines" => set_lines(&mut builder, parse_line_range(value)?)?,
                        "--no-follow-symlinks" => builder.follow_symlinks = false,
                        "--exclude-dir" | "-ed" | "-ex" => {
                            builder.exclude_dirs.push(value.to_owned());
                        }
                        "--exclude-file" | "-ef" => builder.exclude_files.push(value.to_owned()),
                        "--exclude-extensions" | "-ee" => {
                            push_exclude_extensions(&mut builder, value);
                        }
                        "--exclude-line" | "-el" => builder.exclude_lines.push(value.to_owned()),
                        "--exclude-regex" | "-er" => {
                            builder.exclude_regexes.push(value.to_owned());
                        }
                        _ if arg.starts_with('-') => {
                            return Err(VFsSnifferError::new(format!("unknown option '{arg}'")));
                        }
                        _ => set_root(&mut builder, PathBuf::from(arg))?,
                    }
                } else if arg.starts_with('-') {
                    return Err(VFsSnifferError::new(format!("unknown option '{arg}'")));
                } else {
                    set_root(&mut builder, PathBuf::from(arg))?;
                }
            }
        }
    }

    if let Some(update_mode) = builder.update_mode {
        if builder_has_search_input(&builder) {
            return Err(VFsSnifferError::new(
                "--update and --check-update cannot be combined with search options",
            ));
        }

        let cli = UpdateCli {
            github_repo: builder.github_repo,
        };
        return Ok(match update_mode {
            UpdateMode::Check => ParsedArgs::CheckUpdate(cli),
            UpdateMode::Install => ParsedArgs::Update(cli),
        });
    }

    let mode = builder.mode.ok_or_else(|| {
        VFsSnifferError::new("missing search mode: use --file, --dir, --str, or --regex")
    })?;

    if let Some(lines) = builder.lines {
        let SearchMode::File(file) = mode else {
            return Err(VFsSnifferError::new("--lines can only be used with --file"));
        };

        if file.is_empty() {
            return Err(VFsSnifferError::new(
                "--file requires a non-empty file path when used with --lines",
            ));
        }

        if builder.root.is_some() {
            return Err(VFsSnifferError::new(
                "--lines reads a direct --file path and does not take a search root",
            ));
        }

        if builder.replace_with.is_some() {
            return Err(VFsSnifferError::new(
                "--lines cannot be used with --replace-with",
            ));
        }

        return Ok(ParsedArgs::ReadLines(LineReadCli {
            file: PathBuf::from(file),
            lines,
            output: builder.output,
            output_format: builder.output_format,
            quiet: builder.quiet,
        }));
    }

    let root = builder
        .root
        .ok_or_else(|| VFsSnifferError::new("missing search root path"))?;

    if builder.replace_with.is_some() && matches!(&mode, SearchMode::Regex(_)) {
        return Err(VFsSnifferError::new(
            "--replace-with can only be used with --file, --dir, or --str",
        ));
    }

    Ok(ParsedArgs::Run(Cli {
        mode,
        root,
        replace_with: builder.replace_with,
        recursive: builder.recursive,
        case_sensitive: builder.case_sensitive,
        follow_symlinks: builder.follow_symlinks,
        output: builder.output,
        output_format: builder.output_format,
        quiet: builder.quiet,
        exclude_dirs: builder.exclude_dirs,
        exclude_files: builder.exclude_files,
        exclude_extensions: builder.exclude_extensions,
        exclude_lines: builder.exclude_lines,
        exclude_regexes: builder.exclude_regexes,
    }))
}

fn split_assignment(arg: &str) -> Option<(&str, &str)> {
    arg.split_once('=')
        .filter(|(name, _)| name.starts_with('-'))
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, VFsSnifferError> {
    args.next()
        .ok_or_else(|| VFsSnifferError::new(format!("expected a value after '{flag}'")))
}

fn push_exclude_extensions(builder: &mut CliBuilder, value: &str) {
    builder
        .exclude_extensions
        .extend(split_extensions(value).map(str::to_owned));
}

fn split_extensions(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
        .map(|part| part.trim().trim_start_matches('.'))
        .filter(|part| !part.is_empty())
}

fn set_mode(builder: &mut CliBuilder, mode: SearchMode) -> Result<(), VFsSnifferError> {
    if builder.mode.is_some() {
        return Err(VFsSnifferError::new(
            "only one search mode is allowed: choose --file, --dir, --str, or --regex",
        ));
    }

    builder.mode = Some(mode);
    Ok(())
}

fn set_replace_with(builder: &mut CliBuilder, value: String) -> Result<(), VFsSnifferError> {
    if builder.replace_with.is_some() {
        return Err(VFsSnifferError::new(
            "only one --replace-with option is allowed per run",
        ));
    }

    builder.replace_with = Some(value);
    Ok(())
}

fn set_update_mode(builder: &mut CliBuilder, value: UpdateMode) -> Result<(), VFsSnifferError> {
    if builder.update_mode.is_some() {
        return Err(VFsSnifferError::new(
            "only one update option is allowed per run",
        ));
    }

    builder.update_mode = Some(value);
    Ok(())
}

fn set_github_repo(builder: &mut CliBuilder, value: String) -> Result<(), VFsSnifferError> {
    if builder.github_repo.is_some() {
        return Err(VFsSnifferError::new(
            "only one --github-repo option is allowed per run",
        ));
    }
    if !is_valid_github_repo(&value) {
        return Err(VFsSnifferError::new(format!(
            "invalid GitHub repository '{value}', expected owner/repo"
        )));
    }

    builder.github_repo = Some(value);
    Ok(())
}

fn is_valid_github_repo(value: &str) -> bool {
    let mut parts = value.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };

    !owner.is_empty()
        && !repo.is_empty()
        && parts.next().is_none()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'))
}

fn builder_has_search_input(builder: &CliBuilder) -> bool {
    builder.mode.is_some()
        || builder.root.is_some()
        || builder.lines.is_some()
        || builder.replace_with.is_some()
        || builder.output.is_some()
        || builder.output_format.is_some()
        || builder.quiet
        || !builder.recursive
        || builder.case_sensitive
        || !builder.follow_symlinks
        || !builder.exclude_dirs.is_empty()
        || !builder.exclude_files.is_empty()
        || !builder.exclude_extensions.is_empty()
        || !builder.exclude_lines.is_empty()
        || !builder.exclude_regexes.is_empty()
}

fn set_lines(builder: &mut CliBuilder, value: LineRange) -> Result<(), VFsSnifferError> {
    if builder.lines.is_some() {
        return Err(VFsSnifferError::new(
            "only one --lines option is allowed per run",
        ));
    }

    builder.lines = Some(value);
    Ok(())
}

fn set_root(builder: &mut CliBuilder, root: PathBuf) -> Result<(), VFsSnifferError> {
    if builder.root.is_some() {
        return Err(VFsSnifferError::new(
            "only one search root path is allowed per run",
        ));
    }

    builder.root = Some(root);
    Ok(())
}

fn parse_line_range(value: &str) -> Result<LineRange, VFsSnifferError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(VFsSnifferError::new(
            "invalid --lines range '', expected START:END",
        ));
    }

    let (start, end) = if let Some((start, end)) = value.split_once(':') {
        (
            parse_line_number(start.trim(), "start", value)?,
            parse_line_number(end.trim(), "end", value)?,
        )
    } else {
        let line = parse_line_number(value, "line", value)?;
        (line, line)
    };

    if start > end {
        return Err(VFsSnifferError::new(format!(
            "invalid --lines range '{value}': start line must be less than or equal to end line"
        )));
    }

    Ok(LineRange { start, end })
}

fn parse_line_number(value: &str, label: &str, range: &str) -> Result<usize, VFsSnifferError> {
    let line = value.parse::<usize>().map_err(|_| {
        VFsSnifferError::new(format!(
            "invalid --lines range '{range}': {label} line must be a positive number"
        ))
    })?;

    if line == 0 {
        return Err(VFsSnifferError::new(format!(
            "invalid --lines range '{range}': {label} line must be 1 or greater"
        )));
    }

    Ok(line)
}

fn parse_output_format(value: &str) -> Result<OutputFormat, VFsSnifferError> {
    match value.to_ascii_lowercase().as_str() {
        "text" | "txt" => Ok(OutputFormat::Text),
        "json" => Ok(OutputFormat::Json),
        _ => Err(VFsSnifferError::new(format!(
            "invalid output format '{value}', expected 'text' or 'json'"
        ))),
    }
}

pub(crate) fn usage() -> String {
    let usage = r#"v_fs_sniffer - deep filesystem sniffing for files, dirs, strings, regexes, and clues

USAGE:
  v_fs_sniffer
  v_fs_sniffer --file <path> --lines <start:end>
  v_fs_sniffer --file <name> <root> [options]
  v_fs_sniffer --file <name> <root> --replace-with <name> [options]
  v_fs_sniffer --dir <name> <root> [options]
  v_fs_sniffer --dir <name> <root> --replace-with <name> [options]
  v_fs_sniffer --str <text> <root> [options]
  v_fs_sniffer --str <text> <root> --replace-with <text> [options]
  v_fs_sniffer --regex <expr> <root> [options]
  v_fs_sniffer --check-update [--github-repo owner/repo]
  v_fs_sniffer --update [--github-repo owner/repo]
  v_fs_sniffer --uninstall

INTERACTIVE:
  Run without arguments to open the terminal app. Press Tab to autocomplete
  commands and filesystem paths. Paths with spaces are inserted quoted, such as
  "C:\Program Files". Type clear to clear the console and redraw the ASCII
  header. Type version to print the full installed app version. Type update to
  install the latest GitHub release for this OS.

SEARCH:
  --file <name>             Find files whose names contain <name>
  --file <path> --lines N:M Read inclusive line range N:M from one file
  --dir <name>              Find directories whose names contain <name>
  --str <text>              Find literal text inside files
  --regex, -rx <expr>       Find regex matches inside files; supports /pattern/imsgxU

OPTIONS:
  -nr, --no-recursive       Search only the root's direct children
  -cs, --case-sensitive     Use case-sensitive matching; default is case-insensitive
  --no-follow-symlinks      Do not follow symlinked files and directories
  --lines <start:end>       Read a 1-based inclusive line range; only with --file
  --replace-with <text>     Replace literal --file, --dir, or --str matches
  -o, --output <file>       Export findings; .json extension implies JSON
  --output-format <format>  text or json
  --json                    Print/export JSON
  -q, --quiet               Do not print findings to stdout
  --check-update            Check GitHub Releases for a newer version
  --update                  Download and run the latest matching GitHub release
  --github-repo <owner/repo> Override the embedded GitHub repository
  --uninstall               Remove the Cargo-installed binary; source files are untouched
  -V, --version             Print the full app version

EXCLUSIONS:
  -ex, --exclude-dir <path-or-name>      Exclude a directory subtree
  -ed, --exclude-dir <path-or-name>      Same as above
  -ef, --exclude-file <path-or-name>     Exclude matching files
  -ee, --exclude-extensions <exts>       Exclude comma/space-separated file extensions
  -el, --exclude-line <text>             Exclude content lines containing text
  -er, --exclude-regex <expr>            Exclude paths or content lines matching regex
"#;

    usage.to_owned()
}

pub(crate) fn version_text() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{parse, version_text, LineRange, ParsedArgs};

    #[test]
    fn help_is_not_a_parse_error() {
        assert!(matches!(
            parse(["v_fs_sniffer", "--help"]).unwrap(),
            ParsedArgs::Help(_)
        ));
    }

    #[test]
    fn version_is_not_a_parse_error() {
        assert!(matches!(
            parse(["v_fs_sniffer", "--version"]).unwrap(),
            ParsedArgs::Version(_)
        ));
    }

    #[test]
    fn version_text_is_only_the_package_version() {
        assert_eq!(version_text(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn uninstall_is_not_a_parse_error() {
        assert!(matches!(
            parse(["v_fs_sniffer", "--uninstall"]).unwrap(),
            ParsedArgs::Uninstall
        ));
    }

    #[test]
    fn check_update_is_not_a_parse_error() {
        assert!(matches!(
            parse(["v_fs_sniffer", "--check-update"]).unwrap(),
            ParsedArgs::CheckUpdate(_)
        ));
    }

    #[test]
    fn update_accepts_github_repo_override() {
        let ParsedArgs::Update(cli) =
            parse(["v_fs_sniffer", "--update", "--github-repo", "owner/repo"]).unwrap()
        else {
            panic!("expected update CLI args");
        };

        assert_eq!(cli.github_repo.as_deref(), Some("owner/repo"));
    }

    #[test]
    fn update_rejects_search_options() {
        let err = parse(["v_fs_sniffer", "--update", "--file", "Cargo.toml", "."]).unwrap_err();

        assert_eq!(
            err.to_string(),
            "--update and --check-update cannot be combined with search options"
        );
    }

    #[test]
    fn no_args_opens_interactive_shell() {
        assert!(matches!(
            parse(["v_fs_sniffer"]).unwrap(),
            ParsedArgs::Interactive
        ));
    }

    #[test]
    fn follows_symlinks_by_default() {
        let ParsedArgs::Run(cli) =
            parse(["v_fs_sniffer", "--str", "needle", "."]).expect("args should parse")
        else {
            panic!("expected runnable CLI args");
        };

        assert!(cli.follow_symlinks);
    }

    #[test]
    fn no_follow_symlinks_disables_symlink_following() {
        let ParsedArgs::Run(cli) = parse([
            "v_fs_sniffer",
            "--str",
            "needle",
            ".",
            "--no-follow-symlinks",
        ])
        .expect("args should parse") else {
            panic!("expected runnable CLI args");
        };

        assert!(!cli.follow_symlinks);
    }

    #[test]
    fn follow_symlinks_has_no_public_flag_because_it_is_the_default() {
        let err = parse(["v_fs_sniffer", "--str", "needle", ".", "--follow-symlinks"]).unwrap_err();

        assert_eq!(err.to_string(), "unknown option '--follow-symlinks'");
    }

    #[test]
    fn lines_with_file_reads_a_direct_file_path() {
        let ParsedArgs::ReadLines(cli) = parse([
            "v_fs_sniffer",
            "--file",
            "apps/web/src/lib/company/CompanyProfilePage.svelte",
            "--lines",
            "260:620",
        ])
        .expect("args should parse") else {
            panic!("expected line-read CLI args");
        };

        assert_eq!(
            cli.file,
            PathBuf::from("apps/web/src/lib/company/CompanyProfilePage.svelte")
        );
        assert_eq!(
            cli.lines,
            LineRange {
                start: 260,
                end: 620
            }
        );
    }

    #[test]
    fn lines_can_only_be_used_with_file() {
        let err = parse(["v_fs_sniffer", "--str", "needle", ".", "--lines", "1:2"]).unwrap_err();

        assert_eq!(err.to_string(), "--lines can only be used with --file");
    }

    #[test]
    fn lines_rejects_search_root_with_file() {
        let err = parse(["v_fs_sniffer", "--file", "needle", ".", "--lines", "1:2"]).unwrap_err();

        assert_eq!(
            err.to_string(),
            "--lines reads a direct --file path and does not take a search root"
        );
    }
}
