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
fn wildcard_roots_expand_version_patterns_and_deduplicate() {
    let fixture = Fixture::new("wildcard_versions");
    for version in [
        "v_color_picker_v0.1.2",
        "v_color_picker_v0.1.3",
        "v_color_picker_v0.2.2",
        "v_other_v1.0.0",
    ] {
        fixture.write(&format!("versions/{version}/clue.txt"), "needle\n");
    }
    fixture.write("versions/unrelated/clue.txt", "needle\n");
    for (pattern, count) in [
        ("v_color_picker_v0.1.*", 2),
        ("v_color_picker_v0.*.2", 2),
        ("v_*_v*", 4),
    ] {
        let root = format!("{}/", fixture.root.join("versions").join(pattern).display());
        let output = run(["--str", "needle", &root]);
        assert_success(&output);
        assert!(stdout(&output).contains(&format!("Summary: {count} matches")));
        assert!(!stdout(&output).contains("unrelated"));
    }
    let pattern = fixture.root.join("versions/v_*_v*");
    let literal = fixture.root.join("versions/v_color_picker_v0.1.2");
    let output = run([
        "--file",
        "clue",
        pattern.to_str().unwrap(),
        literal.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&output);
    assert_eq!(stdout(&output).matches("\"kind\": \"file\"").count(), 4);
}

#[test]
fn wildcard_roots_support_relative_paths_spaces_and_multiple_components() {
    let fixture = Fixture::new("wildcard_relative");
    fixture.write("apps/one app/logs/a.log", "needle\n");
    fixture.write("apps/.hidden/logs/b.log", "needle\n");
    fixture.write("apps/one app/logs/deeper/c.log", "needle\n");
    fixture.write("apps/file", "needle\n");
    let output = Command::new(env!("CARGO_BIN_EXE_v_fs_sniffer"))
        .current_dir(&fixture.root)
        .args([
            "--str-regex",
            "needle",
            "./apps/*/logs/*.log",
            "--no-recursive",
        ])
        .output()
        .unwrap();
    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 2 matches"));
    assert!(stdout(&output).contains("a.log"));
    assert!(stdout(&output).contains("b.log"));
    assert!(!stdout(&output).contains("c.log"));
}

#[test]
fn wildcard_roots_honor_trailing_separator_and_exclusions() {
    let fixture = Fixture::new("wildcard_directory_only");
    fixture.write("entry_dir/keep.txt", "needle\n");
    fixture.write("entry_dir/skip.log", "needle\n");
    fixture.write("entry_file", "needle\n");
    let pattern = format!("{}/", fixture.root.join("entry_*").display());
    let output = run(["--str", "needle", &pattern, "-ee", "log", "--no-recursive"]);
    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 1 matches"));
    assert!(stdout(&output).contains("keep.txt"));
    assert!(!stdout(&output).contains("entry_file"));
    assert!(!stdout(&output).contains("skip.log"));
}

#[test]
fn unmatched_root_pattern_is_an_error_even_with_a_valid_root() {
    let fixture = Fixture::new("wildcard_no_matches");
    fixture.write("keep.txt", "needle\n");
    let pattern = fixture.root.join("missing*/logs");
    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        pattern.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("matched no files or directories"));
    assert!(stderr(&output).contains("missing*"));
    assert!(!stdout(&output).contains("keep.txt"));
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
fn extension_presets_exclude_files_in_all_file_search_modes() {
    let fixture = Fixture::new("extension_presets");
    for extension in [
        "MP4", "png", "sqlite", "exe", "mp3", "tar.gz", "woff2", "pdf", "custom",
    ] {
        fixture.write(&format!("nested/sample.{extension}"), "needle\n");
    }
    fixture.write("sample.txt", "needle\n");
    fixture.write("sample.mp4.bak", "needle\n");
    for (mode, query) in [
        ("--file", "sample"),
        ("--str", "needle"),
        ("--str-regex", "needle"),
    ] {
        let output = run([
            mode,
            query,
            fixture.root.to_str().unwrap(),
            "-ee",
            "VIDEO, IMGS, DBS, BINARIES, AUDIO, ARCHIVES, FONTS, DOCUMENTS, .custom",
        ]);
        assert_success(&output);
        let stdout = stdout(&output);
        assert!(stdout.contains("Summary: 2 matches"));
        assert!(stdout.contains("sample.txt"));
        assert!(stdout.contains("sample.mp4.bak"));
    }
    let output = run([
        "--file",
        "sample",
        fixture.root.to_str().unwrap(),
        "-ee",
        "video",
        "--case-sensitive",
    ]);
    assert_success(&output);
    assert!(stdout(&output).contains("sample.MP4"));
}

#[test]
fn excludes_compound_extensions_from_names_and_contents() {
    let fixture = Fixture::new("compound_extensions");
    let archive = "v_color_picker_v0.1.2_linux_x86_64.tar.gz";
    for name in [
        archive,
        "nested/archive.TAR.GZ",
        "archive.bin",
        "archive.deb",
        "archive.zip",
    ] {
        fixture.write(name, "needle\n");
    }
    for name in [
        "keep.gz",
        "keep.tar.gz.bak",
        "keep.notar.gz",
        "keep.txt",
        ".gz",
    ] {
        fixture.write(name, "needle\n");
    }

    for (mode, query) in [
        ("--file", "gz"),
        ("--str", "needle"),
        ("--str-regex", "needle"),
    ] {
        let output = run([
            mode,
            query,
            fixture.root.to_str().unwrap(),
            "-ee",
            ".bin, .deb, .zip, .tar.gz",
        ]);
        assert_success(&output);
        let stdout = stdout(&output);
        assert!(!stdout.contains(archive));
        assert!(!stdout.contains("archive.TAR.GZ"));
        assert!(!stdout.contains("archive.bin"));
        assert!(!stdout.contains("archive.deb"));
        assert!(!stdout.contains("archive.zip"));
        assert!(stdout.contains("keep.gz"));
        assert!(stdout.contains("keep.tar.gz.bak"));
        assert!(stdout.contains("keep.notar.gz"));
    }

    let output = run([
        "--file",
        archive,
        fixture.root.to_str().unwrap(),
        "-ee",
        ".bin, .deb, .zip, .tar.gz",
    ]);
    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 0 matches"));

    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        "-ee",
        "tar.gz",
        "--case-sensitive",
    ]);
    assert_success(&output);
    assert!(!stdout(&output).contains(archive));
    assert!(stdout(&output).contains("archive.TAR.GZ"));

    let output = run([
        "--str",
        "needle",
        fixture.root.to_str().unwrap(),
        "-ee",
        "gz",
    ]);
    assert_success(&output);
    assert!(!stdout(&output).contains(archive));
    assert!(!stdout(&output).contains("keep.gz"));
    assert!(stdout(&output).contains("keep.tar.gz.bak"));
    assert!(stdout(&output).contains("| .gz"));
}

