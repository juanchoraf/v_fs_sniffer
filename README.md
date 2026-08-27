# v_fs_sniffer

`v_fs_sniffer` is a fast Rust CLI for filesystem search and cleanup. It finds files, directories, text, regex matches, hidden entries, and exact line ranges; can rename entries, replace UTF-8 text, exclude noisy paths, export text/JSON reports, and install published updates.

Run one-shot commands or open its interactive terminal with history and Tab completion.

Made with AI (Codex) 🤖

## Supported OS

| Platform           |
| ------------------ |
| Linux              |
| Windows            |
| macOS              |
| Unix               |

## Minimum Supported Rust Version

This crate requires Rust 1.94.1 or later.

## Features

- Finds files by name with case-insensitive matching by default
- Finds directories by name with recursive traversal
- Searches literal text inside files
- Searches file contents with the built-in regex engine
- Reads an exact line or inclusive line range from one file with `--file <path> --lines <start:end>`
- Recurses through hidden entries such as `.git` and `.htaccess`
- Follows symlinked files and directories by default
- Can disable recursion with `--no-recursive`
- Can disable symlink following with `--no-follow-symlinks`
- Can explicitly re-enable symlink following with `--follow-symlinks`
- Can switch matching to case-sensitive with `--case-sensitive`
- Renames matching files without overwriting existing paths
- Renames matching directories recursively without overwriting existing paths
- Replaces literal text in UTF-8 files
- Skips non-UTF-8 files safely during text replacement and reports warnings
- Excludes directory subtrees by path or name
- Excludes files by path or name
- Excludes file extensions from content scans
- Excludes content lines containing specific text
- Excludes paths or content lines with regex rules
- Prints aligned text tables with match metadata
- Prints structured JSON output
- Auto-selects JSON output for `.json` export files
- Exports reports to a file
- Supports quiet mode for file-only output workflows
- Prints warnings to stderr and findings to stdout
- Shows one updating progress line during scans
- Highlights interactive results, summaries, progress, prompts, and errors with terminal colors
- Displays errors in red with padded output
- Opens an interactive terminal when run without arguments
- Shows a cyan/yellow search funnel and large blue ASCII app name in interactive mode
- Uses a cyan `v_fs_sniffer>` prompt while keeping typed commands white
- Supports interactive command history
- Supports Tab completion for commands
- Supports Tab completion for filesystem paths, including paths with spaces
- Supports `help`, `clear`, `version`, `check-update`, `update`, `exit`, and `quit` inside the interactive terminal
- Checks for published updates with `--check-update`
- Downloads, verifies, and installs matching published updates with `--update`
- Supports per-user Cargo uninstall with `--uninstall`
- Builds portable archives and installers for 64-bit systems
- Supports Windows, Linux, macOS, BSD, illumos/Solaris, and other Unix-like systems

## Quick Start

Build and open the interactive terminal:

```bash
cargo build --release
cargo run --
```

Inside the terminal, press Tab to complete commands and paths, including paths with spaces:

```text
v_fs_sniffer> --str "needle" "C:\Program Files"
v_fs_sniffer> clear
v_fs_sniffer> version
```

The `clear` command clears the entire console and redraws the ASCII logo and header.
The `version` command prints only the installed app version, such as `0.1.0`.

Run one-shot searches from source:

```bash
cargo run -- --str "needle" .
cargo run -- --file "Cargo.toml" .
cargo run -- --file "src/main.rs" --lines 260:320
```

After installation:

```bash
v_fs_sniffer --str "TODO" . --exclude-dir target
v_fs_sniffer --regex '/error|warning/i' .
v_fs_sniffer --check-update
v_fs_sniffer --update
```

## Requirements

- Rust stable toolchain with Cargo for source builds
- PowerShell on Windows
- Administrator rights for system-wide Windows installation
- `sudo` or root access for system-wide Linux and macOS installation

Dependencies are declared in `Cargo.toml`. Rustyline provides cross-platform command history and Tab completion.

## Build from Source

From the project root:

```bash
cargo build --release
```

| Platform | Release binary |
| --- | --- |
| Windows | `target\release\v_fs_sniffer.exe` |
| Unix-like systems | `target/release/v_fs_sniffer` |

Run the built binary:

```powershell
# Windows PowerShell
.\target\release\v_fs_sniffer.exe --help
```

