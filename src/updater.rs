use std::cmp::Ordering;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::VFsSnifferError;
use v_concat::*;

const APP_NAME: &str = "v_fs_sniffer";
const GITHUB_API_BASE: &str = "https://api.github.com/repos";
const BUILD_GITHUB_REPO: Option<&str> = option_env!("V_FS_SNIFFER_GITHUB_REPO");
const CARGO_REPOSITORY: Option<&str> = option_env!("CARGO_PKG_REPOSITORY");

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubAsset {
    name: String,
    download_url: String,
}

pub fn check_update(repo_override: Option<&str>) -> Result<String, VFsSnifferError> {
    let repo = resolve_github_repo(repo_override)?;
    let release = fetch_latest_release(&repo)?;
    let latest_version = normalize_version(&release.tag_name);
    let current_version = env!("CARGO_PKG_VERSION");
    let candidates = compatible_asset_names(latest_version);
    let selected = select_asset(&release.assets, &candidates);

    let status = match compare_versions(latest_version, current_version) {
        Ordering::Greater => {
            v_concat!(
                "Update available: {} -> {}",
                current_version,
                latest_version
            )
        }
        Ordering::Equal => v_concat!("{} is up to date at {}", APP_NAME, current_version),
        Ordering::Less => v_concat!(
            "Installed version {} is newer than GitHub latest {}",
            current_version,
            latest_version
        ),
    };

    let asset_line = if let Some(asset) = selected {
        v_concat!("Compatible release asset: {}", asset.name)
    } else {
        v_concat!(
            "No compatible release asset found for {} {}",
            env::consts::OS,
            normalized_arch()
        )
    };

    Ok(v_concat!(
        "{}\nRepository: {}\nLatest tag: {}\n{}\n",
        status,
        repo,
        release.tag_name,
        asset_line
    ))
}

pub fn install_update(repo_override: Option<&str>) -> Result<String, VFsSnifferError> {
    let repo = resolve_github_repo(repo_override)?;
    let release = fetch_latest_release(&repo)?;
    let latest_version = normalize_version(&release.tag_name);
    let current_version = env!("CARGO_PKG_VERSION");

    match compare_versions(latest_version, current_version) {
        Ordering::Greater => {}
        Ordering::Equal => {
            return Ok(v_concat!(
                "{} is already up to date at {}\n",
                APP_NAME,
                current_version
            ));
        }
        Ordering::Less => {
            return Ok(v_concat!(
                "Installed version {} is newer than GitHub latest {}\n",
                current_version,
                latest_version
            ));
        }
    }

    let candidates = compatible_asset_names(latest_version);
    let asset = select_asset(&release.assets, &candidates).ok_or_else(|| {
        VFsSnifferError::new(v_concat!(
            "no compatible release asset found for {} {}; expected one of: {}",
            env::consts::OS,
            normalized_arch(),
            candidates.join(", ")
        ))
    })?;

    let temp_dir = prepare_temp_dir(latest_version)?;
    let artifact_path = temp_dir.join(&asset.name);
    download_file(&asset.download_url, &artifact_path)?;

    let checksum_message = verify_download_checksum(&release, asset, &artifact_path, &temp_dir)?;
    let install_message = install_downloaded_asset(&artifact_path, &asset.name)?;

    Ok(v_concat!(
        "Downloaded {}\n{}\n{}\n",
        asset.name,
        checksum_message,
        install_message
    ))
}

fn resolve_github_repo(repo_override: Option<&str>) -> Result<String, VFsSnifferError> {
    if let Some(repo) = repo_override.and_then(non_empty_trimmed) {
        return validate_repo(repo);
    }

    if let Ok(repo) = env::var("V_FS_SNIFFER_GITHUB_REPO") {
        if let Some(repo) = non_empty_trimmed(&repo) {
            return validate_repo(repo);
        }
    }

    if let Some(repo) = BUILD_GITHUB_REPO.and_then(non_empty_trimmed) {
        return validate_repo(repo);
    }

    if let Some(repo) = CARGO_REPOSITORY
        .and_then(non_empty_trimmed)
        .and_then(github_repo_from_url)
    {
        return validate_repo(&repo);
    }

    Err(VFsSnifferError::new(
        "GitHub repository is not configured. Rebuild releases with V_FS_SNIFFER_GITHUB_REPO=owner/repo or pass --github-repo owner/repo.",
    ))
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn validate_repo(repo: &str) -> Result<String, VFsSnifferError> {
    let mut parts = repo.split('/');
    let Some(owner) = parts.next() else {
        return invalid_repo(repo);
    };
    let Some(name) = parts.next() else {
        return invalid_repo(repo);
    };
    if parts.next().is_some() || owner.is_empty() || name.is_empty() {
        return invalid_repo(repo);
    }
    if !repo
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'))
    {
        return invalid_repo(repo);
    }

    Ok(repo.to_owned())
}

fn invalid_repo(repo: &str) -> Result<String, VFsSnifferError> {
    Err(VFsSnifferError::new(v_concat!(
        "invalid GitHub repository '{}', expected owner/repo",
        repo
    )))
}

fn github_repo_from_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git");
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() {
        return None;
    }

    Some(v_concat!("{}/{}", owner, repo))
}

