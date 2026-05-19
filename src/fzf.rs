use crate::aws_config::SsoProfile;
use crate::cache::LoginStatus;
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

struct PickerEntry<'a> {
    original_index: usize,
    profile: &'a SsoProfile,
    status: LoginStatus,
    is_current: bool,
}

pub fn is_available() -> bool {
    Command::new("fzf")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn select_profile(
    profiles: &[SsoProfile],
    statuses: &[LoginStatus],
    current_profile: Option<&str>,
) -> Result<String> {
    if profiles.is_empty() {
        bail!("no complete AWS SSO profiles found");
    }

    if !is_available() {
        bail!("fzf is required for interactive profile selection; install fzf or run awsp use <profile>");
    }

    let rows = build_rows(profiles, statuses, current_profile);

    let mut command = Command::new("fzf");
    command
        .args([
            "--delimiter",
            "\t",
            "--with-nth",
            "2..",
            "--nth",
            "3..",
            "--prompt",
            "awsp> ",
            "--height",
            "40%",
            "--layout",
            "reverse",
            "--border",
            "--header",
            "current profile is marked with * | region ending in * is inherited from [default]",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = command.spawn().with_context(|| "failed to start fzf")?;
    {
        let mut stdin = child.stdin.take().context("failed to open fzf stdin")?;
        stdin
            .write_all(rows.as_bytes())
            .with_context(|| "failed to write profiles to fzf")?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| "failed to wait for fzf")?;

    if !output.status.success() {
        bail!("profile selection cancelled");
    }

    let selected = String::from_utf8_lossy(&output.stdout);
    let Some(index) = selected
        .split('\t')
        .next()
        .and_then(|value| value.trim().parse::<usize>().ok())
    else {
        bail!("fzf returned an invalid selection");
    };

    profiles
        .get(index)
        .map(|profile| profile.name.clone())
        .context("fzf returned an out-of-range selection")
}

fn build_rows(
    profiles: &[SsoProfile],
    statuses: &[LoginStatus],
    current_profile: Option<&str>,
) -> String {
    let mut entries = profiles
        .iter()
        .enumerate()
        .map(|(original_index, profile)| PickerEntry {
            original_index,
            profile,
            status: statuses
                .get(original_index)
                .copied()
                .unwrap_or(LoginStatus::Unknown),
            is_current: Some(profile.name.as_str()) == current_profile,
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        picker_rank(left)
            .cmp(&picker_rank(right))
            .then_with(|| left.profile.name.cmp(&right.profile.name))
    });

    let mut rows = String::new();
    for entry in entries {
        let marker = if entry.is_current { "*" } else { "" };
        rows.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            entry.original_index,
            marker,
            entry.profile.name,
            entry.profile.role_name,
            entry.profile.region.label(),
            entry.status,
        ));
    }

    rows
}

fn picker_rank(entry: &PickerEntry<'_>) -> u8 {
    if entry.is_current {
        return 0;
    }

    match entry.status {
        LoginStatus::Valid => 1,
        LoginStatus::Expired => 2,
        LoginStatus::Unknown => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws_config::RegionDisplay;

    fn profile(name: &str) -> SsoProfile {
        SsoProfile {
            name: name.to_string(),
            sso_session: Some("corp".to_string()),
            sso_start_url: "https://example.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            role_name: "Admin".to_string(),
            region: RegionDisplay::Unset,
        }
    }

    #[test]
    fn current_profile_is_first_without_filtering_rows() {
        let profiles = vec![profile("dev"), profile("prod"), profile("staging")];
        let statuses = vec![LoginStatus::Valid, LoginStatus::Unknown, LoginStatus::Valid];
        let rows = build_rows(&profiles, &statuses, Some("prod"));
        let lines = rows.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("\t*\tprod\t"));
        assert!(lines.iter().any(|line| line.contains("\t\tdev\t")));
        assert!(lines.iter().any(|line| line.contains("\t\tstaging\t")));
    }
}