```bash
# Linux, macOS, BSD, and other Unix-like systems
./target/release/v_fs_sniffer --help
```

Use double quotes for quoted arguments in Windows `cmd.exe`.

## Build Release Packages

Release scripts create 64-bit packages in `versions/v_fs_sniffer_vTHEACTUALVERSION/` using this convention:

```text
v_fs_sniffer_vTHEACTUALVERSION_architecture.extension
```

Run the builder for the current operating system:

```bash
sh scripts/build_binaries.sh
```

Pass `--locked` to use the lockfile or `--no-update` to skip `cargo update`.

### Linux

```bash
sh scripts/build_binaries_linux.sh
```

Creates a portable binary, `.tar.gz`, optional `.zip`, `.deb`, and SHA-256 checksums. The `.deb` is published by `TheVelasquez.com`, installs the app for every user, and includes its terminal desktop entry and logo.

### macOS

```bash
sh scripts/build_binaries_macos.sh
```

Creates a portable binary, `.tar.gz`, optional `.zip`, optional `.pkg`, and SHA-256 checksums.

### BSD, illumos/Solaris, and Other Unix-like Systems

```bash
sh scripts/build_binaries_unix.sh
```

Creates a portable binary, `.tar.gz`, optional `.zip`, and SHA-256 checksums using the current OS and architecture in the artifact name.

### Windows

```powershell
.\scripts\build_binaries_windows.ps1
```

Or from Command Prompt:

```bat
scripts\build_binaries_windows.cmd
```

