//! The guest commands a build runs, in order. Package names and service commands reach the
//! scripts as `A_*` environment variables, never as shell text; package names were checked by
//! [`super::validate`] to be single words, so an unquoted `$A_PKGS` splits into exactly them.

use silo_api::methods::recipes::{RecipeOs, RecipeSpec};

use super::build::RunAs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// What the step does, in the words of the progress line and the failure message.
    pub label: String,
    pub who: RunAs,
    pub script: String,
    pub args: Vec<(String, String)>,
    /// Checks a result instead of making one.
    pub verify: bool,
}

const LINUX_PRELUDE: &str = "set -eu\nexport DEBIAN_FRONTEND=noninteractive\n";
/// Fresh macOS computers have no Homebrew. `ensure_brew` installs it as the account with the
/// official installer, non-interactively, and is called before the first `brew install`.
const MACOS_PRELUDE: &str = r#"set -eu
BREW=$(command -v brew || echo /opt/homebrew/bin/brew)
[ -x "$BREW" ] && eval "$($BREW shellenv)"
ensure_brew() {
  [ -x "$BREW" ] && return 0
  NONINTERACTIVE=1 /bin/bash -c "$(/usr/bin/curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  BREW=/opt/homebrew/bin/brew
  eval "$($BREW shellenv)"
}
"#;

/// Where pipx keeps its environments and links on Linux, set for the install and the check.
const LINUX_PIPX_ENV: &str = "export PIPX_HOME=/opt/pipx PIPX_BIN_DIR=/usr/local/bin\n";

/// Whether a service command is one plain program with arguments, which the wrapper can
/// `exec` so the service runs under its own name. Anything with shell syntax runs as written.
const SERVICE_LINE: &str = r#"case "${A_RUN%% *}" in
  *=*) line=$A_RUN;;
  *) case "$A_RUN" in
    *[!-A-Za-z0-9_./:=@%+,\ ]*) line=$A_RUN;;
    *) line="exec $A_RUN";;
  esac;;
esac
"#;

/// The installed package name of a spec such as `name@1.2`, `@scope/name@1` or `name==1.0`.
const BASE_NAME: &str = "base() { case \"$1\" in @*/*@*) echo \"${1%@*}\";; @*) echo \"$1\";; *@*) echo \"${1%@*}\";; *) echo \"$1\" | sed 's/[=<>~!^].*//';; esac; }\n";

fn step(label: impl Into<String>, who: RunAs, script: String, args: &[(&str, &str)]) -> Step {
    Step {
        label: label.into(),
        who,
        script,
        args: args
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        verify: false,
    }
}

fn verify_step(mut step: Step) -> Step {
    step.verify = true;
    step
}

fn words(list: &[String]) -> String {
    list.join(" ")
}

