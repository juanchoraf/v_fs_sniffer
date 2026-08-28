use crate::args::OutputFormat;
use crate::search::{Finding, FindingKind, SearchReport};

pub fn render(report: &SearchReport, format: OutputFormat) -> String {
    match format {
        OutputFormat::Text => render_text(report),
        OutputFormat::Json => render_json(report),
    }
}

pub fn render_warnings(report: &SearchReport) -> String {
    if report.warnings.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for warning in &report.warnings {
        out.push_str(&format!(
            "warning: {}: {}\n",
            warning.path.display(),
            warning.message
        ));
    }
    out
}

fn render_text(report: &SearchReport) -> String {
    let mut out = String::new();

    if !report.findings.is_empty() {
        out.push_str(&render_text_table(&report.findings));
        out.push('\n');
    }

    out.push_str(&format!(
        "Summary: {} matches, {} files scanned, {} directories scanned, {} entries skipped, {} warnings\n",
        report.findings.len(),
        report.stats.scanned_files,
        report.stats.scanned_dirs,
        report.stats.skipped_entries,
        report.warnings.len()
    ));

    out
}

fn render_text_table(findings: &[Finding]) -> String {
    let show_replacement = findings
        .iter()
        .any(|finding| finding.replaced_with.is_some());
    let mut headers = vec![
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
    ];
    let mut right_aligned = vec![
        false, false, true, true, false, false, true, true, false, true,
    ];

    if show_replacement {
        headers.insert(5, "ReplacedWith");
        right_aligned.insert(5, false);
    }

    let rows = findings
        .iter()
        .map(|finding| finding_cells(finding, show_replacement))
        .collect::<Vec<_>>();
    let widths = column_widths(&headers, &rows);
    let mut out = String::new();

    push_table_row(&mut out, &headers, &widths, &right_aligned);
    push_table_separator(&mut out, &widths);

    for row in &rows {
        let cells = row.iter().map(String::as_str).collect::<Vec<_>>();
        push_table_row(&mut out, &cells, &widths, &right_aligned);
    }

    out
}

fn finding_cells(finding: &Finding, show_replacement: bool) -> Vec<String> {
    let mut cells = vec![
        kind_name(finding.kind).to_owned(),
        table_escape(&finding.path.display().to_string()),
        option_usize_cell(finding.line),
        option_usize_cell(finding.column),
        finding
            .found
            .as_deref()
            .map(table_escape)
            .unwrap_or_default(),
    ];

    if show_replacement {
        cells.push(
            finding
                .replaced_with
                .as_deref()
                .map(table_escape)
                .unwrap_or_default(),
        );
    }

    cells.extend([
        table_escape(&finding.metadata.file_type),
        option_usize_cell(finding.byte_offset),
        finding
            .metadata
            .size_bytes
            .map(|value| value.to_string())
            .unwrap_or_default(),
        finding.metadata.readonly.to_string(),
        finding
            .metadata
            .modified_unix_secs
            .map(|value| value.to_string())
            .unwrap_or_default(),
    ]);

    cells
}

fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths = headers
        .iter()
        .map(|header| display_width(header))
        .collect::<Vec<_>>();

    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(display_width(cell));
        }
    }

    widths
}

fn push_table_row(out: &mut String, cells: &[&str], widths: &[usize], right_aligned: &[bool]) {
    out.push('|');

    for (index, cell) in cells.iter().enumerate() {
        out.push(' ');
        push_padded_cell(out, cell, widths[index], right_aligned[index]);
        out.push(' ');
        out.push('|');
    }

    out.push('\n');
}

fn push_table_separator(out: &mut String, widths: &[usize]) {
    out.push('|');

    for width in widths {
        out.push(' ');
        out.push_str(&"-".repeat(*width));
        out.push(' ');
        out.push('|');
    }

    out.push('\n');
}

