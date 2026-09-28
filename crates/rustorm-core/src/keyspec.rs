//! The value type and form group of every ssh_config(5) keyword a host can
//! set, for the TUI and GUI settings forms, and the check each value
//! passes before it is written.

use crate::keys::{canonical_key, is_multi_valued, KNOWN_KEYS};

/// Where a keyword shows in the settings form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyGroup {
    /// Who and where to connect, and the session itself.
    Connection,
    /// Keys, agents, passwords and host-key checking.
    Authentication,
    /// Agent, X11, port and tunnel forwarding.
    Forwarding,
    /// Reaching the host through another one.
    Proxy,
    /// Connection sharing.
    Multiplexing,
    /// Algorithms, canonicalization and the rest.
    Advanced,
}

impl KeyGroup {
    /// Every group, in form order.
    pub const ALL: [KeyGroup; 6] = [
        KeyGroup::Connection,
        KeyGroup::Authentication,
        KeyGroup::Forwarding,
        KeyGroup::Proxy,
        KeyGroup::Multiplexing,
        KeyGroup::Advanced,
    ];

    /// The heading the forms show.
    pub fn title(self) -> &'static str {
        match self {
            KeyGroup::Connection => "Connection",
            KeyGroup::Authentication => "Authentication",
            KeyGroup::Forwarding => "Forwarding",
            KeyGroup::Proxy => "Proxy",
            KeyGroup::Multiplexing => "Multiplexing",
            KeyGroup::Advanced => "Advanced",
        }
    }
}

/// The shape of a keyword's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    /// `yes` or `no`.
    Flag,
    /// One of `values`; with `open`, any other text is accepted too (a
    /// duration, a socket path, an environment variable).
    Choice {
        /// The documented values, in ssh_config(5) spelling.
        values: &'static [&'static str],
        /// Other text is valid as well.
        open: bool,
    },
    /// A TCP port, 1 to 65535.
    Port,
    /// A non-negative whole number.
    Number,
    /// A file or socket path; `~` and `%` tokens are kept as written.
    Path,
    /// A forwarding spec such as `8080 localhost:80`.
    Forward,
    /// Anything on one line.
    Text,
}

/// A keyword's form placement and value shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    /// The canonical ssh_config(5) spelling.
    pub key: &'static str,
    /// The form group.
    pub group: KeyGroup,
    /// The value shape.
    pub kind: KeyType,
    /// Every line counts (see [`crate::MULTI_VALUED_KEYS`]).
    pub multi: bool,
}

const YES_NO: &[&str] = &["yes", "no"];

/// Keywords a host block cannot carry.
const NOT_SETTABLE: &[&str] = &["Host", "Match", "Include"];

