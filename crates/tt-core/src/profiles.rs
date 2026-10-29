//! Data-driven device profiles. Anyone can add a JSON file under `profiles/`.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SftpSupport {
    Full,
    ScpOnly,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickCommand {
    pub group: String,
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub dangerous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostLoginStep {
    /// Text to wait for in the device output before sending (empty = send right away).
    #[serde(default)]
    pub expect: String,
    pub send: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_port: u16,
    pub pty: bool,
    pub sftp: SftpSupport,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub username_suffix: Option<String>,
    #[serde(default)]
    pub init_commands: Vec<String>,
    #[serde(default)]
    pub post_login: Vec<PostLoginStep>,
    #[serde(default)]
    pub enable_support: bool,
    #[serde(default)]
    pub legacy_hint: bool,
    #[serde(default)]
    pub quick_commands: Vec<QuickCommand>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl DeviceProfile {
    pub fn is_netconf(&self) -> bool {
        self.mode.as_deref() == Some("netconf")
    }

    /// Username actually sent to the device (e.g. MikroTik `admin+ct`).
    pub fn effective_username(&self, user: &str) -> String {
        match &self.username_suffix {
            Some(s) if !user.contains('+') => format!("{user}{s}"),
            _ => user.to_string(),
        }
    }
}

const BUILTIN: &[&str] = &[
    include_str!("../../../profiles/generic.json"),
    include_str!("../../../profiles/debian.json"),
    include_str!("../../../profiles/raspberry-pi.json"),
    include_str!("../../../profiles/openwrt.json"),
    include_str!("../../../profiles/pfsense.json"),
    include_str!("../../../profiles/opnsense.json"),
    include_str!("../../../profiles/mikrotik.json"),
    include_str!("../../../profiles/cisco-ios.json"),
    include_str!("../../../profiles/junos.json"),
    include_str!("../../../profiles/huawei-vrp.json"),
    include_str!("../../../profiles/netconf.json"),
];

pub fn builtin_profiles() -> Vec<DeviceProfile> {
    BUILTIN
        .iter()
        .map(|s| serde_json::from_str(s).expect("invalid built-in profile"))
        .collect()
}

/// Built-in profiles plus any `*.json` found in `dir` (user profiles override by id).
pub fn load_profiles(dir: Option<&std::path::Path>) -> Vec<DeviceProfile> {
    let mut list = builtin_profiles();
    if let Some(dir) = dir {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if e.path().extension().and_then(|x| x.to_str()) != Some("json") {
                    continue;
                }
                match std::fs::read_to_string(e.path())
                    .map_err(anyhow::Error::from)
                    .and_then(|s| serde_json::from_str::<DeviceProfile>(&s).map_err(Into::into))
                {
                    Ok(p) => {
                        list.retain(|x| x.id != p.id);
                        list.push(p);
                    }
                    Err(err) => eprintln!("ignoring profile {:?}: {err}", e.path()),
                }
            }
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_and_are_unique() {
        let p = builtin_profiles();
        let mut ids: Vec<_> = p.iter().map(|x| x.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), p.len());
        assert!(p.iter().any(|x| x.is_netconf()));
    }

    #[test]
    fn mikrotik_suffix() {
        let p = builtin_profiles().into_iter().find(|x| x.id == "mikrotik").unwrap();
        assert_eq!(p.effective_username("admin"), "admin+ct");
        assert_eq!(p.effective_username("admin+c"), "admin+c");
    }
}
