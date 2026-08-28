use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn finds_strings_recursively_and_case_insensitively() {
    let fixture = Fixture::new("recursive_case_insensitive");
    fixture.write("app/service.conf", "first\nString Found here\nlast\n");

    let output = run(["--str", "string found", fixture.root.to_str().unwrap()]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("| Kind   | Path"));
    assert_table_columns(
        &stdout,
        &[
            "Kind",
            "Path",
            "Line",
            "Col",
            "Found",
            "Type",
            "ByteOffset",
            "SizeBytes",
            "Readonly",
            "ModifiedUnix",
        ],
    );
    assert!(stdout.contains("| string |"));
    assert!(stdout.contains("service.conf"));
    assert!(stdout.contains("|    2 |   1 | String Found"));
    assert!(!stdout.contains("Match"));
    assert!(!stdout.contains("string found on that file"));
    assert!(stdout.contains("\n\nSummary: 1 matches"));
    assert!(stdout.contains("Summary: 1 matches"));
}

#[test]
fn found_column_shows_fifteen_chars_around_content_match() {
    let fixture = Fixture::new("found_context");
    fixture.write("context.txt", "abcdefghijklmnopNEEDLEqrstuvwxyzabcdef\n");

    let output = run(["--str", "NEEDLE", fixture.root.to_str().unwrap()]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("bcdefghijklmnopNEEDLEqrstuvwxyzabcde"));
    assert!(!stdout.contains("abcdefghijklmnopNEEDLE"));
    assert!(!stdout.contains("NEEDLEqrstuvwxyzabcdef"));
}

#[test]
fn progress_is_single_stderr_line_and_does_not_pollute_stdout() {
    let fixture = Fixture::new("progress");
    fixture.write("service.conf", "needle\n");

    let output = run(["--str", "needle", fixture.root.to_str().unwrap()]);

    assert_success(&output);
    let stdout = stdout(&output);
    let stderr = stderr(&output);
    assert!(stdout.starts_with('\n'));
    assert!(stdout.ends_with("\n\n"));
    assert!(stdout.contains("Summary: 1 matches"));
    assert!(!stdout.contains("reading"));
    assert!(stderr.starts_with('\n'));
    assert!(stderr.contains("reading"));
    assert_eq!(stderr.matches('\n').count(), 1);
    assert!(!stdout.contains("\x1b["));
    assert!(!stderr.contains("\x1b["));
}

#[test]
fn case_sensitive_flag_changes_string_matching() {
    let fixture = Fixture::new("case_sensitive");
    fixture.write("service.conf", "Password=secret\n");

    let output = run(["--str", "password", fixture.root.to_str().unwrap(), "-cs"]);

    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 0 matches"));
}

#[test]
fn finds_strings_across_multiple_roots() {
    let first = Fixture::new("multi_root_first");
    let second = Fixture::new("multi_root_second");
    first.write("web.txt", "needle from web\n");
    second.write("api.txt", "needle from api\n");
    let first_root = first.root.to_str().unwrap().to_owned();
    let second_root = second.root.to_str().unwrap().to_owned();

    let output = run(["--str", "needle", first_root.as_str(), second_root.as_str()]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("web.txt"));
    assert!(stdout.contains("api.txt"));
    assert!(stdout.contains("Summary: 2 matches"));
}