fn classify(key: &str) -> (KeyGroup, KeyType) {
    use KeyGroup::*;
    use KeyType::*;
    let choice = |values, open| Choice { values, open };
    match key {
        "HostName" | "User" | "BindAddress" | "BindInterface" | "RemoteCommand" | "EscapeChar"
        | "LocalCommand" | "SendEnv" | "SetEnv" | "IPQoS" | "RekeyLimit" | "ChannelTimeout"
        | "LogVerbose" | "Tag" => (Connection, Text),
        "Port" => (Connection, Port),
        "ConnectTimeout" | "ConnectionAttempts" | "ServerAliveInterval" | "ServerAliveCountMax" => {
            (Connection, Number)
        }
        "TCPKeepAlive"
        | "Compression"
        | "StdinNull"
        | "ForkAfterAuthentication"
        | "EnableEscapeCommandline"
        | "PermitLocalCommand"
        | "BatchMode"
        | "RefuseConnection" => (Connection, Flag),
        "AddressFamily" => (Connection, choice(&["any", "inet", "inet6"], false)),
        "RequestTTY" => (Connection, choice(&["no", "yes", "force", "auto"], false)),
        "SessionType" => (Connection, choice(&["none", "subsystem", "default"], false)),
        "ObscureKeystrokeTiming" => (Connection, choice(YES_NO, true)),
        "LogLevel" => (
            Connection,
            choice(
                &[
                    "QUIET", "FATAL", "ERROR", "INFO", "VERBOSE", "DEBUG", "DEBUG1", "DEBUG2",
                    "DEBUG3",
                ],
                false,
            ),
        ),
        "SyslogFacility" => (
            Connection,
            choice(
                &[
                    "DAEMON", "USER", "AUTH", "LOCAL0", "LOCAL1", "LOCAL2", "LOCAL3", "LOCAL4",
                    "LOCAL5", "LOCAL6", "LOCAL7",
                ],
                false,
            ),
        ),

        "IdentityFile"
        | "IdentityAgent"
        | "CertificateFile"
        | "PKCS11Provider"
        | "SecurityKeyProvider"
        | "UserKnownHostsFile"
        | "GlobalKnownHostsFile"
        | "RevokedHostKeys" => (Authentication, Path),
        "IdentitiesOnly"
        | "UseKeychain"
        | "PasswordAuthentication"
        | "KbdInteractiveAuthentication"
        | "ChallengeResponseAuthentication"
        | "GSSAPIAuthentication"
        | "GSSAPIDelegateCredentials"
        | "HostbasedAuthentication"
        | "HashKnownHosts"
        | "CheckHostIP"
        | "VisualHostKey"
        | "NoHostAuthenticationForLocalhost"
        | "EnableSSHKeysign" => (Authentication, Flag),
        "AddKeysToAgent" => (
            Authentication,
            choice(&["yes", "no", "ask", "confirm"], true),
        ),
        "PubkeyAuthentication" => (
            Authentication,
            choice(&["yes", "no", "unbound", "host-bound"], false),
        ),
        "StrictHostKeyChecking" => (
            Authentication,
            choice(&["yes", "no", "ask", "accept-new", "off"], false),
        ),
        "UpdateHostKeys" | "VerifyHostKeyDNS" => {
            (Authentication, choice(&["yes", "no", "ask"], false))
        }
        "FingerprintHash" => (Authentication, choice(&["md5", "sha256"], false)),
        "NumberOfPasswordPrompts" => (Authentication, Number),
        "PreferredAuthentications"
        | "KbdInteractiveDevices"
        | "KnownHostsCommand"
        | "HostKeyAlias" => (Authentication, Text),

        "ForwardAgent" => (Forwarding, choice(YES_NO, true)),
        "ForwardX11"
        | "ForwardX11Trusted"
        | "GatewayPorts"
        | "ExitOnForwardFailure"
        | "ClearAllForwardings"
        | "StreamLocalBindUnlink" => (Forwarding, Flag),
        "LocalForward" | "RemoteForward" | "DynamicForward" => (Forwarding, Forward),
        "XAuthLocation" => (Forwarding, Path),
        "Tunnel" => (
            Forwarding,
            choice(&["yes", "no", "point-to-point", "ethernet"], false),
        ),
        "ForwardX11Timeout" | "PermitRemoteOpen" | "StreamLocalBindMask" | "TunnelDevice" => {
            (Forwarding, Text)
        }

        "ProxyJump" | "ProxyCommand" => (Proxy, Text),
        "ProxyUseFdpass" => (Proxy, Flag),

        "ControlMaster" => (
            Multiplexing,
            choice(&["no", "yes", "ask", "auto", "autoask"], false),
        ),
        "ControlPath" => (Multiplexing, Path),
        "ControlPersist" => (Multiplexing, choice(YES_NO, true)),

        "CanonicalizeHostname" => (Advanced, choice(&["no", "yes", "always", "none"], false)),
        "CanonicalizeFallbackLocal" => (Advanced, Flag),
        "CanonicalizeMaxDots" | "RequiredRSASize" => (Advanced, Number),
        "WarnWeakCrypto" => (Advanced, choice(YES_NO, true)),
        _ => (Advanced, Text),
    }
}

