//! Masks secrets in terminal text before it leaves the machine (AI assistant / MCP).
//! Best effort: it removes the common patterns, it cannot guarantee a secret never leaks.
use regex::Regex;
use std::sync::OnceLock;

const MASK: &str = "[REDACTED]";

fn rules() -> &'static Vec<(Regex, String)> {
    static R: OnceLock<Vec<(Regex, String)>> = OnceLock::new();
    R.get_or_init(|| {
        let r = |p: &str, rep: &str| (Regex::new(p).expect("valid regex"), rep.to_string());
        vec![
            // PEM / OpenSSH private key blocks (multi-line)
            r(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(-----END [A-Z ]*PRIVATE KEY-----|\z)", "[REDACTED PRIVATE KEY]"),
            // Cisco / Huawei style: "secret 5 $1$...", "password 7 0822455D0A16", "enable secret 9 $9$..."
            r(r"(?i)\b(secret|password|passwd|pwd)(\s+\d)?\s+\S+", "$1$2 [REDACTED]"),
            // key=value forms: password=abc, passphrase: abc, psk=..., token=..., api_key=...
            r(r#"(?i)\b(password|passwd|passphrase|pre-shared-key|psk|wpa-psk|secret|token|api[_-]?key|apikey|auth-key|private-key|community)\s*[=:]\s*("[^"]*"|'[^']*'|\S+)"#, "$1=[REDACTED]"),
            // SNMP communities
            r(r"(?i)\bsnmp-server\s+community\s+\S+", "snmp-server community [REDACTED]"),
            // HTTP auth headers and bearer tokens
            r(r"(?i)\b(authorization:\s*(bearer|basic))\s+\S+", "$1 [REDACTED]"),
            r(r"\b(sk-[A-Za-z0-9_-]{16,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16})\b", MASK),
            // Password prompts answered on the same line ("Password: hunter2")
            r(r"(?im)^(\s*password\s*:)\s*\S+.*$", "$1 [REDACTED]"),
        ]
    })
}

pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    for (re, rep) in rules() {
        out = re.replace_all(&out, rep.as_str()).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_common_patterns() {
        let cases = [
            ("enable secret 9 $9$abcDEF123", "secret 9 [REDACTED]"),
            ("username admin privilege 15 secret 5 $1$xyz$abc", "secret 5 [REDACTED]"),
            ("/user add name=bob password=hunter2 group=full", "password=[REDACTED]"),
            ("wpa-psk=\"my wifi pass\"", "wpa-psk=[REDACTED]"),
            ("snmp-server community public RO", "snmp-server community [REDACTED]"),
            ("Authorization: Bearer abc.def.ghi", "Bearer [REDACTED]"),
            ("key sk-abcdefghijklmnopqrstuvwxyz123456", "[REDACTED]"),
        ];
        for (input, expect) in cases {
            let out = redact(input);
            assert!(out.contains(expect), "{input:?} -> {out:?}");
            assert!(!out.contains("hunter2") && !out.contains("abcDEF123") && !out.contains("my wifi pass"));
        }
    }

    #[test]
    fn masks_private_key_block() {
        let t = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\nAAAA\n-----END OPENSSH PRIVATE KEY-----\nafter";
        let out = redact(t);
        assert!(out.contains("[REDACTED PRIVATE KEY]") && !out.contains("b3Blbn") && out.contains("after"));
    }

    #[test]
    fn leaves_normal_output() {
        let t = "ether1  running  192.168.88.1/24\nuptime 3d";
        assert_eq!(redact(t), t);
    }
}