#[test]
fn json_output_lists_multiple_roots() {
    let first = Fixture::new("multi_root_json_first");
    let second = Fixture::new("multi_root_json_second");
    first.write("web.txt", "needle from web\n");
    second.write("api.txt", "needle from api\n");
    let first_root = first.root.to_str().unwrap().to_owned();
    let second_root = second.root.to_str().unwrap().to_owned();

    let output = run([
        "--str",
        "needle",
        first_root.as_str(),
        second_root.as_str(),
        "--json",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("\"root\":"));
    assert!(stdout.contains("\"roots\": ["));
    assert!(stdout.contains(&first_root));
    assert!(stdout.contains(&second_root));
    assert!(stdout.contains("\"findings\""));
}

#[test]
fn no_recursive_still_checks_direct_children() {
    let fixture = Fixture::new("no_recursive");
    fixture.write("top.txt", "needle\n");
    fixture.write("nested/deep.txt", "needle\n");

    let output = run(["--str", "needle", fixture.root.to_str().unwrap(), "-nr"]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("top.txt"));
    assert!(!stdout.contains("deep.txt"));
    assert!(stdout.contains("Summary: 1 matches"));
}

#[test]
fn exclusions_apply_to_each_root() {
    let first = Fixture::new("multi_root_exclusions_first");
    let second = Fixture::new("multi_root_exclusions_second");
    first.write("keep.txt", "needle visible\n");
    first.write("skip/drop.txt", "needle hidden\n");
    second.write("keep.txt", "needle visible\n");
    second.write("skip/drop.txt", "needle hidden\n");
    let first_root = first.root.to_str().unwrap().to_owned();
    let second_root = second.root.to_str().unwrap().to_owned();

    let output = run([
        "--str",
        "needle",
        first_root.as_str(),
        second_root.as_str(),
        "--exclude-dir",
        "skip",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("keep.txt"));
    assert!(!stdout.contains("drop.txt"));
    assert!(stdout.contains("Summary: 2 matches"));
}

#[test]
fn excludes_directories_files_lines_and_regexes() {
    let fixture = Fixture::new("exclusions");
    fixture.write("include/keep.txt", "needle one\nneedle two\n");
    fixture.write("skip_dir/drop.txt", "needle hidden\n");
    fixture.write("skip_file.txt", "needle hidden\n");
    fixture.write("include/regex_skip.txt", "needle hidden\n");

    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        "-ex=skip_dir",
        "-ef=skip_file.txt",
        "-el=two",
        "-er=regex_skip",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("keep.txt"));
    assert!(!stdout.contains("drop.txt"));
    assert!(!stdout.contains("skip_file.txt"));
    assert!(!stdout.contains("regex_skip.txt"));
    assert!(stdout.contains("Summary: 1 matches"));
}

#[test]
fn excludes_multiple_extensions_from_one_flag() {
    let fixture = Fixture::new("exclude_extensions");
    fixture.write("keep.txt", "needle visible\n");
    fixture.write("debug.log", "needle hidden\n");
    fixture.write("cache.tmp", "needle hidden\n");
    fixture.write("notes.MD", "needle hidden\n");

    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        "-ee=.log,tmp,md",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("keep.txt"));
    assert!(!stdout.contains("debug.log"));
    assert!(!stdout.contains("cache.tmp"));
    assert!(!stdout.contains("notes.MD"));
    assert!(stdout.contains("Summary: 1 matches"));
}

#[test]
fn delimited_regex_flags_are_supported() {
    let fixture = Fixture::new("regex_flags");
    fixture.write("images.txt", "A.png\n7.png\n");

    let output = run([
        "--regex",
        r"/([^0-9]\.png)+/gim",
        fixture.root.to_str().unwrap(),
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("| regex |"));
    assert!(stdout.contains("| A.png"));
    assert!(stdout.contains("Summary: 1 matches"));
}

#[test]
fn finds_files_and_directories_by_name_case_insensitively() {
    let fixture = Fixture::new("names");
    fixture.write("Photos/PHOTO.PNG", "not image data\n");

    let file_output = run(["--file", "photo.png", fixture.root.to_str().unwrap()]);
    assert_success(&file_output);
    let file_stdout = stdout(&file_output);
    assert!(file_stdout.contains("| file |"));
    assert!(file_stdout.contains("PHOTO.PNG"));

    let dir_output = run(["--dir", "photos", fixture.root.to_str().unwrap()]);
    assert_success(&dir_output);
    let dir_stdout = stdout(&dir_output);
    assert!(dir_stdout.contains("| dir  |"));
    assert!(dir_stdout.contains("Photos"));
    assert!(dir_stdout.contains("Summary: 1 matches"));
}

#[test]
fn reads_direct_file_line_range_with_file_and_lines() {
    let fixture = Fixture::new("file_lines");
    let file = fixture.root.join("CompanyProfilePage.svelte");
    fixture.write(
        "CompanyProfilePage.svelte",
        "one\n<script>\n  const company = 'Velasquez';\n</script>\nfive\n",
    );

    let output = run(["--file", file.to_str().unwrap(), "--lines", "2:4"]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("CompanyProfilePage.svelte"));
    assert!(stdout.contains("2 | <script>"));
    assert!(stdout.contains("3 |   const company = 'Velasquez';"));
    assert!(stdout.contains("4 | </script>"));
    assert!(!stdout.contains("1 | one"));
    assert!(!stdout.contains("5 | five"));
}

#[test]
fn lines_option_is_rejected_outside_file_mode() {
    let fixture = Fixture::new("lines_wrong_mode");
    fixture.write("service.conf", "needle\n");

    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        "--lines",
        "1:1",
    ]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("--lines can only be used with --file"));
}