#[test]
fn file_regex_matches_only_names_and_honors_search_options() {
    let fixture = Fixture::new("file_regex");
    fixture.write("report1.txt", "unrelated\n");
    fixture.write("nested/REPORT2.TXT", "unrelated\n");
    fixture.write("report3.txt/child.bin", "unrelated\n");
    fixture.write("other.txt", "report4.txt\n");
    fixture.write(".report5.txt", "unrelated\n");
    let pattern = r"^report[0-9]+\.txt$";
    let root = fixture.root.to_str().unwrap();
    for options in [
        vec![],
        vec!["--case-sensitive"],
        vec!["--no-recursive"],
        vec!["-ex", "nested"],
    ] {
        let mut args = vec!["--file-regex", pattern, root];
        args.extend_from_slice(&options);
        let output = run(args);
        assert_success(&output);
        let stdout = stdout(&output);
        assert!(stdout.contains("report1.txt"));
        assert!(!stdout.contains("report3.txt"));
        assert!(!stdout.contains("other.txt"));
        assert!(!stdout.contains(".report5.txt"));
        assert!(stdout.contains(if options.is_empty() {
            "Summary: 2 matches"
        } else {
            "Summary: 1 matches"
        }));
    }
    let output = run(["--file-regex", pattern, root, "-ee", "txt"]);
    assert_success(&output);
    assert!(stdout(&output).contains("Summary: 0 matches"));
    let file = fixture.root.join("report1.txt");
    let output = run(["--file-regex", pattern, file.to_str().unwrap(), "--json"]);
    assert_success(&output);
    assert!(stdout(&output).contains("\"kind\": \"file\""));
    assert!(stdout(&output).contains("\"line\": null"));
}

#[test]
fn dir_regex_matches_names_including_hidden_directories() {
    let fixture = Fixture::new("dir_regex");
    fixture.write(".git/config", "unrelated\n");
    fixture.write("nested/.GIT/config", "unrelated\n");
    fixture.write("other/.git", "unrelated\n");
    fixture.write("text.txt", ".git\n");
    let root = fixture.root.to_str().unwrap();
    for (pattern, options, count) in [
        (r"^\.git$", vec![], 2),
        (r"^\.git$", vec!["--case-sensitive"], 1),
        (r"/^\.git$/i", vec!["--case-sensitive"], 2),
        (r"^\.git$", vec!["--no-recursive"], 1),
        (r"^\.git$", vec!["-ex", "nested"], 1),
        (r"^\.git$", vec!["-er", r"/\.git$/i"], 0),
    ] {
        let mut args = vec!["--dir-regex", pattern, root];
        args.extend(options);
        let output = run(args);
        assert_success(&output);
        assert!(stdout(&output).contains(&format!("Summary: {count} matches")));
        assert!(!stdout(&output).contains("text.txt"));
    }
}

#[test]
fn str_regex_searches_contents_across_roots_without_matching_names() {
    let first = Fixture::new("str_regex_first");
    let second = Fixture::new("str_regex_second");
    first.write("error404.txt", "unrelated\n");
    first.write("logs/app.log", "header\nERROR: 404\n");
    second.write("service.log", "error 500\n");
    let output = run([
        "--str-regex",
        "error[ :]+[0-9]+",
        first.root.to_str().unwrap(),
        second.root.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&output);
    let stdout = stdout(&output);
    assert!(!stdout.contains("error404.txt"));
    assert!(stdout.contains("app.log"));
    assert!(stdout.contains("service.log"));
    assert!(stdout.contains("\"line\": 2"));
    assert!(stdout.contains("\"column\": 1"));
    assert_eq!(stdout.matches("\"kind\": \"regex\"").count(), 2);
}

#[test]
fn regex_modes_reject_invalid_patterns() {
    let fixture = Fixture::new("invalid_regex_modes");
    for flag in ["--file-regex", "--dir-regex", "--str-regex"] {
        let output = run([flag, "[", fixture.root.to_str().unwrap()]);
        assert!(!output.status.success());
        assert!(stderr(&output).contains("regex"));
    }
}

#[test]
fn delimited_regex_flags_are_supported() {
    let fixture = Fixture::new("regex_flags");
    fixture.write("images.txt", "A.png\n7.png\n");

    let output = run([
        "--str-regex",
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
        "--str-regex",
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

fn run<'a>(args: impl IntoIterator<Item = &'a str>) -> Output {
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