Creates a `.zip`, optional `.msi`, optional `.exe` installer, and SHA-256 checksums. The installers use `TheVelasquez.com` as publisher and launch the app in PowerShell so colors remain visible. When no signing certificate is configured, the PowerShell builder can generate a local certificate under `certs\`.

## Install for All Users

Use a generated package from `versions/v_fs_sniffer_vTHEACTUALVERSION/`.

### Windows

Run either installer as Administrator:

```text
v_fs_sniffer_vTHEACTUALVERSION_windows_x86_64.exe
v_fs_sniffer_vTHEACTUALVERSION_windows_x86_64.msi
```

The installer adds `v_fs_sniffer` to the machine `Path` and creates a Start Menu launcher that opens PowerShell.

### Debian and Ubuntu

```bash
sudo apt install ./versions/v_fs_sniffer_vTHEACTUALVERSION/v_fs_sniffer_vTHEACTUALVERSION_linux_x86_64.deb
```

### macOS

```bash
# Apple Silicon
sudo installer -pkg ./versions/v_fs_sniffer_vTHEACTUALVERSION/v_fs_sniffer_vTHEACTUALVERSION_macos_arm64.pkg -target /
```

Use the `macos_x86_64.pkg` artifact on Intel Macs.

### Other Unix-like Systems

Extract `v_fs_sniffer_vTHEACTUALVERSION_ARCH.tar.gz`, built on a compatible system, and place the binary somewhere all users can execute it, such as `/usr/local/bin/v_fs_sniffer`.

Verify the installation:

```bash
v_fs_sniffer --help
```

## Update app

Install the newer package over the older one. Do not uninstall first. Published release builds can fetch the newest release:

```bash
v_fs_sniffer --check-update
v_fs_sniffer --update
```

`--update` downloads the matching release asset, verifies its SHA-256 checksum when the checksum asset exists, then starts the native update path for the current OS. Windows runs the `.exe` or `.msi`; Debian/Ubuntu installs the `.deb`; macOS installs the `.pkg`; portable Unix installs replace the current binary from a matching archive. Normal OS permission prompts still apply.

| Platform | Update command |
| --- | --- |
| Windows `.exe` or `.msi` | Run the newer installer as Administrator. It keeps the same installer identity and upgrades the installed app in place. |
| Debian/Ubuntu `.deb` | `sudo apt install ./versions/v_fs_sniffer_vTHEACTUALVERSION/v_fs_sniffer_vTHEACTUALVERSION_linux_x86_64.deb` |
| macOS `.pkg` | `sudo installer -pkg ./versions/v_fs_sniffer_vTHEACTUALVERSION/v_fs_sniffer_vTHEACTUALVERSION_macos_ARCH.pkg -target /` |
| Portable Unix binary | Replace `/usr/local/bin/v_fs_sniffer` with the newer binary while no `v_fs_sniffer` process is running. |
| Cargo per-user install | `cargo install --path . --force` |

The Linux package also replaces the legacy `fs-sniffer` package name. The macOS package removes legacy `fs_sniffer` paths during installation.

## Install for the Current User

```bash
cargo install --path . --force
```

Ensure Cargo's bin directory is on `PATH`, then run `v_fs_sniffer --help`.

## Uninstall

`v_fs_sniffer` does not create config directories, caches, logs, services, scheduled tasks, shell-profile edits, or startup entries.

| Installation | Uninstall command |
| --- | --- |
| Windows installer | `winget uninstall v_fs_sniffer`, Windows Settings, or the installer's Remove option |
| Debian/Ubuntu package | `sudo apt remove v-fs-sniffer` |
| Cargo per-user install | `cargo uninstall v_fs_sniffer` or `v_fs_sniffer --uninstall` |
| Portable Unix binary | `sudo rm -f /usr/local/bin/v_fs_sniffer` |

For a macOS package installation:

```bash
sudo rm -f /usr/local/bin/v_fs_sniffer
sudo rm -rf /usr/local/share/v_fs_sniffer
```

Cargo uninstall commands affect only the current user. Package-manager uninstall commands do not delete this source checkout.

## Command Reference

```text
v_fs_sniffer
v_fs_sniffer --file <path> --lines <start:end>
v_fs_sniffer --file <name> <root> [options]
v_fs_sniffer --file <name> <root> --replace-with <name> [options]
v_fs_sniffer --dir <name> <root> [options]
v_fs_sniffer --dir <name> <root> --replace-with <name> [options]
v_fs_sniffer --str <text> <root> [options]
v_fs_sniffer --str <text> <root> --replace-with <text> [options]
v_fs_sniffer --regex <expr> <root> [options]
v_fs_sniffer --check-update
v_fs_sniffer --update
v_fs_sniffer --uninstall
```

Running without arguments opens the interactive terminal. A one-shot command requires exactly one search mode.

### Search Modes

| Mode | Description |
| --- | --- |
| `--file <name>` | Find files whose names contain the value. |
| `--file <path> --lines <start:end>` | Read a 1-based inclusive line range from one file. |
| `--file <name> --replace-with <name>` | Rename matching files. |
| `--dir <name>` | Find directories whose names contain the value. |
| `--dir <name> --replace-with <name>` | Rename matching directories. |
| `--str <text>` | Find literal text inside files. |
| `--str <text> --replace-with <text>` | Replace literal text in UTF-8 files. |
| `--regex <expr>`, `-rx <expr>` | Find regex matches inside files. |

Matching is case-insensitive and recursive by default. Symlink targets are followed by default. File and directory replacement changes only the entry name, never its parent, and never overwrites a destination. Non-UTF-8 files are skipped with a warning during string replacement.

### General Options

| Option | Description |
| --- | --- |
| `-nr`, `--no-recursive` | Search only the root's direct children. |
| `-cs`, `--case-sensitive` | Match case sensitively. |
| `--no-follow-symlinks` | Inspect symlinks without following their targets. |
| `--follow-symlinks` | Follow symlink targets explicitly. This is the default behavior. |
| `--lines <start:end>` | Read a line range from `--file <path>` without a search root. |
| `--replace-with <text>` | Replace file, directory, or literal string matches. |
| `-o`, `--output`, `--export <file>` | Export findings. |
| `--output-format text`, `--text` | Force text output. |
| `--output-format json`, `--json` | Force JSON output. |
| `-q`, `--quiet` | Suppress findings on stdout. |
| `--check-update` | Check for a newer published version and matching asset. |
| `--update` | Download, verify, and install the latest matching published asset. |
| `--uninstall` | Remove the current user's Cargo installation. |
| `-h`, `--help` | Print help. |
| `-V`, `--version` | Print the version. |

A `.json` output extension selects JSON automatically unless text output is explicitly requested.

### Exclusion Options

| Option | Description |
| --- | --- |
| `-ex`, `-ed`, `--exclude-dir <value>` | Exclude a directory subtree by name or path. |
| `-ef`, `--exclude-file <value>` | Exclude files by name or path. |
| `-ee`, `--exclude-extensions <values>` | Exclude comma-, semicolon-, or space-separated extensions. |
| `-el`, `--exclude-line <text>` | Exclude lines containing literal text. |
| `-er`, `--exclude-regex <expr>` | Exclude matching paths and lines by regex. |

Exclusions follow the active case-sensitivity setting. Extensions may include or omit the leading dot.

## Examples

These installed-command examples work in PowerShell and Unix-like shells.

```bash
# Find and rename entries
v_fs_sniffer --file "Cargo.toml" .
v_fs_sniffer --file "apps/web/src/lib/company/CompanyProfilePage.svelte" --lines 260:620
v_fs_sniffer --file "old_name" . --replace-with "new_name"
v_fs_sniffer --dir "src" .
v_fs_sniffer --dir "old_name" . --replace-with "new_name"

