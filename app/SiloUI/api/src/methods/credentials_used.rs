//! `computers.credentials`: which Silo-managed secrets a computer used.
//!
//! Names, hosts, methods and counts only; the API never carries a secret value or a placeholder.

wire_struct! {
    /// `computers.credentials` parameters. `since` (epoch milliseconds) drops older events.
    ComputerCredentialsParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<u64>,
    }
}

wire_struct! {
    /// A secret that was substituted for requests to one host with one method.
    CredentialUse {
        secret: String,
        host: String,
        method: String,
        count: u64,
        /// Epoch milliseconds of the latest request.
        last_at: u64,
    }
}

wire_struct! {
    /// A placeholder sent to a host its secret is not allowed for.
    CredentialBlocked {
        /// The secret's name when the proxy named it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret: Option<String>,
        host: String,
        method: String,
        count: u64,
        reason: String,
        /// Epoch milliseconds of the latest attempt.
        last_at: u64,
    }
}

wire_struct! {
    /// Credentials a computer used, and attempts that were blocked.
    CredentialsUsage {
        used: Vec<CredentialUse>,
        blocked: Vec<CredentialBlocked>,
    }
}
