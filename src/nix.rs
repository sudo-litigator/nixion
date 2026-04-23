use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};
use time::{macros::format_description, OffsetDateTime, UtcOffset};

#[derive(Clone, Debug)]
pub struct InstalledPackage {
    pub index: String,
    pub name: String,
    pub attr_path: String,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct SearchPackage {
    pub attr: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub struct Generation {
    pub generation: u32,
    pub summary: String,
    pub created_at: String,
    pub age: String,
    pub booted: bool,
    pub running: bool,
}

#[derive(Clone, Debug)]
pub struct FlakeHost {
    pub name: String,
    pub current: bool,
}

#[derive(Clone, Debug)]
pub struct FlakeInfo {
    pub path: PathBuf,
    pub description: String,
    pub url: String,
    pub revision: String,
    pub last_modified: String,
    pub input_count: usize,
    pub hosts: Vec<FlakeHost>,
    pub config_files: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationAction {
    Switch,
    Test,
    Boot,
}

impl GenerationAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Switch => "switch",
            Self::Test => "test",
            Self::Boot => "boot",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RebuildAction {
    Switch,
    Test,
    Boot,
}

impl RebuildAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Switch => "switch",
            Self::Test => "test",
            Self::Boot => "boot",
        }
    }
}

#[derive(Default)]
pub struct NixClient;

impl NixClient {
    pub fn discover_flake_at(&self, path: &Path) -> Result<FlakeInfo> {
        self.flake_info(path)
    }

    pub fn discover_flake(&self) -> Result<FlakeInfo> {
        let mut fallback = None;

        for candidate in self.flake_candidates()? {
            match self.flake_info(&candidate) {
                Ok(info) if !info.hosts.is_empty() => return Ok(info),
                Ok(info) => {
                    if fallback.is_none() {
                        fallback = Some(info);
                    }
                }
                Err(_) => continue,
            }
        }

        fallback.ok_or_else(|| {
            anyhow!("no usable flake found in the current directory tree or /etc/nixos")
        })
    }

    pub fn installed_packages(&self) -> Result<Vec<InstalledPackage>> {
        let output = self.run_capture("nix", &["profile", "list", "--json"])?;
        let root: Value =
            serde_json::from_str(&output).context("failed to parse nix profile JSON")?;
        let elements = root.get("elements").unwrap_or(&root);

        let mut packages = match elements {
            Value::Object(map) => map
                .iter()
                .filter_map(|(key, value)| Self::parse_installed_package(key, value))
                .collect::<Vec<_>>(),
            Value::Array(items) => items
                .iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    Self::parse_installed_package(&index.to_string(), value)
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };

        packages.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(packages)
    }

    pub fn search_packages(&self, query: &str) -> Result<Vec<SearchPackage>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let output = self.run_capture("nix", &["search", "nixpkgs", query, "--json"])?;
        let root: Value =
            serde_json::from_str(&output).context("failed to parse nix search JSON")?;

