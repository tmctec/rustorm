//! ssh_config(5) keywords: canonical spelling, multi-valued keys, and the
//! known-keyword list that `check` validates against.

/// Every keyword ssh_config(5) documents, in canonical case, plus `UseKeychain`
/// (macOS) and the deprecated spellings OpenSSH still accepts.
pub const KNOWN_KEYS: &[&str] = &[
    "AddKeysToAgent",
    "AddressFamily",
    "BatchMode",
    "BindAddress",
    "BindInterface",
    "CanonicalDomains",
    "CanonicalizeFallbackLocal",
    "CanonicalizeHostname",
    "CanonicalizeMaxDots",
    "CanonicalizePermittedCNAMEs",
    "CASignatureAlgorithms",
    "CertificateFile",
    "ChallengeResponseAuthentication",
    "ChannelTimeout",
    "CheckHostIP",
    "Ciphers",
    "ClearAllForwardings",
    "Compression",
    "ConnectionAttempts",
    "ConnectTimeout",
    "ControlMaster",
    "ControlPath",
    "ControlPersist",
    "DynamicForward",
    "EnableEscapeCommandline",
    "EnableSSHKeysign",
    "EscapeChar",
    "ExitOnForwardFailure",
    "FingerprintHash",
    "ForkAfterAuthentication",
    "ForwardAgent",
    "ForwardX11",
    "ForwardX11Timeout",
    "ForwardX11Trusted",
    "GatewayPorts",
    "GlobalKnownHostsFile",
    "GSSAPIAuthentication",
    "GSSAPIDelegateCredentials",
    "HashKnownHosts",
    "Host",
    "HostbasedAcceptedAlgorithms",
    "HostbasedAcceptedKeyTypes",
    "HostbasedAuthentication",
    "HostKeyAlgorithms",
    "HostKeyAlias",
    "HostName",
    "IdentitiesOnly",
    "IdentityAgent",
    "IdentityFile",
    "IgnoreUnknown",
    "Include",
    "IPQoS",
    "KbdInteractiveAuthentication",
    "KbdInteractiveDevices",
    "KexAlgorithms",
    "KnownHostsCommand",
    "LocalCommand",
    "LocalForward",
    "LogLevel",
    "LogVerbose",
    "MACs",
    "Match",
    "NoHostAuthenticationForLocalhost",
    "NumberOfPasswordPrompts",
    "ObscureKeystrokeTiming",
    "PasswordAuthentication",
    "PermitLocalCommand",
    "PermitRemoteOpen",
    "PKCS11Provider",
    "Port",
    "PreferredAuthentications",
    "ProxyCommand",
    "ProxyJump",
    "ProxyUseFdpass",
    "PubkeyAcceptedAlgorithms",
    "PubkeyAcceptedKeyTypes",
    "PubkeyAuthentication",
    "RefuseConnection",
    "RekeyLimit",
    "RemoteCommand",
    "RemoteForward",
    "RequestTTY",
    "RequiredRSASize",
    "RevokedHostKeys",
    "SecurityKeyProvider",
    "SendEnv",
    "ServerAliveCountMax",
    "ServerAliveInterval",
    "SessionType",
    "SetEnv",
    "StdinNull",
    "StreamLocalBindMask",
    "StreamLocalBindUnlink",
    "StrictHostKeyChecking",
    "SyslogFacility",
    "Tag",
    "TCPKeepAlive",
    "Tunnel",
    "TunnelDevice",
    "UpdateHostKeys",
    "UseKeychain",
    "User",
    "UserKnownHostsFile",
    "VerifyHostKeyDNS",
    "VisualHostKey",
    "WarnWeakCrypto",
    "XAuthLocation",
];

/// Keys that accumulate: every line counts, instead of the first one winning.
pub const MULTI_VALUED_KEYS: &[&str] = &[
    "IdentityFile",
    "LocalForward",
    "RemoteForward",
    "DynamicForward",
    "CertificateFile",
    "SendEnv",
    "SetEnv",
];

/// Returns the canonical ssh_config(5) spelling of `key` (matched
/// case-insensitively), or `key` unchanged when it is not a known keyword.
///
/// ```
/// assert_eq!(rustorm_core::canonical_key("hostname"), "HostName");
/// assert_eq!(rustorm_core::canonical_key("MyKey"), "MyKey");
/// ```
pub fn canonical_key(key: &str) -> String {
    KNOWN_KEYS
        .iter()
        .find(|k| k.eq_ignore_ascii_case(key))
        .map_or_else(|| key.to_string(), |k| (*k).to_string())
}

/// True when `key` names an ssh_config(5) keyword, ignoring case.
pub fn is_known_key(key: &str) -> bool {
    KNOWN_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

/// True when `key` is multi-valued (see [`MULTI_VALUED_KEYS`]), ignoring case.
pub fn is_multi_valued(key: &str) -> bool {
    MULTI_VALUED_KEYS
        .iter()
        .any(|k| k.eq_ignore_ascii_case(key))
}