#[test]
fn exports_json_findings_to_file() {
    let fixture = Fixture::new("export");
    fixture.write("service.conf", "needle\n");
    let export_path = fixture.root.join("findings.json");

    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        "-o",
        export_path.to_str().unwrap(),
    ]);

    assert_success(&output);
    let exported = fs::read_to_string(export_path).unwrap();
    assert!(!exported.starts_with('\n'));
    assert!(!exported.ends_with("\n\n"));
    assert!(exported.contains("\"findings\""));
    assert!(exported.contains("\"matched\": \"needle\""));
}

#[test]
fn replace_with_updates_string_matches_in_multiple_files() {
    let fixture = Fixture::new("replace_multiple_files");
    fixture.write("one.txt", "before needle after\n");
    fixture.write("nested/two.txt", "needle and needle\n");

    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        "--replace-with",
        "thread",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert_table_columns(
        &stdout,
        &[
            "Kind",
            "Path",
            "Line",
            "Col",
            "Found",
            "ReplacedWith",
            "Type",
            "ByteOffset",
            "SizeBytes",
            "Readonly",
            "ModifiedUnix",
        ],
    );
    assert!(stdout.contains("before needle after"));
    assert!(stdout.contains("before thread after"));
    assert!(stdout.contains("needle and needle"));
    assert!(stdout.contains("thread and thread"));
    assert!(stdout.contains("Summary: 3 matches"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("one.txt")).unwrap(),
        "before thread after\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("nested/two.txt")).unwrap(),
        "thread and thread\n"
    );
}

#[test]
fn replace_with_updates_string_matches_across_multiple_roots() {
    let first = Fixture::new("multi_root_replace_first");
    let second = Fixture::new("multi_root_replace_second");
    first.write("one.txt", "before needle after\n");
    second.write("two.txt", "needle here\n");
    let first_root = first.root.to_str().unwrap().to_owned();
    let second_root = second.root.to_str().unwrap().to_owned();

    let output = run([
        "--str",
        "needle",
        first_root.as_str(),
        second_root.as_str(),
        "--replace-with",
        "thread",
    ]);

    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 2 matches"));
    assert_eq!(
        fs::read_to_string(first.root.join("one.txt")).unwrap(),
        "before thread after\n"
    );
    assert_eq!(
        fs::read_to_string(second.root.join("two.txt")).unwrap(),
        "thread here\n"
    );
}