/// The spec of `key` (matched case-insensitively), or `None` for an
/// unknown keyword and for `Host`, `Match` and `Include`.
///
/// ```
/// use rustorm_core::{key_spec, KeyGroup, KeyType};
/// let s = key_spec("forwardagent").unwrap();
/// assert_eq!(s.key, "ForwardAgent");
/// assert_eq!(s.group, KeyGroup::Forwarding);
/// assert_eq!(key_spec("Port").unwrap().kind, KeyType::Port);
/// assert!(key_spec("Match").is_none());
/// ```
pub fn key_spec(key: &str) -> Option<KeySpec> {
    let key = KNOWN_KEYS.iter().find(|k| k.eq_ignore_ascii_case(key))?;
    if NOT_SETTABLE.contains(key) {
        return None;
    }
    let (group, kind) = classify(key);
    Some(KeySpec {
        key,
        group,
        kind,
        multi: is_multi_valued(key),
    })
}

/// Every keyword a host can set, in [`KNOWN_KEYS`] order.
pub fn key_specs() -> Vec<KeySpec> {
    KNOWN_KEYS.iter().filter_map(|k| key_spec(k)).collect()
}

/// Checks `value` for `key`. An unknown keyword only needs a value on one
/// line. `Err` is the reason, without the key name.
///
/// ```
/// use rustorm_core::validate_setting;
/// assert!(validate_setting("Port", "2222").is_ok());
/// assert!(validate_setting("Port", "abc").is_err());
/// assert!(validate_setting("ForwardAgent", "$SSH_AUTH_SOCK").is_ok());
/// assert!(validate_setting("Compression", "maybe").is_err());
/// ```
pub fn validate_setting(key: &str, value: &str) -> std::result::Result<(), String> {
    let v = value.trim();
    if v.is_empty() {
        return Err("needs a value".into());
    }
    if v.contains('\n') || v.contains('\r') {
        return Err("must be on one line".into());
    }
    let Some(spec) = key_spec(key) else {
        return Ok(());
    };
    match spec.kind {
        KeyType::Flag => one_of(v, YES_NO),
        KeyType::Choice { values, open } => {
            if open {
                Ok(())
            } else {
                one_of(v, values)
            }
        }
        KeyType::Port => match v.parse::<u32>() {
            Ok(p) if (1..=65535).contains(&p) => Ok(()),
            _ => Err("must be a port from 1 to 65535".into()),
        },
        KeyType::Number => v
            .parse::<u64>()
            .map(|_| ())
            .map_err(|_| "must be a whole number".into()),
        KeyType::Forward => {
            let words = v.split_whitespace().count();
            let need = if spec.key == "LocalForward" { 2 } else { 1 };
            if words < need || words > 2 {
                Err(match need {
                    2 => "must be `[bind:]port host:hostport`".into(),
                    _ => "must be `[bind:]port [host:hostport]`".into(),
                })
            } else {
                Ok(())
            }
        }
        KeyType::Path | KeyType::Text => Ok(()),
    }
}

fn one_of(v: &str, values: &[&str]) -> std::result::Result<(), String> {
    if values.iter().any(|x| x.eq_ignore_ascii_case(v)) {
        Ok(())
    } else {
        Err(format!("must be one of {}", values.join(", ")))
    }
}

/// One keyword's new values in a settings change: empty removes every line
/// of the key; otherwise the values replace the key's lines, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChange {
    /// The keyword, any case; written in canonical case.
    pub key: String,
    /// The new values; empty unsets the key.
    pub values: Vec<String>,
}

impl SettingChange {
    /// Sets `key` to `value`.
    pub fn set(key: &str, value: &str) -> SettingChange {
        SettingChange {
            key: canonical_key(key),
            values: vec![value.to_string()],
        }
    }

    /// Removes `key`.
    pub fn unset(key: &str) -> SettingChange {
        SettingChange {
            key: canonical_key(key),
            values: Vec::new(),
        }
    }
}

/// One value line of a settings form. A multi-valued key has one row per
/// value plus one empty row to add another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRow {
    /// The keyword.
    pub spec: KeySpec,
    /// The value as edited; empty means the key is not set.
    pub value: String,
    /// The value as loaded.
    pub original: String,
    /// What `Host *` gives the host for this key, shown while unset.
    pub inherited: Option<String>,
}

