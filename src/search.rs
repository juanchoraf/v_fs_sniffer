use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::args::{Cli, SearchMode};
use crate::simple_regex::{MatchSpan, RegexOptions, SimpleRegex};
use crate::VFsSnifferError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    File,
    Dir,
    String,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    pub path: PathBuf,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub byte_offset: Option<usize>,
    pub matched: Option<String>,
    pub found: Option<String>,
    pub replaced_with: Option<String>,
    pub metadata: EntryMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMetadata {
    pub file_type: String,
    pub size_bytes: Option<u64>,
    pub readonly: bool,
    pub modified_unix_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchStats {
    pub scanned_dirs: usize,
    pub scanned_files: usize,
    pub skipped_entries: usize,
    pub unreadable_entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchWarning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchReport {
    pub root: PathBuf,
    pub mode: FindingKind,
    pub case_sensitive: bool,
    pub recursive: bool,
    pub findings: Vec<Finding>,
    pub stats: SearchStats,
    pub warnings: Vec<SearchWarning>,
}

pub trait ProgressReporter {
    fn reading(&mut self, path: &Path);
}

struct SearchContext<'a> {
    cli: &'a Cli,
    root: PathBuf,
    mode: CompiledMode,
    exclude_dirs: Vec<PathPattern>,
    exclude_files: Vec<PathPattern>,
    exclude_extensions: Vec<ExtensionPattern>,
    exclude_lines: Vec<LiteralMatcher>,
    exclude_regexes: Vec<SimpleRegex>,
    progress: &'a mut dyn ProgressReporter,
    report: SearchReport,
}

#[derive(Debug)]
enum CompiledMode {
    File(LiteralMatcher),
    Dir(LiteralMatcher),
    String(LiteralMatcher),
    Regex(SimpleRegex),
}

#[derive(Debug, Clone)]
struct LiteralMatcher {
    needle: String,
    folded_needle: String,
    needle_chars: Vec<char>,
    case_sensitive: bool,
}

#[derive(Debug, Clone)]
struct PathPattern {
    normalized: String,
    case_sensitive: bool,
    contains_separator: bool,
}

#[derive(Debug, Clone)]
struct ExtensionPattern {
    normalized: String,
    case_sensitive: bool,
}

pub fn search_with_progress(
    cli: &Cli,
    progress: &mut dyn ProgressReporter,
) -> Result<SearchReport, VFsSnifferError> {
    let root = absolute_existing_path(&cli.root)?;
    let kind = finding_kind(&cli.mode);
    let mode = compile_mode(&cli.mode, cli.case_sensitive)?;

    let mut ctx = SearchContext {
        cli,
        root: root.clone(),
        mode,
        exclude_dirs: cli
            .exclude_dirs
            .iter()
            .map(|pattern| PathPattern::new(pattern, cli.case_sensitive))
            .collect(),
        exclude_files: cli
            .exclude_files
            .iter()
            .map(|pattern| PathPattern::new(pattern, cli.case_sensitive))
            .collect(),
        exclude_extensions: cli
            .exclude_extensions
            .iter()
            .map(|extension| ExtensionPattern::new(extension, cli.case_sensitive))
            .collect(),
        exclude_lines: cli
            .exclude_lines
            .iter()
            .map(|pattern| LiteralMatcher::new(pattern, cli.case_sensitive))
            .collect(),
        exclude_regexes: cli
            .exclude_regexes
            .iter()
            .map(|expr| compile_user_regex(expr, !cli.case_sensitive))
            .collect::<Result<Vec<_>, _>>()?,
        progress,
        report: SearchReport {
            root: root.clone(),
            mode: kind,
            case_sensitive: cli.case_sensitive,
            recursive: cli.recursive,
            findings: Vec::new(),
            stats: SearchStats::default(),
            warnings: Vec::new(),
        },
    };

    let metadata = metadata_for(&root, cli.follow_symlinks).map_err(|err| {
        VFsSnifferError::new(format!(
            "failed to read metadata for '{}': {err}",
            root.display()
        ))
    })?;

    if metadata.is_dir() {
        ctx.visit_dir(&root, 0);
    } else if metadata.is_file() {
        if ctx.file_excluded(&root) {
            ctx.report.stats.skipped_entries += 1;
        } else {
            ctx.process_file(&root, &metadata);
        }
    } else {
        return Err(VFsSnifferError::new(format!(
            "'{}' is not a file or directory",
            root.display()
        )));
    }

    Ok(ctx.report)
}

impl SearchContext<'_> {
    fn visit_dir(&mut self, dir: &Path, depth: usize) {
        self.progress.reading(dir);
        self.report.stats.scanned_dirs += 1;

        if depth > 0 && self.dir_excluded(dir) {
            self.report.stats.skipped_entries += 1;
            return;
        }

        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                self.warn(dir, err);
                return;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    self.report.stats.unreadable_entries += 1;
                    self.report.warnings.push(SearchWarning {
                        path: dir.to_path_buf(),
                        message: err.to_string(),
                    });
                    continue;
                }
            };

            let path = entry.path();
            let metadata = match metadata_for(&path, self.cli.follow_symlinks) {
                Ok(metadata) => metadata,
                Err(err) => {
                    self.warn(&path, err);
                    continue;
                }
            };

            if metadata.is_dir() {
                if self.dir_excluded(&path) {
                    self.report.stats.skipped_entries += 1;
                    continue;
                }

                let path = self.process_dir_match(&path, &metadata);

                if self.cli.recursive {
                    self.visit_dir(&path, depth + 1);
                }
            } else if metadata.is_file() {
                if self.file_excluded(&path) {
                    self.report.stats.skipped_entries += 1;
                    continue;
                }

                self.process_file(&path, &metadata);
            }
        }
    }

    fn process_dir_match(&mut self, path: &Path, metadata: &fs::Metadata) -> PathBuf {
        if let CompiledMode::Dir(matcher) = &self.mode {
            let matcher = matcher.clone();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");

            if let Some(replacement) = self.cli.replace_with.clone() {
                return self.replace_entry_name(
                    path,
                    metadata,
                    FindingKind::Dir,
                    "dir",
                    &matcher,
                    &replacement,
                );
            }

            if matcher.contains(name).is_some() {
                self.push_entry_finding(path, metadata, FindingKind::Dir, "dir", name, None);
            }
        }

        path.to_path_buf()
    }

    fn process_file(&mut self, path: &Path, metadata: &fs::Metadata) {
        self.progress.reading(path);
        self.report.stats.scanned_files += 1;

        match &self.mode {
            CompiledMode::File(matcher) => {
                let matcher = matcher.clone();
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");

                if let Some(replacement) = self.cli.replace_with.clone() {
                    self.replace_entry_name(
                        path,
                        metadata,
                        FindingKind::File,
                        "file",
                        &matcher,
                        &replacement,
                    );
                    return;
                }

                if matcher.contains(name).is_some() {
                    self.push_entry_finding(path, metadata, FindingKind::File, "file", name, None);
                }
            }
            CompiledMode::String(matcher) => {
                let matcher = matcher.clone();
                if let Some(replacement) = self.cli.replace_with.clone() {
                    self.replace_file_content(path, metadata, matcher, &replacement);
                } else {
                    self.scan_file_content(
                        path,
                        metadata,
                        ContentMatcher::Literal(matcher),
                        FindingKind::String,
                    );
                }
            }
            CompiledMode::Regex(regex) => {
                self.scan_file_content(
                    path,
                    metadata,
                    ContentMatcher::Regex(regex.clone()),
                    FindingKind::Regex,
                );
            }
            CompiledMode::Dir(_) => {}
        }
    }

    fn replace_entry_name(
        &mut self,
        path: &Path,
        metadata: &fs::Metadata,
        kind: FindingKind,
        file_type: &str,
        matcher: &LiteralMatcher,
        replacement: &str,
    ) -> PathBuf {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return path.to_path_buf();
        };
        let Some(replaced_name) = replace_literal_matches(name, matcher, replacement) else {
            return path.to_path_buf();
        };

        if let Some(reason) = invalid_entry_name_reason(&replaced_name) {
            self.warn(path, std::io::Error::new(ErrorKind::InvalidInput, reason));
            return path.to_path_buf();
        }

        let replaced_path = path.with_file_name(&replaced_name);
        let mut report_path = path.to_path_buf();
        let mut report_metadata = EntryMetadata::from_metadata(metadata, file_type);

        if replaced_path != path {
            match destination_exists(&replaced_path) {
                Ok(true) => {
                    self.warn(
                        &replaced_path,
                        std::io::Error::new(
                            ErrorKind::AlreadyExists,
                            "replacement destination already exists",
                        ),
                    );
                    return path.to_path_buf();
                }
                Ok(false) => {}
                Err(err) => {
                    self.warn(&replaced_path, err);
                    return path.to_path_buf();
                }
            }

            if let Err(err) = fs::rename(path, &replaced_path) {
                self.warn(path, err);
                return path.to_path_buf();
            }

            report_path = replaced_path.clone();
            if let Ok(updated_metadata) = metadata_for(&replaced_path, self.cli.follow_symlinks) {
                report_metadata = EntryMetadata::from_metadata(&updated_metadata, file_type);
            }
        }

        self.report.findings.push(Finding {
            kind,
            path: report_path,
            line: None,
            column: None,
            byte_offset: None,
            matched: Some(name.to_owned()),
            found: Some(name.to_owned()),
            replaced_with: Some(replaced_name),
            metadata: report_metadata,
        });

        replaced_path
    }

    fn push_entry_finding(
        &mut self,
        path: &Path,
        metadata: &fs::Metadata,
        kind: FindingKind,
        file_type: &str,
        name: &str,
        replaced_with: Option<String>,
    ) {
        self.report.findings.push(Finding {
            kind,
            path: path.to_path_buf(),
            line: None,
            column: None,
            byte_offset: None,
            matched: Some(name.to_owned()),
            found: Some(name.to_owned()),
            replaced_with,
            metadata: EntryMetadata::from_metadata(metadata, file_type),
        });
    }

    fn scan_file_content(
        &mut self,
        path: &Path,
        metadata: &fs::Metadata,
        matcher: ContentMatcher,
        kind: FindingKind,
    ) {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(err) => {
                self.warn(path, err);
                return;
            }
        };

        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        let mut line_number = 0usize;
        let mut bytes_before_line = 0usize;

        loop {
            line.clear();
            let bytes_read = match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(bytes_read) => bytes_read,
                Err(err) => {
                    self.warn(path, err);
                    break;
                }
            };

            line_number += 1;
            let text = String::from_utf8_lossy(&line);

            if self.line_excluded(&text) {
                bytes_before_line += bytes_read;
                continue;
            }

            for matched in matcher.find_iter(&text) {
                let start = matched.start;
                let column = text[..start].chars().count() + 1;
                self.report.findings.push(Finding {
                    kind,
                    path: path.to_path_buf(),
                    line: Some(line_number),
                    column: Some(column),
                    byte_offset: Some(bytes_before_line + start),
                    matched: Some(text[matched.start..matched.end].to_owned()),
                    found: Some(context_around_match(&text, matched)),
                    replaced_with: None,
                    metadata: EntryMetadata::from_metadata(metadata, "file"),
                });
            }

            bytes_before_line += bytes_read;
        }
    }

    fn replace_file_content(
        &mut self,
        path: &Path,
        metadata: &fs::Metadata,
        matcher: LiteralMatcher,
        replacement: &str,
    ) {
        let original = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                self.warn(path, err);
                return;
            }
        };

        let mut updated = String::with_capacity(original.len());
        let mut findings = Vec::new();
        let mut line_number = 0usize;
        let mut bytes_before_line = 0usize;

        for line in original.split_inclusive('\n') {
            line_number += 1;

            if self.line_excluded(line) {
                updated.push_str(line);
                bytes_before_line += line.len();
                continue;
            }

            let matches = matcher.find_iter(line);
            if matches.is_empty() {
                updated.push_str(line);
                bytes_before_line += line.len();
                continue;
            }

            let mut updated_line = String::with_capacity(line.len());
            let mut last = 0usize;
            let mut replacement_spans = Vec::with_capacity(matches.len());

            for matched in &matches {
                updated_line.push_str(&line[last..matched.start]);
                let replacement_start = updated_line.len();
                updated_line.push_str(replacement);
                let replacement_end = updated_line.len();
                replacement_spans.push(MatchSpan {
                    start: replacement_start,
                    end: replacement_end,
                });
                last = matched.end;
            }
            updated_line.push_str(&line[last..]);

            for (matched, replaced) in matches.into_iter().zip(replacement_spans) {
                let column = line[..matched.start].chars().count() + 1;
                findings.push(Finding {
                    kind: FindingKind::String,
                    path: path.to_path_buf(),
                    line: Some(line_number),
                    column: Some(column),
                    byte_offset: Some(bytes_before_line + matched.start),
                    matched: Some(line[matched.start..matched.end].to_owned()),
                    found: Some(context_around_match(line, matched)),
                    replaced_with: Some(context_around_match(&updated_line, replaced)),
                    metadata: EntryMetadata::from_metadata(metadata, "file"),
                });
            }

            bytes_before_line += line.len();
            updated.push_str(&updated_line);
        }

        if findings.is_empty() {
            return;
        }

        if updated != original {
            if let Err(err) =
                write_replacement_file(path, updated.as_bytes(), metadata, self.cli.follow_symlinks)
            {
                self.warn(path, err);
                return;
            }

            if let Ok(updated_metadata) = metadata_for(path, self.cli.follow_symlinks) {
                let entry_metadata = EntryMetadata::from_metadata(&updated_metadata, "file");
                for finding in &mut findings {
                    finding.metadata = entry_metadata.clone();
                }
            }
        }

        self.report.findings.extend(findings);
    }

    fn dir_excluded(&self, path: &Path) -> bool {
        self.exclude_dirs
            .iter()
            .any(|pattern| pattern.matches_path(path, &self.root))
            || self.path_excluded_by_regex(path)
    }

    fn file_excluded(&self, path: &Path) -> bool {
        self.exclude_files
            .iter()
            .any(|pattern| pattern.matches_path(path, &self.root))
            || self
                .exclude_extensions
                .iter()
                .any(|pattern| pattern.matches_path(path))
            || self.path_excluded_by_regex(path)
    }

    fn path_excluded_by_regex(&self, path: &Path) -> bool {
        let normalized = normalize_path(path);
        self.exclude_regexes
            .iter()
            .any(|regex| regex.is_match(&normalized))
    }

    fn line_excluded(&self, line: &str) -> bool {
        self.exclude_lines
            .iter()
            .any(|matcher| matcher.contains(line).is_some())
            || self
                .exclude_regexes
                .iter()
                .any(|regex| regex.is_match(line))
    }

    fn warn(&mut self, path: &Path, err: std::io::Error) {
        self.report.stats.unreadable_entries += 1;
        self.report.warnings.push(SearchWarning {
            path: path.to_path_buf(),
            message: err.to_string(),
        });
    }
}

