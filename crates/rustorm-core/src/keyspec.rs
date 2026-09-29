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

/// Algorithm lists: long, rarely set by hand, and sharing prefixes with
/// everyday keywords, so completion offers them only when typed in full.
const ALGORITHM_LISTS: &[&str] = &[
    "CASignatureAlgorithms",
    "Ciphers",
    "HostKeyAlgorithms",
    "HostbasedAcceptedAlgorithms",
    "HostbasedAcceptedKeyTypes",
    "KexAlgorithms",
    "MACs",
    "PubkeyAcceptedAlgorithms",
    "PubkeyAcceptedKeyTypes",
];

/// The keywords of `candidates` that match `typed` (ignoring case and
/// surrounding space), in `candidates` order: those starting with it, or
/// when none does, those containing it. Empty text matches nothing.
fn complete_from(candidates: &[&'static str], typed: &str) -> Vec<&'static str> {
    let t = typed.trim().to_ascii_lowercase();
    if t.is_empty() {
        return Vec::new();
    }
    let offered: Vec<&'static str> = candidates
        .iter()
        .filter(|k| !ALGORITHM_LISTS.contains(k) || k.eq_ignore_ascii_case(&t))
        .copied()
        .collect();
    let prefix: Vec<&'static str> = offered
        .iter()
        .filter(|k| k.to_ascii_lowercase().starts_with(&t))
        .copied()
        .collect();
    if !prefix.is_empty() {
        return prefix;
    }
    offered
        .into_iter()
        .filter(|k| k.to_ascii_lowercase().contains(&t))
        .collect()
}

/// The keywords a host can set that match `typed`, best first: those
/// starting with it (ignoring case) in [`KNOWN_KEYS`] order, or when none
/// does, those containing it. Algorithm-list keywords (`Ciphers`,
/// `HostKeyAlgorithms` and the like) are offered only when typed in full.
///
/// ```
/// use rustorm_core::complete_setting;
/// assert_eq!(complete_setting("hostk"), ["HostKeyAlias"]);
/// assert_eq!(complete_setting("forward")[0], "ForwardAgent");
/// assert_eq!(complete_setting("keyal"), ["HostKeyAlias"]);
/// assert!(complete_setting("zzz").is_empty());
/// ```
pub fn complete_setting(typed: &str) -> Vec<&'static str> {
    let keys: Vec<&'static str> = key_specs().iter().map(|s| s.key).collect();
    complete_from(&keys, typed)
}

/// The value a keyword is usually set to, which completing or adding the
/// keyword fills in ready to overwrite: `yes` for a flag, the first
/// documented value of a choice, and a common value for a few others.
/// `None` for free text such as `HostKeyAlias` or `ProxyCommand`.
///
/// ```
/// use rustorm_core::premade_value;
/// assert_eq!(premade_value("port"), Some("22"));
/// assert_eq!(premade_value("StrictHostKeyChecking"), Some("yes"));
/// assert_eq!(premade_value("HostKeyAlias"), None);
/// ```
pub fn premade_value(key: &str) -> Option<&'static str> {
    let spec = key_spec(key)?;
    match spec.key {
        "Port" => return Some("22"),
        "ServerAliveInterval" => return Some("60"),
        "ConnectTimeout" => return Some("10"),
        "ControlPersist" => return Some("10m"),
        "ControlPath" => return Some("~/.ssh/cm-%r@%h:%p"),
        "IdentityFile" => return Some("~/.ssh/id_ed25519"),
        "LocalForward" => return Some("8080 localhost:80"),
        _ => {}
    }
    match spec.kind {
        KeyType::Flag => Some("yes"),
        KeyType::Choice { values, .. } => values.first().copied(),
        _ => None,
    }
}

/// The value after `value` (any case) among the words of a flag or choice
/// keyword, wrapping around: `yes` and `no` swap. `None` for other kinds
/// and for a value that is not one of the words.
///
/// ```
/// use rustorm_core::next_choice;
/// assert_eq!(next_choice("Compression", "yes"), Some("no"));
/// assert_eq!(next_choice("StrictHostKeyChecking", "ask"), Some("accept-new"));
/// assert_eq!(next_choice("User", "travis"), None);
/// ```
pub fn next_choice(key: &str, value: &str) -> Option<&'static str> {
    let words = match key_spec(key)?.kind {
        KeyType::Flag => YES_NO,
        KeyType::Choice { values, .. } => values,
        _ => return None,
    };
    let at = words
        .iter()
        .position(|w| w.eq_ignore_ascii_case(value.trim()))?;
    Some(words[(at + 1) % words.len()])
}

/// Keywords that start a block; completed only at column 0.
const BLOCK_KEYS: &[&str] = &["Host", "Match", "Include"];

/// A keyword suggestion for the first word of a line being typed. Columns
/// and ranges count characters, as editor cursors do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineCompletion {
    /// The keyword in canonical spelling.
    pub keyword: &'static str,
    /// The typed word, which accepting replaces.
    pub word: std::ops::Range<usize>,
    /// The value accepting inserts after the keyword (see
    /// [`premade_value`]).
    pub premade: Option<&'static str>,
}

/// A line after accepting a [`LineCompletion`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    /// The new line.
    pub line: String,
    /// The cursor column: after the premade value, else after the space
    /// that follows the keyword.
    pub cursor: usize,
    /// The premade value's columns, to select so typing replaces it.
    pub select: Option<std::ops::Range<usize>>,
}