        let mut results = match root {
            Value::Object(map) => map
                .into_iter()
                .map(|(attr, value)| SearchPackage {
                    attr,
                    description: value
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("no description")
                        .to_string(),
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };

        results.sort_by(|left, right| left.attr.cmp(&right.attr));
        Ok(results)
    }

    pub fn install_package(&self, attr: &str) -> Result<()> {
        self.run_status("nix", &["profile", "install", &format!("nixpkgs#{attr}")])
    }

    pub fn remove_package(&self, index: &str) -> Result<()> {
        self.run_status("nix", &["profile", "remove", index])
    }

    pub fn list_generations(&self) -> Result<Vec<Generation>> {
        let profiles_dir = Path::new("/nix/var/nix/profiles");
        let boot_generation = Self::active_generation_number(&profiles_dir.join("system"));
        let running_target = Self::resolve_symlink_target(Path::new("/run/current-system")).ok();
        let now = SystemTime::now();
        let mut discovered = Vec::new();

        for entry in fs::read_dir(profiles_dir)
            .with_context(|| format!("failed to read {}", profiles_dir.display()))?
        {
            let entry = entry?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let Some(generation) = Self::parse_generation_link_name(&file_name) else {
                continue;
            };

            let link_path = entry.path();
            let target = Self::resolve_symlink_target(&link_path)
                .with_context(|| format!("failed to resolve {}", link_path.display()))?;
            let modified = fs::symlink_metadata(&link_path)
                .with_context(|| format!("failed to inspect {}", link_path.display()))?
                .modified()
                .ok();

            discovered.push((generation, target, modified));
        }

        discovered.sort_by(|left, right| right.0.cmp(&left.0));

        let running_index = running_target.as_ref().and_then(|target| {
            discovered
                .iter()
                .position(|(_, generation_target, _)| generation_target == target)
        });

        let generations = discovered
            .into_iter()
            .enumerate()
            .map(|(index, (generation, target, modified))| Generation {
                generation,
                summary: Self::summarize_generation_target(&target),
                created_at: modified
                    .map(Self::format_system_time)
                    .unwrap_or_else(|| String::from("unknown")),
                age: modified
                    .map(|time| Self::format_relative_age(time, now))
                    .unwrap_or_else(|| String::from("unknown age")),
                booted: boot_generation == Some(generation),
                running: running_index == Some(index),
            })
            .collect::<Vec<_>>();

        Ok(generations)
    }

    pub fn activate_generation(&self, generation: u32, action: GenerationAction) -> Result<()> {
        let script =
            format!("/nix/var/nix/profiles/system-{generation}-link/bin/switch-to-configuration");

        self.run_status("sudo", &[&script, action.label()])
    }

    pub fn delete_generation(&self, generation: u32) -> Result<()> {
        let generation = generation.to_string();
        self.run_status(
            "sudo",
            &[
                "nix-env",
                "-p",
                "/nix/var/nix/profiles/system",
                "--delete-generations",
                &generation,
            ],
        )
    }

    pub fn delete_old_generations(&self) -> Result<()> {
        self.run_status(
            "sudo",
            &[
                "nix-env",
                "-p",
                "/nix/var/nix/profiles/system",
                "--delete-generations",
                "old",
            ],
        )
    }

    pub fn flake_info(&self, root: &Path) -> Result<FlakeInfo> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let root_arg = root.to_string_lossy().to_string();

        let metadata_output = self.run_capture_in(
            "nix",
            &["flake", "metadata", "--json", root_arg.as_str()],
            Some(&root),
        )?;
        let metadata: Value = serde_json::from_str(&metadata_output)
            .context("failed to parse nix flake metadata JSON")?;

        let show_output = self.run_capture_in(
            "nix",
            &["flake", "show", "--json", root_arg.as_str()],
            Some(&root),
        )?;
        let show: Value =
            serde_json::from_str(&show_output).context("failed to parse nix flake show JSON")?;

        let current_host = self.current_hostname();
        let mut hosts = show
            .get("nixosConfigurations")
            .and_then(Value::as_object)
            .map(|hosts| {
                hosts
                    .keys()
                    .map(|name| FlakeHost {
                        name: name.to_string(),
                        current: current_host.as_deref() == Some(name.as_str()),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        hosts.sort_by(|left, right| left.name.cmp(&right.name));

        let description = metadata
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("no description")
            .to_string();
        let url = metadata
            .get("originalUrl")
            .or_else(|| metadata.get("url"))
            .and_then(Value::as_str)
            .unwrap_or("local flake")
            .to_string();
        let revision = metadata
            .get("revision")
            .or_else(|| metadata.get("locked").and_then(|locked| locked.get("rev")))
            .and_then(Value::as_str)
            .unwrap_or("workspace")
            .to_string();
        let last_modified = metadata
            .get("lastModified")
            .map(Self::value_as_string)
            .unwrap_or_else(|| String::from("unknown"));
        let input_count = metadata
            .get("locks")
            .and_then(|locks| locks.get("nodes"))
            .and_then(Value::as_object)
            .map(|nodes| nodes.len().saturating_sub(1))
            .unwrap_or(0);
        let config_files = self.collect_nix_files(&root)?;

        Ok(FlakeInfo {
            path: root,
            description,
            url,
            revision,
            last_modified,
            input_count,
            hosts,
            config_files,
        })
    }

    pub fn flake_check(&self, root: &Path) -> Result<()> {
        let root_arg = root.to_string_lossy().to_string();
        self.run_status_in("nix", &["flake", "check", root_arg.as_str()], Some(root))
    }

    pub fn flake_update(&self, root: &Path) -> Result<()> {
        self.run_status_in("nix", &["flake", "update"], Some(root))
    }

    pub fn flake_format(&self, root: &Path) -> Result<()> {
        self.run_status_in("nix", &["fmt"], Some(root))
    }

    pub fn rebuild_host(&self, root: &Path, host: &str, action: RebuildAction) -> Result<()> {
        let flake_ref = format!("{}#{host}", root.to_string_lossy());
        self.run_status_in(
            "sudo",
            &[
                "nixos-rebuild",
                action.label(),
                "--flake",
                flake_ref.as_str(),
            ],
            Some(root),
        )
    }

    fn parse_installed_package(index: &str, value: &Value) -> Option<InstalledPackage> {
        let attr_path = value
            .get("attrPath")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let name = value
            .get("pname")
            .and_then(Value::as_str)
            .or_else(|| value.get("name").and_then(Value::as_str))
            .map(ToString::to_string)
            .filter(|name| !name.is_empty())
            .or_else(|| {
                attr_path
                    .split('.')
                    .next_back()
                    .map(ToString::to_string)
                    .filter(|name| !name.is_empty())
            })
            .unwrap_or_else(|| index.to_string());

        let source = value
            .get("originalUrl")
            .or_else(|| value.get("url"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        Some(InstalledPackage {
            index: index.to_string(),
            name,
            attr_path,
            source,
        })
    }

    fn parse_generation_link_name(file_name: &str) -> Option<u32> {
        file_name
            .strip_prefix("system-")?
            .strip_suffix("-link")?
            .parse::<u32>()
            .ok()
    }

    fn active_generation_number(path: &Path) -> Option<u32> {
        let target = fs::read_link(path).ok()?;
        let file_name = target.file_name()?.to_string_lossy();
        Self::parse_generation_link_name(&file_name)
    }

    fn resolve_symlink_target(path: &Path) -> Result<PathBuf> {
        fs::canonicalize(path).or_else(|_| {
            let target = fs::read_link(path)
                .with_context(|| format!("failed to read symlink {}", path.display()))?;

            if target.is_absolute() {
                Ok(target)
            } else {
                Ok(path.parent().unwrap_or_else(|| Path::new("/")).join(target))
            }
        })
    }

    fn summarize_generation_target(target: &Path) -> String {
        let file_name = target
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| target.display().to_string());

        file_name
            .split_once('-')
            .map(|(_, summary)| summary.to_string())
            .unwrap_or(file_name)
    }

    fn format_system_time(time: SystemTime) -> String {
        let datetime = OffsetDateTime::from(time);
        let datetime = UtcOffset::current_local_offset()
            .map(|offset| datetime.to_offset(offset))
            .unwrap_or(datetime);

        datetime
            .format(&format_description!("[year]-[month]-[day] [hour]:[minute]"))
            .unwrap_or_else(|_| String::from("unknown"))
    }

    fn format_relative_age(time: SystemTime, now: SystemTime) -> String {
        match now.duration_since(time) {
            Ok(duration) => Self::humanize_duration(duration.as_secs(), false),
            Err(error) => Self::humanize_duration(error.duration().as_secs(), true),
        }
    }

    fn humanize_duration(seconds: u64, future: bool) -> String {
        if seconds < 60 {
            return if future {
                String::from("soon")
            } else {
                String::from("just now")
            };
        }

        let (value, unit) = if seconds < 3_600 {
            (seconds / 60, "m")
        } else if seconds < 86_400 {
            (seconds / 3_600, "h")
        } else if seconds < 604_800 {
            (seconds / 86_400, "d")
        } else if seconds < 2_592_000 {
            (seconds / 604_800, "w")
        } else {
            (seconds / 2_592_000, "mo")
        };

        if future {
            format!("in {value}{unit}")
        } else {
            format!("{value}{unit} ago")
        }
    }

    fn run_capture(&self, program: &str, args: &[&str]) -> Result<String> {
        self.run_capture_in(program, args, None)
    }

    fn run_capture_in(&self, program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String> {
        let mut command = Command::new(program);
        command.args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        let output = command
            .output()
            .with_context(|| format!("failed to execute {program}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let details = if stderr.is_empty() { stdout } else { stderr };
            bail!("{program} {} failed: {details}", args.join(" "));
        }

        String::from_utf8(output.stdout).map_err(|error| anyhow!(error))
    }

    fn run_status(&self, program: &str, args: &[&str]) -> Result<()> {
        self.run_status_in(program, args, None)
    }

    fn run_status_in(&self, program: &str, args: &[&str], cwd: Option<&Path>) -> Result<()> {
        let mut command = Command::new(program);
        command.args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        let output = command
            .output()
            .with_context(|| format!("failed to execute {program}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let details = if stderr.is_empty() { stdout } else { stderr };
            bail!("{program} {} failed: {details}", args.join(" "));
        }

        Ok(())
    }

    fn flake_candidates(&self) -> Result<Vec<PathBuf>> {
        let current_dir = env::current_dir().context("failed to determine current directory")?;
        let mut candidates = Vec::new();

        for ancestor in current_dir.ancestors() {
            if ancestor.join("flake.nix").is_file() {
                candidates.push(ancestor.to_path_buf());
            }
        }

        let etc_nixos = PathBuf::from("/etc/nixos");
        if etc_nixos.join("flake.nix").is_file() && !candidates.contains(&etc_nixos) {
            candidates.push(etc_nixos);
        }

        Ok(candidates)
    }

    fn collect_nix_files(&self, root: &Path) -> Result<Vec<String>> {
        let mut files = Vec::new();
        let mut stack = vec![root.to_path_buf()];

        while let Some(directory) = stack.pop() {
            for entry in fs::read_dir(&directory)
                .with_context(|| format!("failed to read {}", directory.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                let file_type = entry.file_type()?;
                let name = entry.file_name();
                let name = name.to_string_lossy();

                if file_type.is_dir() {
                    if matches!(name.as_ref(), ".git" | ".direnv" | "result" | "target") {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }

                if file_type.is_file()
                    && path.extension().and_then(|ext| ext.to_str()) == Some("nix")
                {
                    let relative = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .display()
                        .to_string();
                    files.push(relative);
                }
            }
        }

        files.sort();
        Ok(files)
    }

    fn current_hostname(&self) -> Option<String> {
        env::var("HOSTNAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.run_capture("hostname", &[])
                    .ok()
                    .map(|hostname| hostname.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
    }

    fn value_as_string(value: &Value) -> String {
        match value {
            Value::String(string) => string.clone(),
            Value::Number(number) => number.to_string(),
            Value::Bool(boolean) => boolean.to_string(),
            _ => value.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::NixClient;

    #[test]
    fn parses_system_generation_link_names() {
        assert_eq!(
            NixClient::parse_generation_link_name("system-28-link"),
            Some(28)
        );
        assert_eq!(NixClient::parse_generation_link_name("system-link"), None);
        assert_eq!(
            NixClient::parse_generation_link_name("default-28-link"),
            None
        );
    }

    #[test]
    fn summarizes_generation_target_from_store_path() {
        let target = Path::new(
            "/nix/store/8pm85p9069rwd34pvrsvkyhkkhrf4zfs-nixos-system-nixos-26.05.20260418.b12141e",
        );

        assert_eq!(
            NixClient::summarize_generation_target(target),
            "nixos-system-nixos-26.05.20260418.b12141e"
        );
    }
}
