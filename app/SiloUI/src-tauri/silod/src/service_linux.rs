//! `silod --service` on Linux. The systemd user unit that replaces the LaunchAgent is roadmap
//! milestone 6, phase 3; until then the agent is started by hand (`silod --agent`).

pub fn run_command(_args: &[String]) -> Result<i32, String> {
    Err("silod --service is not available on Linux yet; start the engine with silod --agent".into())
}