impl LiteralMatcher {
    fn new(needle: &str, case_sensitive: bool) -> Self {
        Self {
            needle: needle.to_owned(),
            folded_needle: needle.to_lowercase(),
            needle_chars: needle.chars().collect(),
            case_sensitive,
        }
    }

    fn contains(&self, haystack: &str) -> Option<usize> {
        if self.case_sensitive {
            return haystack.find(&self.needle);
        }

        haystack.to_lowercase().find(&self.folded_needle)
    }

    fn find_iter(&self, haystack: &str) -> Vec<MatchSpan> {
        if self.needle_chars.is_empty() {
            return Vec::new();
        }

        let chars: Vec<(usize, char)> = haystack.char_indices().collect();
        let mut spans = Vec::new();
        let mut start = 0usize;

        while start + self.needle_chars.len() <= chars.len() {
            let matches = self
                .needle_chars
                .iter()
                .enumerate()
                .all(|(offset, expected)| {
                    chars_equal(*expected, chars[start + offset].1, self.case_sensitive)
                });

            if matches {
                let end_char = start + self.needle_chars.len();
                let end = if end_char == chars.len() {
                    haystack.len()
                } else {
                    chars[end_char].0
                };
                spans.push(MatchSpan {
                    start: chars[start].0,
                    end,
                });
                start = end_char;
            } else {
                start += 1;
            }
        }

        spans
    }
}