fn fetch_latest_release(repo: &str) -> Result<GitHubRelease, VFsSnifferError> {
    let url = v_concat!("{}/{}/releases/latest", GITHUB_API_BASE, repo);
    let body = download_text(&url)?;
    let tag_name = json_string_field(&body, "tag_name").ok_or_else(|| {
        VFsSnifferError::new("GitHub latest release response did not include tag_name")
    })?;
    let assets = parse_assets(&body);

    Ok(GitHubRelease { tag_name, assets })
}

fn normalize_version(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = normalize_version(left);
    let right = normalize_version(right);
    let left_main = left.split_once('-').map_or(left, |(main, _)| main);
    let right_main = right.split_once('-').map_or(right, |(main, _)| main);
    let left_parts = left_main.split('.').collect::<Vec<_>>();
    let right_parts = right_main.split('.').collect::<Vec<_>>();
    let max_len = left_parts.len().max(right_parts.len());

    for index in 0..max_len {
        let left_part = left_parts
            .get(index)
            .and_then(|part| part.parse::<u64>().ok())
            .unwrap_or(0);
        let right_part = right_parts
            .get(index)
            .and_then(|part| part.parse::<u64>().ok())
            .unwrap_or(0);

        match left_part.cmp(&right_part) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }

    left.cmp(right)
}

fn compatible_asset_names(version: &str) -> Vec<String> {
    let arch = normalized_arch();
    let versioned_name = v_concat!("{}_v{}", APP_NAME, version);
    let mut names = Vec::new();

    match env::consts::OS {
        "windows" => {
            push_artifact(&mut names, &versioned_name, "windows", &arch, "exe");
            push_artifact(&mut names, &versioned_name, "windows", &arch, "msi");
            if arch == "arm64" {
                push_artifact(&mut names, &versioned_name, "windows", "x86_64", "exe");
                push_artifact(&mut names, &versioned_name, "windows", "x86_64", "msi");
            }
            push_artifact(&mut names, &versioned_name, "windows", &arch, "zip");
        }
        "macos" => {
            push_artifact(&mut names, &versioned_name, "macos", &arch, "pkg");
            push_artifact(&mut names, &versioned_name, "macos", &arch, "tar.gz");
            push_artifact(&mut names, &versioned_name, "macos", &arch, "zip");
        }
        "linux" => {
            if is_debian_like() {
                push_artifact(&mut names, &versioned_name, "linux", &arch, "deb");
            }
            push_artifact(&mut names, &versioned_name, "linux", &arch, "tar.gz");
            push_artifact(&mut names, &versioned_name, "linux", &arch, "zip");
        }
        other_unix => {
            push_artifact(&mut names, &versioned_name, other_unix, &arch, "tar.gz");
            push_artifact(&mut names, &versioned_name, other_unix, &arch, "zip");
            push_artifact(&mut names, &versioned_name, "unix", &arch, "tar.gz");
            push_artifact(&mut names, &versioned_name, "unix", &arch, "zip");
        }
    }

    names
}

fn push_artifact(names: &mut Vec<String>, versioned_name: &str, os: &str, arch: &str, ext: &str) {
    names.push(v_concat!("{}_{}_{}.{}", versioned_name, os, arch, ext));
}

fn normalized_arch() -> String {
    match env::consts::ARCH {
        "aarch64" => "arm64".to_owned(),
        "x86_64" => "x86_64".to_owned(),
        other => other.to_owned(),
    }
}

