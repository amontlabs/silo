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
const MACOS_PRELUDE: &str = "set -eu\nBREW=$(command -v brew || echo /opt/homebrew/bin/brew)\n[ -x \"$BREW\" ] && eval \"$($BREW shellenv)\"\n";

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
    add("brew", &p.brew, "brew install $A_PKGS\n");
    let (ensure_pip, pip) = if linux {
        (
            "command -v pip3 >/dev/null || { apt-get update -qq; apt-get install -y -qq python3-pip; }\n",
            "pip3 install --break-system-packages $A_PKGS\n",
        )
    } else {
        (
            "command -v python3 >/dev/null || brew install python\n",
            "python3 -m pip install --user --break-system-packages $A_PKGS\n",
        )
    };
    add("pip", &p.pip, &format!("{ensure_pip}{pip}"));
    let pipx = if linux {
        "command -v pipx >/dev/null || { apt-get update -qq; apt-get install -y -qq pipx; }\nexport PIPX_HOME=/opt/pipx PIPX_BIN_DIR=/usr/local/bin\nfor package in $A_PKGS; do pipx install \"$package\"; done\n"
    } else {
        "command -v pipx >/dev/null || brew install pipx\nfor package in $A_PKGS; do pipx install \"$package\"; done\n"
    };
    add("pipx", &p.pipx, pipx);
    let npm = if linux {
        "command -v npm >/dev/null || { apt-get update -qq; apt-get install -y -qq nodejs npm; }\nnpm install -g $A_PKGS\n"
    } else {
        "command -v npm >/dev/null || brew install node\nnpm install -g $A_PKGS\n"
    };
    add("npm", &p.npm, npm);
    let cargo = if linux {
        "command -v cargo >/dev/null || { apt-get update -qq; apt-get install -y -qq cargo; }\ncargo install --root /usr/local $A_PKGS\n"
    } else {
        "command -v cargo >/dev/null || brew install rust\ncargo install $A_PKGS\n"
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

pub fn service_steps(spec: &RecipeSpec) -> Vec<Step> {
    spec.services
        .iter()
        .map(|service| {
            let (who, script) = if spec.os == RecipeOs::Linux {
                (
                    RunAs::Root,
                    "set -eu\ninstall -d -m 0755 /usr/local/libexec /etc/xdg/autostart\nprintf '#!/bin/sh\\n%s\\n' \"$A_RUN\" > \"/usr/local/libexec/silo-service-$A_NAME\"\nchmod 0755 \"/usr/local/libexec/silo-service-$A_NAME\"\nprintf '[Desktop Entry]\\nType=Application\\nName=silo-%s\\nExec=/usr/local/libexec/silo-service-%s\\nX-GNOME-Autostart-enabled=true\\n' \"$A_NAME\" \"$A_NAME\" > \"/etc/xdg/autostart/silo-$A_NAME.desktop\"\n",
                )
            } else {
                (
                    RunAs::Account,
                    "set -eu\nmkdir -p \"$HOME/Library/LaunchAgents\"\nplist=\"$HOME/Library/LaunchAgents/silo.service.$A_NAME.plist\"\nrm -f \"$plist\"\nplutil -create xml1 \"$plist\"\nplutil -insert Label -string \"silo.service.$A_NAME\" \"$plist\"\nplutil -insert ProgramArguments -array \"$plist\"\nplutil -insert ProgramArguments.0 -string /bin/sh \"$plist\"\nplutil -insert ProgramArguments.1 -string -c \"$plist\"\nplutil -insert ProgramArguments.2 -string \"$A_RUN\" \"$plist\"\nplutil -insert RunAtLoad -bool true \"$plist\"\n",
                )
            };
            step(
                format!("Adding the {} service", service.name),
                who,
                script.to_owned(),
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
    add(
        "pipx",
        &p.pipx,
        "pipx list --short 2>/dev/null | grep -qi \"^$name \"",
    );
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
            "test -x \"/usr/local/libexec/silo-service-$A_NAME\" && test -f \"/etc/xdg/autostart/silo-$A_NAME.desktop\"\n"
        } else {
            "plutil -lint \"$HOME/Library/LaunchAgents/silo.service.$A_NAME.plist\" >/dev/null\n"
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