impl PathPattern {
    fn new(pattern: &str, case_sensitive: bool) -> Self {
        let normalized = normalize_raw_path(pattern);
        let normalized = if case_sensitive {
            normalized
        } else {
            normalized.to_lowercase()
        };

        Self {
            contains_separator: pattern.contains('/') || pattern.contains('\\'),
            normalized,
            case_sensitive,
        }
    }

    fn matches_path(&self, path: &Path, root: &Path) -> bool {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(normalize_raw_path)
            .unwrap_or_default();
        let full = normalize_path(path);
        let relative = path
            .strip_prefix(root)
            .ok()
            .map(normalize_path)
            .unwrap_or_default();

        let (name, full, relative) = if self.case_sensitive {
            (name, full, relative)
        } else {
            (
                name.to_lowercase(),
                full.to_lowercase(),
                relative.to_lowercase(),
            )
        };

        if self.contains_separator {
            return path_pattern_matches(&relative, &self.normalized)
                || path_pattern_matches(&full, &self.normalized);
        }

        name == self.normalized || full.ends_with(&format!("/{}", self.normalized))
    }
}

impl ExtensionPattern {
    fn new(extension: &str, case_sensitive: bool) -> Self {
        let normalized = extension.trim().trim_start_matches('.').to_owned();
        let normalized = if case_sensitive {
            normalized
        } else {
            normalized.to_lowercase()
        };

        Self {
            normalized,
            case_sensitive,
        }
    }