fn is_debian_like() -> bool {
    if command_available("apt") || command_available("apt-get") || command_available("dpkg") {
        return true;
    }

    let Ok(os_release) = fs::read_to_string("/etc/os-release") else {
        return false;
    };
    let lower = os_release.to_ascii_lowercase();
    lower.contains("id=debian") || lower.contains("id=ubuntu") || lower.contains("id_like=debian")
}

fn select_asset<'a>(assets: &'a [GitHubAsset], candidates: &[String]) -> Option<&'a GitHubAsset> {
    candidates
        .iter()
        .find_map(|candidate| assets.iter().find(|asset| asset.name == *candidate))
}

fn checksum_name_for(asset_name: &str) -> String {
    let base = asset_name
        .strip_suffix(".tar.gz")
        .or_else(|| asset_name.strip_suffix(".zip"))
        .or_else(|| asset_name.strip_suffix(".deb"))
        .or_else(|| asset_name.strip_suffix(".pkg"))
        .or_else(|| asset_name.strip_suffix(".exe"))
        .or_else(|| asset_name.strip_suffix(".msi"))
        .unwrap_or(asset_name);

    v_concat!("{}.sha256", base)
}

fn verify_download_checksum(
    release: &GitHubRelease,
    asset: &GitHubAsset,
    artifact_path: &Path,
    temp_dir: &Path,
) -> Result<String, VFsSnifferError> {
    let checksum_name = checksum_name_for(&asset.name);
    let Some(checksum_asset) = release
        .assets
        .iter()
        .find(|release_asset| release_asset.name == checksum_name)
    else {
        return Ok(v_concat!(
            "Warning: no SHA-256 checksum asset found for {}",
            asset.name
        ));
    };

    let checksum_path = temp_dir.join(&checksum_asset.name);
    download_file(&checksum_asset.download_url, &checksum_path)?;
    let checksum_text = fs::read_to_string(&checksum_path).map_err(|err| {
        VFsSnifferError::new(v_concat!(
            "failed to read checksum file '{}': {}",
            checksum_path.display(),
            err
        ))
    })?;
    let expected = expected_checksum(&checksum_text, &asset.name).ok_or_else(|| {
        VFsSnifferError::new(v_concat!(
            "checksum file '{}' does not include {}",
            checksum_asset.name,
            asset.name
        ))
    })?;
    let actual = file_sha256(artifact_path)?;

    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(VFsSnifferError::new(v_concat!(
            "SHA-256 mismatch for {}: expected {}, got {}",
            asset.name,
            expected,
            actual
        )));
    }

    Ok(v_concat!("Verified SHA-256: {}", actual))
}

fn expected_checksum(checksum_text: &str, asset_name: &str) -> Option<String> {
    checksum_text.lines().find_map(|line| {
        if !line.contains(asset_name) {
            return None;
        }

        let checksum = line.split_whitespace().next()?;
        if checksum.len() == 64 && checksum.chars().all(|ch| ch.is_ascii_hexdigit()) {
            Some(checksum.to_owned())
        } else {
            None
        }
    })
}

fn prepare_temp_dir(version: &str) -> Result<PathBuf, VFsSnifferError> {
    let dir = env::temp_dir().join(v_concat!(
        "{}_update_{}_{}",
        APP_NAME,
        version,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|err| {
        VFsSnifferError::new(v_concat!(
            "failed to create update temp directory '{}': {}",
            dir.display(),
            err
        ))
    })?;

    Ok(dir)
}

fn install_downloaded_asset(path: &Path, asset_name: &str) -> Result<String, VFsSnifferError> {
    if cfg!(windows) {
        return install_windows_asset(path, asset_name);
    }

    if asset_name.ends_with(".deb") {
        return install_deb(path);
    }
    if asset_name.ends_with(".pkg") {
        return install_pkg(path);
    }
    if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".zip") {
        return install_unix_archive(path, asset_name);
    }

    Err(VFsSnifferError::new(v_concat!(
        "downloaded {}, but this platform does not know how to install that asset type",
        asset_name
    )))
}