impl LineCompletion {
    /// The hint to show dimmed after the cursor: the rest of the keyword
    /// when the typed word starts it, else ` → Keyword`.
    pub fn ghost(&self, line: &str) -> String {
        let typed: String = line
            .chars()
            .skip(self.word.start)
            .take(self.word.len())
            .collect();
        let lower = self.keyword.to_ascii_lowercase();
        if lower.starts_with(&typed.to_ascii_lowercase()) {
            self.keyword.chars().skip(self.word.len()).collect()
        } else {
            format!(" → {}", self.keyword)
        }
    }

    /// `line` with the typed word replaced by the keyword, a space and the
    /// premade value. Text after the word is kept as it is.
    pub fn accept(&self, line: &str) -> Accepted {
        let chars: Vec<char> = line.chars().collect();
        let mut out: String = chars[..self.word.start].iter().collect();
        out.push_str(self.keyword);
        out.push(' ');
        let start = out.chars().count();
        let select = self.premade.map(|v| {
            out.push_str(v);
            start..start + v.chars().count()
        });
        let cursor = out.chars().count();
        out.extend(&chars[self.word.end..]);
        Accepted {
            line: out,
            cursor,
            select,
        }
    }
}

/// The keyword suggestion for `line` with the cursor at column `col`, when
/// the cursor ends the line's first word: settable keywords on an indented
/// line, `Host`, `Match` and `Include` at column 0, matched as
/// [`complete_setting`] does. `None` in a comment, a value, the middle of a
/// word, or when nothing matches.
///
/// ```
/// use rustorm_core::complete_line;
/// let c = complete_line("    por", 7).unwrap();
/// assert_eq!(c.keyword, "Port");
/// assert_eq!(c.accept("    por").line, "    Port 22");
/// assert_eq!(complete_line("ho", 2).unwrap().keyword, "Host");
/// assert!(complete_line("    User tra", 12).is_none());
/// ```
pub fn complete_line(line: &str, col: usize) -> Option<LineCompletion> {
    let chars: Vec<char> = line.chars().collect();
    let indent = chars.iter().take_while(|c| c.is_whitespace()).count();
    if col <= indent || col > chars.len() {
        return None;
    }
    let word = &chars[indent..col];
    if word[0] == '#' || word.iter().any(|c| c.is_whitespace() || *c == '=') {
        return None;
    }
    if chars
        .get(col)
        .is_some_and(|c| !c.is_whitespace() && *c != '=')
    {
        return None;
    }
    let typed: String = word.iter().collect();
    let keyword = if indent == 0 {
        complete_from(BLOCK_KEYS, &typed)
    } else {
        complete_setting(&typed)
    }
    .into_iter()
    .next()?;
    Some(LineCompletion {
        keyword,
        word: indent..col,
        premade: premade_value(keyword),
    })
}

/// The swap for the value under column `col` of `line` (`Key value` or
/// `Key=value`): its columns and the next word (see [`next_choice`]).
/// `None` off the value, or when the value does not cycle.
///
/// ```
/// use rustorm_core::swap_value;
/// assert_eq!(swap_value("    Compression yes", 19), Some((16..19, "no")));
/// assert_eq!(swap_value("    User travis", 12), None);
/// ```
pub fn swap_value(line: &str, col: usize) -> Option<(std::ops::Range<usize>, &'static str)> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = chars.iter().take_while(|c| c.is_whitespace()).count();
    let key_start = i;
    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' {
        i += 1;
    }
    let key: String = chars[key_start..i].iter().collect();
    while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '=') {
        i += 1;
    }
    let start = i;
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    let end = i;
    if start == end
        || !(start..=end).contains(&col)
        || chars[end..].iter().any(|c| !c.is_whitespace())
    {
        return None;
    }
    let value: String = chars[start..end].iter().collect();
    next_choice(&key, &value).map(|next| (start..end, next))
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
    /// Added through [`SettingsDraft::add_key`] since the draft was made.
    pub added: bool,
}

impl SettingRow {
    /// True when the value differs from the loaded one.
    pub fn changed(&self) -> bool {
        self.value.trim() != self.original.trim()
    }

    /// True when the row shows in the filled view: it has a loaded or
    /// edited value, or was added this session. A loaded value the user
    /// clears stays filled until the draft is dropped.
    pub fn filled(&self) -> bool {
        self.added || !self.original.trim().is_empty() || !self.value.trim().is_empty()
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
                    added: false,
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

    /// Indices of the rows the filled view shows, in form order (see
    /// [`SettingRow::filled`]); the all view shows every row.
    pub fn filled_rows(&self) -> Vec<usize> {
        (0..self.rows.len())
            .filter(|&i| self.rows[i].filled())
            .collect()
    }

    /// The keywords offered for `typed` text in an Add setting field: see
    /// [`complete_setting`].
    pub fn complete(&self, typed: &str) -> Vec<&'static str> {
        complete_setting(typed)
    }

    /// Adds `key` (any case) to the filled view and returns the row to
    /// focus: the existing row of a single-value key the host sets, else an
    /// empty row marked added (for a repeatable key, a row after its
    /// values). `None` for a keyword a host cannot set. The value is left
    /// as it is; a form may prefill [`premade_value`].
    pub fn add_key(&mut self, key: &str) -> Option<usize> {
        let spec = key_spec(key)?;
        let first = self.rows.iter().position(|r| r.spec.key == spec.key)?;
        if !spec.multi {
            if !self.rows[first].filled() {
                self.rows[first].added = true;
            }
            return Some(first);
        }
        let mut last = self.rows.iter().rposition(|r| r.spec.key == spec.key)?;
        if !self.rows[last].value.trim().is_empty() {
            self.grow(last);
            last += 1;
        }
        self.rows[last].added = true;
        Some(last)
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
            blank.added = false;
            self.rows.insert(last + 1, blank);
        }
    }
}