    fn matches_path(&self, path: &Path) -> bool {
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return false;
        };

        if self.case_sensitive {
            extension == self.normalized
        } else {
            extension.to_lowercase() == self.normalized
        }
    }
}

impl EntryMetadata {
    fn from_metadata(metadata: &fs::Metadata, file_type: &str) -> Self {
        Self {
            file_type: file_type.to_owned(),
            size_bytes: metadata.is_file().then_some(metadata.len()),
            readonly: metadata.permissions().readonly(),
            modified_unix_secs: metadata.modified().ok().and_then(system_time_to_unix),
        }
    }
}

fn compile_mode(mode: &SearchMode, case_sensitive: bool) -> Result<CompiledMode, VFsSnifferError> {
    match mode {
        SearchMode::File(needle) => {
            ensure_non_empty(needle, "--file")?;
            Ok(CompiledMode::File(LiteralMatcher::new(
                needle,
                case_sensitive,
            )))
        }
        SearchMode::Dir(needle) => {
            ensure_non_empty(needle, "--dir")?;
            Ok(CompiledMode::Dir(LiteralMatcher::new(
                needle,
                case_sensitive,
            )))
        }
        SearchMode::Str(needle) => {
            ensure_non_empty(needle, "--str")?;
            Ok(CompiledMode::String(LiteralMatcher::new(
                needle,
                case_sensitive,
            )))
        }
        SearchMode::Regex(expr) => Ok(CompiledMode::Regex(compile_user_regex(
            expr,
            !case_sensitive,
        )?)),
    }
}