fn push_padded_cell(out: &mut String, cell: &str, width: usize, right_aligned: bool) {
    let padding = width.saturating_sub(display_width(cell));

    if right_aligned {
        out.push_str(&" ".repeat(padding));
        out.push_str(cell);
    } else {
        out.push_str(cell);
        out.push_str(&" ".repeat(padding));
    }
}

fn option_usize_cell(value: Option<usize>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

fn table_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '|' => escaped.push_str("\\|"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }

    escaped
}

fn render_json(report: &SearchReport) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!(
        "  \"root\": \"{}\",\n",
        json_escape(&report.root.display().to_string())
    ));
    out.push_str(&format!("  \"mode\": \"{}\",\n", kind_name(report.mode)));
    out.push_str(&format!(
        "  \"case_sensitive\": {},\n  \"recursive\": {},\n",
        report.case_sensitive, report.recursive
    ));
    out.push_str("  \"findings\": [\n");

    for (index, finding) in report.findings.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"kind\": \"{}\",\n      \"path\": \"{}\",\n",
            kind_name(finding.kind),
            json_escape(&finding.path.display().to_string())
        ));
        push_json_option_usize(&mut out, "line", finding.line, true);
        push_json_option_usize(&mut out, "column", finding.column, true);
        push_json_option_usize(&mut out, "byte_offset", finding.byte_offset, true);
        out.push_str(&format!(
            "      \"matched\": {},\n",
            finding
                .matched
                .as_ref()
                .map(|value| format!("\"{}\"", json_escape(value)))
                .unwrap_or_else(|| "null".to_owned())
        ));
        out.push_str(&format!(
            "      \"found\": {},\n",
            finding
                .found
                .as_ref()
                .map(|value| format!("\"{}\"", json_escape(value)))
                .unwrap_or_else(|| "null".to_owned())
        ));
        out.push_str(&format!(
            "      \"replaced_with\": {},\n",
            finding
                .replaced_with
                .as_ref()
                .map(|value| format!("\"{}\"", json_escape(value)))
                .unwrap_or_else(|| "null".to_owned())
        ));
        out.push_str("      \"metadata\": {\n");
        out.push_str(&format!(
            "        \"file_type\": \"{}\",\n        \"size_bytes\": {},\n        \"readonly\": {},\n        \"modified_unix_secs\": {}\n",
            json_escape(&finding.metadata.file_type),
            finding
                .metadata
                .size_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            finding.metadata.readonly,
            finding
                .metadata
                .modified_unix_secs
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned())
        ));
        out.push_str("      }\n");
        out.push_str("    }");
        if index + 1 != report.findings.len() {
            out.push(',');
        }
        out.push('\n');
    }

    out.push_str("  ],\n");
    out.push_str(&format!(
        "  \"stats\": {{\n    \"scanned_dirs\": {},\n    \"scanned_files\": {},\n    \"skipped_entries\": {},\n    \"unreadable_entries\": {}\n  }},\n",
        report.stats.scanned_dirs,
        report.stats.scanned_files,
        report.stats.skipped_entries,
        report.stats.unreadable_entries
    ));
    out.push_str("  \"warnings\": [\n");
    for (index, warning) in report.warnings.iter().enumerate() {
        out.push_str(&format!(
            "    {{ \"path\": \"{}\", \"message\": \"{}\" }}",
            json_escape(&warning.path.display().to_string()),
            json_escape(&warning.message)
        ));
        if index + 1 != report.warnings.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

fn push_json_option_usize(out: &mut String, key: &str, value: Option<usize>, trailing_comma: bool) {
    out.push_str(&format!(
        "      \"{key}\": {}{}\n",
        value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_owned()),
        if trailing_comma { "," } else { "" }
    ));
}

fn kind_name(kind: FindingKind) -> &'static str {
    match kind {
        FindingKind::File => "file",
        FindingKind::Dir => "dir",
        FindingKind::String => "string",
        FindingKind::Regex => "regex",
    }
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