pub fn install_steps(spec: &RecipeSpec) -> Vec<Step> {
    let linux = spec.os == RecipeOs::Linux;
    let (who, prelude) = if linux {
        (RunAs::Root, LINUX_PRELUDE)
    } else {
        (RunAs::Account, MACOS_PRELUDE)
    };
    let p = &spec.packages;
    let mut steps = Vec::new();
    let mut add = |manager: &str, list: &[String], body: &str| {
        if !list.is_empty() {
            steps.push(step(
                format!("Installing {manager} packages"),
                who,
                format!("{prelude}{body}"),
                &[("PKGS", &words(list))],
            ));
        }
    };
    add(
        "apt",
        &p.apt,
        "apt-get update -qq\napt-get install -y -qq --no-install-recommends $A_PKGS\n",
    );
    add("brew", &p.brew, "ensure_brew\nbrew install $A_PKGS\n");
    let (ensure_pip, pip) = if linux {
        (
            "command -v pip3 >/dev/null || { apt-get update -qq; apt-get install -y -qq python3-pip; }\n",
            "pip3 install --break-system-packages $A_PKGS\n",
        )
    } else {
        (
            "command -v python3 >/dev/null || { ensure_brew; brew install python; }\n",
            "python3 -m pip install --user --break-system-packages $A_PKGS\n",
        )
    };
    add("pip", &p.pip, &format!("{ensure_pip}{pip}"));
    let pipx = if linux {
        format!("command -v pipx >/dev/null || {{ apt-get update -qq; apt-get install -y -qq pipx; }}\n{LINUX_PIPX_ENV}for package in $A_PKGS; do pipx install \"$package\"; done\n")
    } else {
        "command -v pipx >/dev/null || { ensure_brew; brew install pipx; }\nfor package in $A_PKGS; do pipx install \"$package\"; done\n".to_owned()
    };
    add("pipx", &p.pipx, &pipx);
    let npm = if linux {
        "command -v npm >/dev/null || { apt-get update -qq; apt-get install -y -qq nodejs npm; }\nnpm install -g $A_PKGS\n"
    } else {
        "command -v npm >/dev/null || { ensure_brew; brew install node; }\nnpm install -g $A_PKGS\n"
    };
    add("npm", &p.npm, npm);
    let cargo = if linux {
        "command -v cargo >/dev/null || { apt-get update -qq; apt-get install -y -qq cargo; }\ncargo install --root /usr/local $A_PKGS\n"
    } else {
        "command -v cargo >/dev/null || { ensure_brew; brew install rust; }\ncargo install $A_PKGS\n"
    };
    add("cargo", &p.cargo, cargo);
    steps
}

pub fn setup_steps(spec: &RecipeSpec) -> Vec<Step> {
    let (who, prelude) = if spec.os == RecipeOs::Linux {
        (RunAs::Root, LINUX_PRELUDE)
    } else {
        (RunAs::Account, MACOS_PRELUDE)
    };
    spec.setup
        .iter()
        .enumerate()
        .map(|(index, command)| {
            step(
                format!("Running setup command {} ({})", index + 1, shorten(command)),
                who,
                format!("{prelude}bash -c \"$A_CMD\"\n"),
                &[("CMD", command)],
            )
        })
        .collect()
}

/// Where a Linux service's launcher lives. The launcher is named after the service, as the
/// autostart entry is (`/etc/xdg/autostart/<name>.desktop`), so a `quiet:` entry naming the
/// service finds both, and the launcher `exec`s the command so the running process carries
/// the command's own name.
const LINUX_LAUNCHERS: &str = "/usr/local/libexec/silo-services";
/// Where a macOS service's launcher lives, under the account's home.
const MACOS_LAUNCHERS: &str = ".local/libexec/silo-services";

fn linux_service_script() -> String {
    format!(
        "set -eu\nroot=${{A_ROOT:-}}\ninstall -d -m 0755 \"$root{LINUX_LAUNCHERS}\" \"$root/etc/xdg/autostart\"\n{SERVICE_LINE}printf '#!/bin/sh\\n%s\\n' \"$line\" > \"$root{LINUX_LAUNCHERS}/$A_NAME\"\nchmod 0755 \"$root{LINUX_LAUNCHERS}/$A_NAME\"\nprintf '[Desktop Entry]\\nType=Application\\nName=%s\\nExec={LINUX_LAUNCHERS}/%s\\nX-GNOME-Autostart-enabled=true\\n' \"$A_NAME\" \"$A_NAME\" > \"$root/etc/xdg/autostart/$A_NAME.desktop\"\n"
    )
}

/// The LaunchAgent's label is the service name and its program is a launcher of that name.
fn macos_service_script() -> String {
    format!(
        "set -eu\nlaunchers=\"$HOME/{MACOS_LAUNCHERS}\"\nmkdir -p \"$launchers\" \"$HOME/Library/LaunchAgents\"\n{SERVICE_LINE}printf '#!/bin/sh\\n%s\\n' \"$line\" > \"$launchers/$A_NAME\"\nchmod 0755 \"$launchers/$A_NAME\"\nplist=\"$HOME/Library/LaunchAgents/$A_NAME.plist\"\nrm -f \"$plist\"\nplutil -create xml1 \"$plist\"\nplutil -insert Label -string \"$A_NAME\" \"$plist\"\nplutil -insert ProgramArguments -array \"$plist\"\nplutil -insert ProgramArguments.0 -string \"$launchers/$A_NAME\" \"$plist\"\nplutil -insert RunAtLoad -bool true \"$plist\"\n"
    )
}