fn finding_kind(mode: &SearchMode) -> FindingKind {
    match mode {
        SearchMode::File(_) => FindingKind::File,
        SearchMode::Dir(_) => FindingKind::Dir,
        SearchMode::Str(_) => FindingKind::String,
        SearchMode::Regex(_) => FindingKind::Regex,
    }
}

#[derive(Debug, Clone)]
enum ContentMatcher {
    Literal(LiteralMatcher),
    Regex(SimpleRegex),
}

impl ContentMatcher {
    fn find_iter(&self, haystack: &str) -> Vec<MatchSpan> {
        match self {
            ContentMatcher::Literal(matcher) => matcher.find_iter(haystack),
            ContentMatcher::Regex(regex) => regex.find_iter(haystack),
        }
    }
}

fn compile_user_regex(
    expr: &str,
    default_case_insensitive: bool,
) -> Result<SimpleRegex, VFsSnifferError> {
    let parsed = parse_delimited_regex(expr)?;
    let mut options = RegexOptions {
        case_sensitive: !default_case_insensitive,
        ..RegexOptions::default()
    };

    for flag in parsed.flags.chars() {
        match flag {
            'g' => {}
            'i' => {
                options.case_sensitive = false;
            }
            'm' => {
                options.multi_line = true;
            }
            's' => {
                options.dot_matches_new_line = true;
            }
            'x' => {
                options.ignore_whitespace = true;
            }
            'U' => {
                // Accepted for compatibility with /pattern/U. Matching is greedy.
            }
            _ => {
                return Err(VFsSnifferError::new(format!(
                    "unsupported regex flag '{flag}' in '{}'",
                    expr
                )));
            }
        }
    }

    Ok(SimpleRegex::compile(&parsed.pattern, options)?)
}