# Find and replace text
v_fs_sniffer --str "TODO" .
v_fs_sniffer --str "old_value" . --replace-with "new_value"
v_fs_sniffer --str "needle" ./src/main.rs

# Control traversal and matching
v_fs_sniffer --str "Password" . --case-sensitive
v_fs_sniffer --file ".rs" . --no-recursive
v_fs_sniffer --str "needle" . --no-follow-symlinks

# Use regular expressions
v_fs_sniffer --regex '[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}' .
v_fs_sniffer --regex '/error|warning/i' .

# Exclude content
v_fs_sniffer --str "needle" . --exclude-dir target --exclude-dir node_modules
v_fs_sniffer --str "secret" . --exclude-file ".env.example" --exclude-line "fake-secret"
v_fs_sniffer --str "needle" . --exclude-extensions ".log,tmp,md, .mp4 .zip .webm .exe"
v_fs_sniffer --str "needle" . --exclude-regex '/(^|\/)fixtures(\/|$)/'

# Export results
v_fs_sniffer --str "api_key" . --output findings.json
v_fs_sniffer --str "api_key" . --json --output findings.json --quiet
```

## Output

JSON output contains `root`, `mode`, `case_sensitive`, `recursive`, `findings`, `stats`, and `warnings`. Findings include their kind, path, optional line and column, byte offset, matched text, and basic file metadata.

## Regex Support

The built-in regex engine supports:

- Literals, `.`, alternation, groups, and non-capturing groups
- Character classes such as `[abc]`, `[^abc]`, and `[a-z]`
- Escapes including `\d`, `\D`, `\w`, `\W`, `\s`, `\S`, `\n`, `\r`, and `\t`
- Quantifiers `*`, `+`, `?`, `{n}`, `{n,m}`, and `{n,}`
- Anchors `^` and `$`
- Delimited expressions such as `/pattern/imsgxU`

| Flag | Meaning |
| --- | --- |
| `i` | Case-insensitive matching. |
| `m` | Multiline anchors. |
| `s` | Dot matches newlines. |
| `g` | Accepted for compatibility; matches are already global per line. |
| `x` | Ignore whitespace and `#` comments outside character classes. |
| `U` | Accepted for compatibility; matching remains greedy. |

Word-boundary escapes, lookaround, and backreferences are not supported.

## Exit Codes

| Code | Meaning |
| --- | --- |
| `0` | Completed successfully, including searches with zero findings. |
| `2` | Invalid arguments or regex, unreadable root, or output write failure. |

Warnings for unreadable entries are printed to stderr and included in JSON output.

## Test

```bash
cargo test
```

## Credits

Powered by The Velasquez.

## License

The `v_fs_sniffer` library is distributed under either of

 * [Apache License, Version 2.0][LICENSE-APACHE]
 * [MIT license][LICENSE-MIT]

at your convenience.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

[//]: # (badges)

[GitHub Actions]: https://github.com/juanchoraf/v_fs_sniffer/actions?query=branch:main
[Build Status]: https://github.com/juanchoraf/v_fs_sniffer/actions/workflows/rust.yml/badge.svg?branch=main
[docs.rs]: https://docs.rs/v_fs_sniffer
[Documentation]: https://docs.rs/v_fs_sniffer/badge.svg
[deps.rs]: https://deps.rs/repo/github/juanchoraf/v_fs_sniffer
[Dependency Status]: https://deps.rs/repo/github/juanchoraf/v_fs_sniffer/status.svg
[License]: https://img.shields.io/crates/l/v_fs_sniffer

[//]: # (licenses)

[LICENSE-APACHE]: https://github.com/juanchoraf/v_fs_sniffer/blob/main/LICENSE-APACHE
[LICENSE-MIT]: https://github.com/juanchoraf/v_fs_sniffer/blob/main/LICENSE-MIT