pub fn service_steps(spec: &RecipeSpec) -> Vec<Step> {
    spec.services
        .iter()
        .map(|service| {
            let (who, script) = if spec.os == RecipeOs::Linux {
                (RunAs::Root, linux_service_script())
            } else {
                (RunAs::Account, macos_service_script())
            };
            step(
                format!("Adding the {} service", service.name),
                who,
                script,
                &[("NAME", &service.name), ("RUN", &service.run)],
            )
        })
        .collect()
}

/// One check per package manager and per service. A failed check names what is missing.
pub fn verify_steps(spec: &RecipeSpec) -> Vec<Step> {
    let linux = spec.os == RecipeOs::Linux;
    let (who, prelude) = if linux {
        (RunAs::Root, LINUX_PRELUDE)
    } else {
        (RunAs::Account, MACOS_PRELUDE)
    };
    let p = &spec.packages;
    let mut steps = Vec::new();
    let mut add = |manager: &str, list: &[String], check: &str| {
        if !list.is_empty() {
            steps.push(verify_step(step(
                format!("Checking the {manager} packages"),
                who,
                format!("{prelude}{BASE_NAME}for package in $A_PKGS; do\n  name=$(base \"$package\")\n  {check} || {{ echo \"{manager} package $package is not installed\" >&2; exit 1; }}\ndone\n"),
                &[("PKGS", &words(list))],
            )));
        }
    };
    add("apt", &p.apt, "dpkg -s \"$name\" >/dev/null 2>&1");
    add("brew", &p.brew, "brew list \"$name\" >/dev/null 2>&1");
    let pip = if linux {
        "pip3 show \"$name\" >/dev/null 2>&1"
    } else {
        "python3 -m pip show \"$name\" >/dev/null 2>&1"
    };
    add("pip", &p.pip, pip);
    // The check sees the pipx home and bin directory the install used.
    let pipx = if linux {
        format!(
            "( {}pipx list --short 2>/dev/null | grep -qi \"^$name \" )",
            LINUX_PIPX_ENV.trim_end().to_owned() + "; "
        )
    } else {
        "pipx list --short 2>/dev/null | grep -qi \"^$name \"".to_owned()
    };
    add("pipx", &p.pipx, &pipx);
    add(
        "npm",
        &p.npm,
        "npm ls -g --depth=0 --parseable 2>/dev/null | grep -q \"/node_modules/$name\\$\"",
    );
    let cargo = if linux {
        "cargo install --list --root /usr/local 2>/dev/null | grep -q \"^$name v\""
    } else {
        "cargo install --list 2>/dev/null | grep -q \"^$name v\""
    };
    add("cargo", &p.cargo, cargo);
    for service in &spec.services {
        let check = if linux {
            format!("test -x \"{LINUX_LAUNCHERS}/$A_NAME\" && test -f \"/etc/xdg/autostart/$A_NAME.desktop\"\n")
        } else {
            format!("test -x \"$HOME/{MACOS_LAUNCHERS}/$A_NAME\" && plutil -lint \"$HOME/Library/LaunchAgents/$A_NAME.plist\" >/dev/null\n")
        };
        steps.push(verify_step(step(
            format!("Checking the {} service", service.name),
            who,
            format!("set -eu\n{check}"),
            &[("NAME", &service.name)],
        )));
    }
    steps
}

/// Every step of a build, in order: packages, setup, services, then the checks.
pub fn all_steps(spec: &RecipeSpec) -> Vec<Step> {
    let mut steps = install_steps(spec);
    steps.extend(setup_steps(spec));
    steps.extend(service_steps(spec));
    steps.extend(verify_steps(spec));
    steps
}

fn shorten(command: &str) -> String {
    let line = command.lines().next().unwrap_or_default();
    if line.chars().count() > 40 {
        format!("{}...", line.chars().take(40).collect::<String>())
    } else {
        line.to_owned()
    }
}