fn install_windows_asset(path: &Path, asset_name: &str) -> Result<String, VFsSnifferError> {
    if asset_name.ends_with(".exe") {
        Command::new(path).spawn().map_err(|err| {
            VFsSnifferError::new(v_concat!(
                "failed to start Windows installer '{}': {}",
                path.display(),
                err
            ))
        })?;
        return Ok(
            "Started the Windows installer. Complete the installer to finish updating.".to_owned(),
        );
    }

    if asset_name.ends_with(".msi") {
        let path_str = path_to_string(path)?;
        Command::new("msiexec")
            .args(["/i", path_str.as_str()])
            .spawn()
            .map_err(|err| {
                VFsSnifferError::new(v_concat!(
                    "failed to start msiexec for '{}': {}",
                    path.display(),
                    err
                ))
            })?;
        return Ok("Started msiexec. Complete the installer to finish updating.".to_owned());
    }

    Err(VFsSnifferError::new(v_concat!(
        "downloaded {}, but Windows updates require the .exe or .msi installer asset",
        asset_name
    )))
}

fn install_deb(path: &Path) -> Result<String, VFsSnifferError> {
    let path_str = path_to_string(path)?;
    if command_available("apt") {
        run_privileged("apt", &["install", "-y", path_str.as_str()])?;
        return Ok("Installed the Debian package with apt.".to_owned());
    }
    if command_available("apt-get") {
        run_privileged("apt-get", &["install", "-y", path_str.as_str()])?;
        return Ok("Installed the Debian package with apt-get.".to_owned());
    }
    if command_available("dpkg") {
        run_privileged("dpkg", &["-i", path_str.as_str()])?;
        return Ok("Installed the Debian package with dpkg.".to_owned());
    }

    Err(VFsSnifferError::new(
        "downloaded a .deb update, but apt, apt-get, and dpkg were not found",
    ))
}

fn install_pkg(path: &Path) -> Result<String, VFsSnifferError> {
    let path_str = path_to_string(path)?;
    run_privileged("installer", &["-pkg", path_str.as_str(), "-target", "/"])?;
    Ok("Installed the macOS package with installer.".to_owned())
}

fn install_unix_archive(path: &Path, asset_name: &str) -> Result<String, VFsSnifferError> {
    let extract_dir = prepare_temp_dir("extract")?;
    if asset_name.ends_with(".tar.gz") {
        let path_str = path_to_string(path)?;
        run_command(
            Command::new("tar")
                .arg("-xzf")
                .arg(&path_str)
                .arg("-C")
                .arg(&extract_dir),
            "extract update archive",
        )?;
    } else {
        let path_str = path_to_string(path)?;
        run_command(
            Command::new("unzip")
                .arg("-q")
                .arg(&path_str)
                .arg("-d")
                .arg(&extract_dir),
            "extract update archive",
        )?;
    }

    let new_binary = extract_dir.join(APP_NAME).join("bin").join(APP_NAME);
    if !new_binary.is_file() {
        return Err(VFsSnifferError::new(v_concat!(
            "update archive did not contain '{}'",
            new_binary.display()
        )));
    }

    install_binary_over_current(&new_binary)
}

fn install_binary_over_current(new_binary: &Path) -> Result<String, VFsSnifferError> {
    let current = env::current_exe().map_err(|err| {
        VFsSnifferError::new(v_concat!(
            "failed to resolve current executable path: {}",
            err
        ))
    })?;

    if let Some(parent) = current.parent() {
        let staged = parent.join(v_concat!(".{}_update_{}", APP_NAME, std::process::id()));
        if fs::copy(new_binary, &staged).is_ok() {
            set_executable(&staged)?;
            match fs::rename(&staged, &current) {
                Ok(()) => {
                    return Ok(v_concat!(
                        "Replaced current binary at {}",
                        current.display()
                    ));
                }
                Err(_) => {
                    let _ = fs::remove_file(&staged);
                }
            }
        }
    }

    let new_binary_str = path_to_string(new_binary)?;
    let current_str = path_to_string(&current)?;
    run_privileged(
        "install",
        &["-m", "0755", new_binary_str.as_str(), current_str.as_str()],
    )?;
    Ok(v_concat!(
        "Replaced current binary at {}",
        current.display()
    ))
}

fn set_executable(path: &Path) -> Result<(), VFsSnifferError> {
    #[cfg(not(unix))]
    let _ = path;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|err| {
            VFsSnifferError::new(v_concat!(
                "failed to mark '{}' executable: {}",
                path.display(),
                err
            ))
        })?;
    }

    Ok(())
}

fn run_privileged(program: &str, args: &[&str]) -> Result<(), VFsSnifferError> {
    if cfg!(windows) || is_root() {
        return run_command(Command::new(program).args(args), program);
    }

    if !command_available("sudo") {
        return Err(VFsSnifferError::new(v_concat!(
            "{} needs elevated permissions; rerun as root or install sudo",
            program
        )));
    }

    run_command(Command::new("sudo").arg(program).args(args), program)
}