#[test]
fn replace_with_renames_matching_files() {
    let fixture = Fixture::new("replace_files");
    fixture.write("old-report.txt", "one\n");
    fixture.write("nested/OLD-report.txt", "two\n");

    let output = run([
        "--file",
        "old",
        fixture.root.to_str().unwrap(),
        "--replace-with",
        "new",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert_table_columns(
        &stdout,
        &[
            "Kind",
            "Path",
            "Line",
            "Col",
            "Found",
            "ReplacedWith",
            "Type",
            "ByteOffset",
            "SizeBytes",
            "Readonly",
            "ModifiedUnix",
        ],
    );
    assert!(stdout.contains("old-report.txt"));
    assert!(stdout.contains("new-report.txt"));
    assert!(stdout.contains("OLD-report.txt"));
    assert!(stdout.contains("Summary: 2 matches"));
    assert!(fixture.root.join("new-report.txt").is_file());
    assert!(fixture.root.join("nested/new-report.txt").is_file());
    assert!(!fixture.root.join("old-report.txt").exists());
    assert!(!fixture.root.join("nested/OLD-report.txt").exists());
}

#[test]
fn replace_with_renames_matching_directories_recursively() {
    let fixture = Fixture::new("replace_dirs");
    fixture.write("old-dir/nested-old/file.txt", "content\n");

    let output = run([
        "--dir",
        "old",
        fixture.root.to_str().unwrap(),
        "--replace-with",
        "new",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("| dir"));
    assert!(stdout.contains("old-dir"));
    assert!(stdout.contains("new-dir"));
    assert!(stdout.contains("nested-old"));
    assert!(stdout.contains("nested-new"));
    assert!(stdout.contains("Summary: 2 matches"));
    assert!(fixture.root.join("new-dir/nested-new/file.txt").is_file());
    assert!(!fixture.root.join("old-dir").exists());
}

#[test]
fn replace_with_does_not_overwrite_existing_file_names() {
    let fixture = Fixture::new("replace_file_collision");
    fixture.write("old.txt", "old\n");
    fixture.write("new.txt", "new\n");

    let output = run([
        "--file",
        "old",
        fixture.root.to_str().unwrap(),
        "--replace-with",
        "new",
    ]);

    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 0 matches"));
    assert!(stderr(&output).contains("replacement destination already exists"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("old.txt")).unwrap(),
        "old\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("new.txt")).unwrap(),
        "new\n"
    );
}

#[test]
fn replace_with_is_literal_not_regex() {
    let fixture = Fixture::new("replace_literal");
    fixture.write("regex_like.txt", "a.c a-c aXc\n");

    let output = run([
        "--str",
        "a.c",
        fixture.root.to_str().unwrap(),
        "--replace-with",
        "z",
    ]);

    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 1 matches"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("regex_like.txt")).unwrap(),
        "z a-c aXc\n"
    );
}

#[test]
fn replace_with_requires_string_mode() {
    let fixture = Fixture::new("replace_wrong_mode");
    fixture.write("service.conf", "needle\n");

    let output = run([
        "--regex",
        "needle",
        fixture.root.to_str().unwrap(),
        "--replace-with",
        "thread",
    ]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("--replace-with can only be used with --file, --dir, or --str")
    );
}

#[test]
fn replace_with_leaves_non_utf8_files_unchanged() {
    let fixture = Fixture::new("replace_non_utf8");
    fixture.write_bytes("binary.bin", b"needle \xff needle\n");

    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        "--replace-with",
        "thread",
    ]);

    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 0 matches"));
    assert!(stderr(&output).contains("warning:"));
    assert_eq!(
        fs::read(fixture.root.join("binary.bin")).unwrap(),
        b"needle \xff needle\n"
    );
}

fn run<const N: usize>(args: [&str; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_v_fs_sniffer"))
        .args(args)
        .output()
        .unwrap()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "process failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_table_columns(stdout: &str, expected: &[&str]) {
    let header = stdout
        .lines()
        .find(|line| line.starts_with("| Kind"))
        .expect("table header should be present");
    let columns = header
        .split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>();

    assert_eq!(columns, expected);
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("v_fs_sniffer_{name}_{unique}"));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn write(&self, relative: &str, content: &str) {
        self.write_bytes(relative, content.as_bytes());
    }

    fn write_bytes(&self, relative: &str, content: &[u8]) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        file.write_all(content).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
