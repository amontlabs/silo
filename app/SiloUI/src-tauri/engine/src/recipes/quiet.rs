//! What a fork of a computer built from a recipe stops: the recipe's `quiet` entries, and for a
//! service among them the name its process runs under.

use silo_api::methods::recipes::RecipeDefinition;

fn service_command<'a>(recipe: &'a RecipeDefinition, name: &str) -> Option<&'a str> {
    recipe
        .spec
        .services
        .iter()
        .find(|service| service.name == name)
        .map(|service| service.run.as_str())
}

/// The recipe's `quiet:` list as the guest matches it. The guest stops processes by program
/// or script name, and a service's launcher `exec`s its command, so the running process of a
/// service never carries the service's name: an entry naming one of the recipe's `services:`
/// is kept (its launcher and autostart entry go by that name) and followed by the name its
/// process runs under.
pub fn quiet_names(recipe: &RecipeDefinition) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for entry in &recipe.quiet {
        let program = service_command(recipe, entry).and_then(service_program);
        for name in std::iter::once(entry.clone()).chain(program) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// One line per quiet service whose running process name cannot be derived: only the service's
/// own name is stopped for it, which its process does not carry.
pub fn quiet_warnings(recipe: &RecipeDefinition) -> Vec<String> {
    recipe
        .quiet
        .iter()
        .filter_map(|entry| Some((entry, service_command(recipe, entry)?)))
        .filter(|(_, command)| service_program(command).is_none())
        .map(|(entry, _)| {
            format!(
                "quiet: cannot tell which process service {entry} runs; name its program or script in quiet too."
            )
        })
        .collect()
}

/// Programs that run some other program; their name says nothing about the service.
const LAUNCHERS: [&str; 18] = [
    "env", "sudo", "nohup", "nice", "exec", "timeout", "java", "npm", "npx", "yarn", "pnpm", "uv",
    "docker", "make", "bun", "deno", "bash", "sh",
];

/// Interpreters whose process name is matched by the script they run.
const INTERPRETERS: [&str; 6] = ["python", "node", "nodejs", "perl", "ruby", "php"];

/// Interpreter options that run code or a module instead of a script file.
const NO_SCRIPT: [&str; 8] = [
    "-m",
    "-c",
    "-e",
    "-p",
    "-r",
    "--eval",
    "--print",
    "--require",
];

/// The program or script name a service command runs under, as the guest's quiet step matches
/// it: the script after an interpreter, by its full file name, when the script is a specific
/// file (`python3 -u /opt/app/poller.py` is `poller.py`; `node server` says too little), else
/// the program (`openclaw gateway` is `openclaw`). `None` for a launcher (`npm start`,
/// `sudo x`, `bash run.sh`) and when the command is not a plain command line the launcher
/// `exec`s.
fn service_program(command: &str) -> Option<String> {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_./:=@%+, ".contains(c);
    if !command.chars().all(plain) {
        return None;
    }
    let mut words = command.split_whitespace();
    let first = words.next().filter(|word| !word.contains('='))?;
    let base = |word: &str| word.rsplit('/').next().unwrap_or(word).to_owned();
    let program = base(first);
    let family = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    if LAUNCHERS.contains(&family) {
        return None;
    }
    let name = if INTERPRETERS.contains(&family) {
        let mut options = Vec::new();
        let script = words.find(|word| {
            options.push(*word);
            !word.starts_with('-')
        })?;
        if options.iter().any(|option| NO_SCRIPT.contains(option)) {
            return None;
        }
        let name = base(script);
        let specific = name
            .split_once('.')
            .is_some_and(|(stem, extension)| !stem.is_empty() && !extension.is_empty());
        if !specific {
            return None;
        }
        name
    } else {
        program
    };
    let valid = |c: char| c.is_ascii_alphanumeric() || "._@:+-".contains(c);
    let usable = !name.is_empty() && name != "." && name != ".." && !name.starts_with('-');
    (usable && name.chars().all(valid)).then_some(name)
}

#[cfg(test)]
mod tests {
    use silo_api::methods::recipes::{RecipeOs, RecipePackages, RecipeService, RecipeSpec};

    use super::*;

    fn recipe(services: &[(&str, &str)], quiet: &[&str]) -> RecipeDefinition {
        RecipeDefinition {
            spec: RecipeSpec {
                os: RecipeOs::Linux,
                cpus: None,
                memory_gib: None,
                disk_gib: None,
                packages: RecipePackages::default(),
                setup: Vec::new(),
                services: services
                    .iter()
                    .map(|(name, run)| RecipeService {
                        name: (*name).into(),
                        run: (*run).into(),
                    })
                    .collect(),
                ports: Vec::new(),
                desktop: None,
            },
            quiet: quiet.iter().map(|name| (*name).into()).collect(),
            scrub: Vec::new(),
            keep: None,
            test: None,
            files: Vec::new(),
            commands_hidden: None,
        }
    }

    #[test]
    fn a_quiet_service_also_stops_the_program_it_runs() {
        let recipe = recipe(
            &[
                ("final-poller", "python3 -u /opt/final/poller.py"),
                ("gateway", "openclaw gateway"),
                ("shell", "cd /srv && ./run"),
            ],
            &["final-poller", "gateway", "openclaw", "shell", "cron"],
        );
        assert_eq!(
            quiet_names(&recipe),
            [
                "final-poller",
                "poller.py",
                "gateway",
                "openclaw",
                "shell",
                "cron"
            ]
        );
        assert_eq!(
            service_program("node server.js").as_deref(),
            Some("server.js")
        );
        assert_eq!(
            service_program("/usr/bin/redis-server --port 6380").as_deref(),
            Some("redis-server")
        );
        assert_eq!(service_program("PORT=1 node app.js"), None);
        assert_eq!(service_program("python3"), None);
    }

    #[test]
    fn launchers_and_unspecific_scripts_name_no_process_and_warn() {
        for command in [
            "env FOO=1 poller",
            "sudo -u bot poller",
            "nohup poller",
            "nice -n 5 poller",
            "exec poller",
            "timeout 60 poller",
            "java -jar /opt/app.jar",
            "npm start",
            "npx serve",
            "yarn dev",
            "pnpm run bot",
            "uv run bot.py",
            "docker run app",
            "make serve",
            "bun run index.ts",
            "deno run main.ts",
            "bash run.sh",
            "/bin/sh -c poller",
            "node server",
            "python3 -m http.server",
            "python3 -c pass",
            "node -e 1",
            "python3 -u /opt/app/.env",
        ] {
            assert_eq!(service_program(command), None, "{command}");
        }
        assert_eq!(
            service_program("python3.12 -u /opt/app/poller.py").as_deref(),
            Some("poller.py")
        );
        assert_eq!(
            service_program("node /srv/bot/index.mjs").as_deref(),
            Some("index.mjs")
        );
        let recipe = recipe(
            &[("web", "npm start"), ("poller", "python3 /opt/poller.py")],
            &["web", "poller", "cron"],
        );
        assert_eq!(quiet_names(&recipe), ["web", "poller", "poller.py", "cron"]);
        let warnings = quiet_warnings(&recipe);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("service web"), "{}", warnings[0]);
    }
}