#[derive(Debug, Clone)]
struct ParsedRegex {
    pattern: String,
    flags: String,
}

fn parse_delimited_regex(expr: &str) -> Result<ParsedRegex, VFsSnifferError> {
    if !expr.starts_with('/') {
        return Ok(ParsedRegex {
            pattern: expr.to_owned(),
            flags: String::new(),
        });
    }

    let Some(end) = last_unescaped_slash(expr) else {
        return Err(VFsSnifferError::new(format!(
            "invalid delimited regex '{expr}': missing closing '/'"
        )));
    };

    if end == 0 {
        return Ok(ParsedRegex {
            pattern: expr.to_owned(),
            flags: String::new(),
        });
    }

    Ok(ParsedRegex {
        pattern: unescape_regex_delimiter(&expr[1..end]),
        flags: expr[end + 1..].to_owned(),
    })
}

fn unescape_regex_delimiter(pattern: &str) -> String {
    pattern.replace(r"\/", "/")
}

fn ensure_non_empty(value: &str, flag: &str) -> Result<(), VFsSnifferError> {
    if value.is_empty() {
        return Err(VFsSnifferError::new(format!(
            "{flag} requires a non-empty search value"
        )));
    }

    Ok(())
}

fn chars_equal(left: char, right: char, case_sensitive: bool) -> bool {
    left == right
        || (!case_sensitive
            && left
                .to_lowercase()
                .collect::<String>()
                .eq(&right.to_lowercase().collect::<String>()))
}

fn context_around_match(line: &str, matched: MatchSpan) -> String {
    const CONTEXT_CHARS: usize = 15;

    let display_line = line.trim_end_matches(['\r', '\n']);
    let line = if matched.end <= display_line.len() {
        display_line
    } else {
        line
    };

    let offsets = char_offsets(line);
    let start_char = char_index_for_byte(&offsets, matched.start);
    let end_char = char_index_for_byte(&offsets, matched.end);
    let context_start = start_char.saturating_sub(CONTEXT_CHARS);
    let context_end = (end_char + CONTEXT_CHARS).min(offsets.len().saturating_sub(1));
    let start_byte = offsets[context_start];
    let end_byte = offsets[context_end];

    line[start_byte..end_byte].to_owned()
}

fn replace_literal_matches(
    value: &str,
    matcher: &LiteralMatcher,
    replacement: &str,
) -> Option<String> {
    let matches = matcher.find_iter(value);
    if matches.is_empty() {
        return None;
    }

    let mut replaced = String::with_capacity(value.len());
    let mut last = 0usize;

    for matched in matches {
        replaced.push_str(&value[last..matched.start]);
        replaced.push_str(replacement);
        last = matched.end;
    }
    replaced.push_str(&value[last..]);

    Some(replaced)
}

