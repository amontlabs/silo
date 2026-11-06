//! The in-guest client: a scoped credential a computer's resident agent uses to fork and ask
//! for review from inside, and the frames it sends through the computer's bridge folder.
//! See `docs/SiloUI-IN-GUEST-CLIENT.md`.

/// Where the guest sees the bridge folder (the runtime's `/.msb` share).
pub const GUEST_SPOOL_DIR: &str = "/.msb/silo-bridge";
/// Where the token file is installed in the guest.
pub const GUEST_TOKEN_PATH: &str = "/etc/silo/guest-client.json";
/// The most a bridge request or response may hold.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

wire_struct! {
    /// What the project settings allow the agent, resolved on the host when the token is
    /// minted (the engine has no project folder of its own). The in-guest client never keeps,
    /// so `fold` and `auto_keep` are not part of it.
    GuestClientPolicy {
        /// Secret names (`github` included) a fork made from inside may name. Empty by default.
        #[serde(default)]
        fork_secrets: Vec<String>,
        /// The agent name the settings were resolved for.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
    }
}

wire_struct! {
    GuestClientEnableParams {
        computer_id: String,
        policy: GuestClientPolicy,
    }
}

wire_struct! {
    /// The token is shown here once; the engine keeps only its hash.
    GuestClientEnableResult {
        computer_id: String,
        token: String,
        token_path: String,
        spool_dir: String,
        minted_at: String,
    }
}

wire_struct! {
    GuestClientComputerParams {
        computer_id: String,
    }
}

wire_struct! {
    GuestClientStatus {
        computer_id: String,
        enabled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minted_at: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        policy: Option<GuestClientPolicy>,
    }
}

wire_struct! {
    /// One request file the guest client writes into the bridge folder.
    GuestRequestFrame {
        v: u32,
        id: String,
        token: String,
        method: String,
        #[serde(default)]
        params: serde_json::Value,
    }
}

wire_struct! {
    GuestResponseFrame {
        v: u32,
        id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<GuestErrorBody>,
    }
}

wire_struct! {
    GuestErrorBody {
        code: String,
        message: String,
    }
}