impl SettingRow {
    /// True when the value differs from the loaded one.
    pub fn changed(&self) -> bool {
        self.value.trim() != self.original.trim()
    }

    /// The words a flag or choice offers, in order; `None` for typed kinds.
    pub fn choices(&self) -> Option<&'static [&'static str]> {
        match self.spec.kind {
            KeyType::Flag => Some(YES_NO),
            KeyType::Choice { values, .. } => Some(values),
            _ => None,
        }
    }

    /// True when the value is typed; a flag or a closed choice is only
    /// picked.
    pub fn typed(&self) -> bool {
        !matches!(
            self.spec.kind,
            KeyType::Flag | KeyType::Choice { open: false, .. }
        )
    }

    /// Why the value does not fit its keyword; `None` when it does or is
    /// empty.
    pub fn problem(&self) -> Option<String> {
        let v = self.value.trim();
        if v.is_empty() {
            return None;
        }
        validate_setting(self.spec.key, v).err()
    }
}

/// Every keyword a host can set, as rows grouped in [`KeyGroup::ALL`]
/// order, with the changes to write once edited. The TUI and GUI settings
/// forms edit one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsDraft {
    /// The host's primary name.
    pub host: String,
    /// The rows.
    pub rows: Vec<SettingRow>,
}

impl SettingsDraft {
    /// The rows for `block`, with `defaults` (the `Host *` block) giving the
    /// inherited values.
    pub fn new(
        host: &str,
        block: &crate::HostBlock,
        defaults: Option<&crate::HostBlock>,
    ) -> SettingsDraft {
        let mut rows = Vec::new();
        for group in KeyGroup::ALL {
            for spec in key_specs().into_iter().filter(|s| s.group == group) {
                let inherited = defaults.and_then(|d| d.get(spec.key));
                let row = |v: String| SettingRow {
                    spec,
                    value: v.clone(),
                    original: v,
                    inherited: inherited.clone(),
                };
                let values = block.get_all(spec.key);
                if spec.multi {
                    rows.extend(values.into_iter().map(row));
                    rows.push(row(String::new()));
                } else {
                    rows.push(row(values.into_iter().next().unwrap_or_default()));
                }
            }
        }
        SettingsDraft {
            host: host.to_string(),
            rows,
        }
    }

    /// The changes to write, one per keyword whose values differ from the
    /// loaded ones; `Err` names the first value that does not fit its key.
    pub fn changes(&self) -> std::result::Result<Vec<SettingChange>, String> {
        let mut keys: Vec<&'static str> = Vec::new();
        for r in &self.rows {
            if !keys.contains(&r.spec.key) {
                keys.push(r.spec.key);
            }
        }
        let mut out = Vec::new();
        for key in keys {
            let rows = self.rows.iter().filter(|r| r.spec.key == key);
            let values: Vec<String> = rows
                .clone()
                .map(|r| r.value.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect();
            let original: Vec<String> = rows
                .map(|r| r.original.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect();
            if values == original {
                continue;
            }
            for v in &values {
                validate_setting(key, v).map_err(|reason| format!("{key} {reason}."))?;
            }
            out.push(SettingChange {
                key: key.to_string(),
                values,
            });
        }
        Ok(out)
    }

    /// Once the last row of a multi-valued key holds a value, adds an empty
    /// row after it, so another value can always be added. `i` is any row
    /// of the key.
    pub fn grow(&mut self, i: usize) {
        let spec = self.rows[i].spec;
        if !spec.multi {
            return;
        }
        let Some(last) = self.rows.iter().rposition(|r| r.spec.key == spec.key) else {
            return;
        };
        if !self.rows[last].value.trim().is_empty() {
            let mut blank = self.rows[last].clone();
            blank.value.clear();
            blank.original.clear();
            self.rows.insert(last + 1, blank);
        }
    }
}