fn invalid_entry_name_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("replacement would create an empty file or directory name");
    }
    if name == "." || name == ".." {
        return Some("replacement would create a reserved file or directory name");
    }
    if name.contains('/') || name.contains('\\') {
        return Some("replacement file or directory name cannot contain path separators");
    }
    if name.contains('\0') {
        return Some("replacement file or directory name cannot contain a null byte");
    }

    None
}

fn destination_exists(path: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

fn write_replacement_file(
    path: &Path,
    content: &[u8],
    metadata: &fs::Metadata,
    follow_symlinks: bool,
) -> std::io::Result<()> {
    let target = writable_target(path, follow_symlinks)?;
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("v_fs_sniffer");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut last_exists = None;

    for attempt in 0..100 {
        let temp = parent.join(format!(
            ".{file_name}.v_fs_sniffer_tmp_{}_{}_{}",
            std::process::id(),
            unique,
            attempt
        ));

        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(mut file) => {
                let write_result = (|| {
                    file.write_all(content)?;
                    file.sync_all()?;
                    drop(file);
                    fs::set_permissions(&temp, metadata.permissions())?;
                    replace_existing_file(&temp, &target)
                })();

                if write_result.is_err() {
                    let _ = fs::remove_file(&temp);
                }

                return write_result;
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                last_exists = Some(err);
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_exists.unwrap_or_else(|| {
        std::io::Error::new(
            ErrorKind::AlreadyExists,
            "failed to create a unique temporary replacement file",
        )
    }))
}

fn writable_target(path: &Path, follow_symlinks: bool) -> std::io::Result<PathBuf> {
    if follow_symlinks && fs::symlink_metadata(path)?.file_type().is_symlink() {
        fs::canonicalize(path)
    } else {
        Ok(path.to_path_buf())
    }
}

#[cfg(not(windows))]
fn replace_existing_file(temp: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(temp, target)
}

#[cfg(windows)]
fn replace_existing_file(temp: &Path, target: &Path) -> std::io::Result<()> {
    fs::remove_file(target)?;
    fs::rename(temp, target)
}

fn char_offsets(value: &str) -> Vec<usize> {
    let mut offsets = value
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    offsets.push(value.len());
    offsets
}

fn char_index_for_byte(offsets: &[usize], byte: usize) -> usize {
    offsets
        .binary_search(&byte)
        .unwrap_or_else(|index| index.saturating_sub(1))
}

fn last_unescaped_slash(expr: &str) -> Option<usize> {
    let bytes = expr.as_bytes();
    for index in (1..bytes.len()).rev() {
        if bytes[index] != b'/' {
            continue;
        }

        let mut slash_count = 0usize;
        let mut cursor = index;
        while cursor > 0 && bytes[cursor - 1] == b'\\' {
            slash_count += 1;
            cursor -= 1;
        }

        if slash_count % 2 == 0 {
            return Some(index);
        }
    }

    None
}

fn absolute_existing_path(path: &Path) -> Result<PathBuf, VFsSnifferError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };

    fs::canonicalize(&absolute).map_err(|err| {
        VFsSnifferError::new(format!(
            "failed to resolve search root '{}': {err}",
            path.display()
        ))
    })
}

fn metadata_for(path: &Path, follow_symlinks: bool) -> std::io::Result<fs::Metadata> {
    if follow_symlinks {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
}

fn normalize_path(path: impl AsRef<Path>) -> String {
    normalize_raw_path(&path.as_ref().to_string_lossy())
}

fn normalize_raw_path(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_owned()
}

fn path_pattern_matches(candidate: &str, pattern: &str) -> bool {
    candidate == pattern
        || candidate.starts_with(&format!("{pattern}/"))
        || candidate.ends_with(&format!("/{pattern}"))
        || candidate.contains(&format!("/{pattern}/"))
}

fn system_time_to_unix(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}