fn is_root() -> bool {
    let Ok(output) = Command::new("id").arg("-u").output() else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "0"
}

fn command_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn run_command(command: &mut Command, description: &str) -> Result<(), VFsSnifferError> {
    let status = command
        .status()
        .map_err(|err| VFsSnifferError::new(v_concat!("failed to run {}: {}", description, err)))?;

    if status.success() {
        Ok(())
    } else {
        Err(VFsSnifferError::new(v_concat!(
            "{} exited with status {}",
            description,
            status
        )))
    }
}

fn download_text(url: &str) -> Result<String, VFsSnifferError> {
    #[cfg(windows)]
    {
        let script = "$ErrorActionPreference = 'Stop'; [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; (Invoke-WebRequest -UseBasicParsing -Uri $env:V_FS_SNIFFER_UPDATE_URL -Headers @{Accept='application/vnd.github+json'; 'User-Agent'='v_fs_sniffer'}).Content";
        return powershell_output(script, &[("V_FS_SNIFFER_UPDATE_URL", url)]);
    }

    #[cfg(not(windows))]
    {
        if let Ok(output) = Command::new("curl")
            .args([
                "-fsSL",
                "-H",
                "Accept: application/vnd.github+json",
                "-H",
                "User-Agent: v_fs_sniffer",
                url,
            ])
            .output()
        {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
        }

        if let Ok(output) = Command::new("fetch").args(["-qo", "-", url]).output() {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
        }

        Err(VFsSnifferError::new(
            "failed to download release metadata; install curl or fetch and verify network access",
        ))
    }
}

fn download_file(url: &str, path: &Path) -> Result<(), VFsSnifferError> {
    #[cfg(windows)]
    {
        let path_str = path_to_string(path)?;
        let script = "$ErrorActionPreference = 'Stop'; [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -UseBasicParsing -Uri $env:V_FS_SNIFFER_UPDATE_URL -OutFile $env:V_FS_SNIFFER_UPDATE_OUT -Headers @{'User-Agent'='v_fs_sniffer'}";
        powershell_output(
            script,
            &[
                ("V_FS_SNIFFER_UPDATE_URL", url),
                ("V_FS_SNIFFER_UPDATE_OUT", &path_str),
            ],
        )?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let path_str = path_to_string(path)?;
        if run_command(
            Command::new("curl")
                .arg("-fL")
                .arg("-o")
                .arg(&path_str)
                .arg(url),
            "download release asset",
        )
        .is_ok()
        {
            return Ok(());
        }

        run_command(
            Command::new("fetch").arg("-o").arg(&path_str).arg(url),
            "download release asset",
        )
    }
}

fn file_sha256(path: &Path) -> Result<String, VFsSnifferError> {
    #[cfg(windows)]
    {
        let path_str = path_to_string(path)?;
        let script = "$ErrorActionPreference = 'Stop'; (Get-FileHash -Algorithm SHA256 -Path $env:V_FS_SNIFFER_HASH_PATH).Hash.ToLowerInvariant()";
        return powershell_output(script, &[("V_FS_SNIFFER_HASH_PATH", &path_str)])
            .map(|text| text.trim().to_owned());
    }

    #[cfg(not(windows))]
    {
        let path_str = path_to_string(path)?;
        if let Ok(output) = Command::new("sha256sum").arg(&path_str).output() {
            if output.status.success() {
                if let Some(hash) = String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .next()
                {
                    return Ok(hash.to_owned());
                }
            }
        }

        if let Ok(output) = Command::new("shasum")
            .arg("-a")
            .arg("256")
            .arg(&path_str)
            .output()
        {
            if output.status.success() {
                if let Some(hash) = String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .next()
                {
                    return Ok(hash.to_owned());
                }
            }
        }

        Err(VFsSnifferError::new(
            "failed to compute SHA-256; install sha256sum or shasum",
        ))
    }
}

#[cfg(windows)]
fn powershell_output(script: &str, envs: &[(&str, &str)]) -> Result<String, VFsSnifferError> {
    let mut last_error = None;
    for shell in ["pwsh", "powershell"] {
        let mut command = Command::new(shell);
        command.args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ]);
        for (name, value) in envs {
            command.env(name, value);
        }

        match command.output() {
            Ok(output) if output.status.success() => {
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
            Ok(output) => {
                last_error = Some(String::from_utf8_lossy(&output.stderr).trim().to_owned());
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    Err(VFsSnifferError::new(v_concat!(
        "PowerShell update command failed: {}",
        last_error.unwrap_or_else(|| "PowerShell was not found".to_owned())
    )))
}

fn path_to_string(path: &Path) -> Result<String, VFsSnifferError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        VFsSnifferError::new(v_concat!("path '{}' is not valid UTF-8", path.display()))
    })
}

fn json_string_field(object: &str, field: &str) -> Option<String> {
    let needle = v_concat!("\"{}\"", field);
    let start = object.find(&needle)? + needle.len();
    let after_key = object[start..].trim_start();
    let after_colon = after_key.strip_prefix(':')?.trim_start();

    parse_json_string(after_colon).map(|(value, _)| value)
}

fn parse_assets(json: &str) -> Vec<GitHubAsset> {
    let Some(assets_start) = json.find("\"assets\"") else {
        return Vec::new();
    };
    let Some(array_start) = json[assets_start..].find('[') else {
        return Vec::new();
    };
    let array_start = assets_start + array_start;
    let Some(array_end) = matching_json_container(json, array_start, '[', ']') else {
        return Vec::new();
    };
    let assets_json = &json[array_start + 1..array_end];
    let mut assets = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = assets_json[cursor..].find('{') {
        let object_start = cursor + relative_start;
        let Some(object_end) = matching_json_container(assets_json, object_start, '{', '}') else {
            break;
        };
        let object = &assets_json[object_start..=object_end];
        if let (Some(name), Some(download_url)) = (
            json_string_field(object, "name"),
            json_string_field(object, "browser_download_url"),
        ) {
            assets.push(GitHubAsset { name, download_url });
        }
        cursor = object_end + 1;
    }

    assets
}

fn matching_json_container(value: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;

    for (index, ch) in value[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(start + index);
            }
        }
    }

    None
}

fn parse_json_string(value: &str) -> Option<(String, usize)> {
    let mut chars = value.char_indices();
    let (_, first) = chars.next()?;
    if first != '"' {
        return None;
    }

    let mut out = String::new();
    let mut escaped = false;

    for (index, ch) in chars {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{08}'),
                'f' => out.push('\u{0c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => out.push('?'),
                other => out.push(other),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some((out, index + ch.len_utf8()));
        } else {
            out.push(ch);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        checksum_name_for, compare_versions, expected_checksum, github_repo_from_url, parse_assets,
    };
    use std::cmp::Ordering;

    #[test]
    fn parses_github_repository_urls() {
        assert_eq!(
            github_repo_from_url("https://github.com/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            github_repo_from_url("git@github.com:owner/repo").as_deref(),
            Some("owner/repo")
        );
    }

    #[test]
    fn version_comparison_uses_numeric_parts() {
        assert_eq!(compare_versions("0.1.10", "0.1.9"), Ordering::Greater);
        assert_eq!(compare_versions("v0.1.5", "0.1.5"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.4", "0.1.5"), Ordering::Less);
    }

    #[test]
    fn parses_release_assets_from_github_json() {
        let assets = parse_assets(
            r#"{
                "tag_name": "v1.2.3",
                "assets": [
                    {
                        "name": "v_fs_sniffer_v1.2.3_linux_x86_64.deb",
                        "browser_download_url": "https://example.test/download.deb"
                    }
                ]
            }"#,
        );

        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "v_fs_sniffer_v1.2.3_linux_x86_64.deb");
        assert_eq!(assets[0].download_url, "https://example.test/download.deb");
    }

    #[test]
    fn checksum_names_match_packaging_scripts() {
        assert_eq!(
            checksum_name_for("v_fs_sniffer_v1.2.3_linux_x86_64.tar.gz"),
            "v_fs_sniffer_v1.2.3_linux_x86_64.sha256"
        );
        assert_eq!(
            checksum_name_for("v_fs_sniffer_v1.2.3_windows_x86_64.exe"),
            "v_fs_sniffer_v1.2.3_windows_x86_64.sha256"
        );
    }

    #[test]
    fn expected_checksum_reads_matching_asset_line() {
        let checksum = expected_checksum(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  app.zip\n",
            "app.zip",
        );

        assert_eq!(
            checksum.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }
}
