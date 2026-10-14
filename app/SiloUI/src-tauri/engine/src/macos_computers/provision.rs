//! Provisioning of an installed macOS computer for computer use: an account with
//! automatic login and SSH, System Integrity Protection off, computer use and the
//! clipboard agent installed. Progress is kept in the computer's record, so a retry
//! resumes at the first unfinished step.
//!
//! The computer's state stays `setting-up` throughout; each step names itself in
//! the detail. Machines started here have no display.
use super::setup_log::SetupLog;
use super::MacosContext;
use super::{
    app_data, checkpoints, developer_tools, engine, guest_access, guest_clipboard,
    guest_computer_use, layout_and_record, offline_setup, personalize, recovery, set_detail,
    store::{self, Layout, Record, SetupProgress},
    templates, Stop, RUN,
};
use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};

/// How long the first boot may take to give the guest an address.
const FIRST_BOOT_ADDRESS: Duration = Duration::from_secs(120);
/// A guest gets its address within seconds of starting, long before launchd has written
/// the state the offline edit builds on. The first boot runs this long after the address
/// appears, and twice as long again each time the edit finds that state missing.
const FIRST_BOOT_SETTLE: Duration = Duration::from_secs(50);
const FIRST_BOOT_ATTEMPTS: u32 = 3;
const SSH_WAIT: Duration = Duration::from_secs(300);
/// NVRAM's `prev-lang:kbd` is the language and keyboard layout Recovery starts with.
const RECOVERY_LANGUAGE_COMMAND: &str = "/usr/bin/sudo -n /usr/sbin/nvram prev-lang:kbd=en-US:0";
const GUEST_COMMAND: Duration = Duration::from_secs(120);
/// Personalization may expand the disk, which takes the guest a while.
const PERSONALIZE_COMMAND: Duration = Duration::from_secs(600);
/// How long an automatic login may take after a boot.
const LOGIN_WAIT: Duration = Duration::from_secs(180);
const SSH_POLL: Duration = Duration::from_secs(3);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(120);
const FORCED_STOP_WAIT: Duration = Duration::from_secs(30);
const MACHINE_POLL: Duration = Duration::from_millis(500);

const INHERITED_ACCESS_GONE: &str =
    "This fork has lost the access it was copied with. Delete it and fork the checkpoint again.";

pub(super) fn developer_tools_outdated(record: &Record, version: &str) -> bool {
    record.developer_tools_version.as_deref() != Some(version)
}

/// Runs every unfinished step for `record`, saving progress after each.
pub(super) fn run(
    app: &MacosContext,
    layout: &Layout,
    record: &mut Record,
    cancel: &AtomicU8,
    reservation: Option<templates::SpaceReservation>,
) -> Result<(), Stop> {
    let provision = Provision {
        app,
        layout,
        id: record.id.clone(),
        cancel,
        reservation: std::cell::Cell::new(reservation),
        log: SetupLog::open(app, &record.id),
    };
    provision.log("setup started");
    let result = provision.steps(record);
    match &result {
        Ok(()) => provision.log("setup finished"),
        Err(Stop::Cancelled) => provision.log("setup cancelled"),
        Err(Stop::Failed(message)) => provision.log(&format!("setup failed: {message}")),
    }
    if result.is_err() {
        // Nothing may report the setup as over while the machine can still run.
        provision.ensure_stopped();
    }
    result
}

struct Provision<'a> {
    app: &'a MacosContext,
    layout: &'a Layout,
    id: String,
    cancel: &'a AtomicU8,
    /// Space set aside for a copy's writes, held by the creation that made it.
    reservation: std::cell::Cell<Option<templates::SpaceReservation>>,
    log: Option<SetupLog>,
}

impl Provision<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) != RUN
            || crate::runtime::OPERATIONS.removal_queued(&self.id)
    }

    fn log(&self, message: &str) {
        if let Some(log) = &self.log {
            log.line(message);
        }
    }

    fn log_command(&self, what: &str, output: &guest_access::CommandOutput) {
        if let Some(log) = &self.log {
            log.command(what, output.status, &output.stdout, &output.stderr);
        }
    }

    fn check(&self) -> Result<(), Stop> {
        if self.cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Names the current step, unless the setup is already ending.
    fn say(&self, step: &str) -> Result<(), Stop> {
        self.check()?;
        self.log(&format!("step: {step}"));
        set_detail(self.app, &self.id, step);
        Ok(())
    }

    /// Records a finished step and saves it.
    fn mark(
        &self,
        record: &mut Record,
        finished: impl FnOnce(&mut SetupProgress),
    ) -> Result<(), Stop> {
        finished(&mut record.setup);
        self.log(&format!(
            "setup step finished: {}",
            setup_summary(&record.setup)
        ));
        store::save(self.layout, record)?;
        let saved = record.clone();
        super::update(self.app, &self.id, |entry| entry.record = saved);
        Ok(())
    }

    fn steps(&self, record: &mut Record) -> Result<(), Stop> {
        let developer_tools_version = developer_tools::version();
        if record.setup.needs_personalizing {
            if record.imported {
                self.personalize_import(record)?;
            } else {
                self.personalize(record)?;
            }
        }
        // The account step leaves the language Recovery uses set.
        let mut language_ready = false;
        if !record.setup.account {
            self.create_account(record)?;
            language_ready = true;
        }
        if !record.setup.sip {
            self.say("Turning off System Integrity Protection")?;
            let set_language = || {
                self.prepare_recovery_language().map_err(|stop| match stop {
                    Stop::Cancelled => guest_access::CANCELLED.to_string(),
                    Stop::Failed(message) => message,
                })
            };
            recovery::disable_sip(
                self.app,
                &self.id,
                &|| self.cancelled(),
                language_ready,
                &set_language,
            )
            .map_err(|message| {
                if self.cancelled() {
                    Stop::Cancelled
                } else {
                    Stop::Failed(message)
                }
            })?;
            self.check()?;
        }
        if setup_finished(record, &developer_tools_version) {
            if record.imported {
                record.imported = false;
                self.mark(record, |_| {})?;
            }
            return Ok(());
        }
        self.say("Starting macOS")?;
        self.boot(Login::Key)?;
        if !record.setup.sip {
            self.say("Checking System Integrity Protection")?;
            self.verify_sip_disabled()?;
            self.mark(record, |setup| setup.sip = true)?;
        }
        if !record.setup.developer_tools
            || developer_tools_outdated(record, &developer_tools_version)
        {
            self.say("Installing developer tools")?;
            developer_tools::install(self.app, &self.id, &|| self.cancelled()).map_err(
                |message| {
                    if self.cancelled() || message == guest_access::CANCELLED {
                        Stop::Cancelled
                    } else {
                        Stop::Failed(message)
                    }
                },
            )?;
            self.check()?;
            record.developer_tools_version = Some(developer_tools_version);
            record.setup_version = Some(templates::setup_version());
            self.mark(record, |setup| setup.developer_tools = true)?;
        }
        if !record.setup.computer_use {
            self.say("Installing computer use")?;
            // The mode installed is the one the template's version names.
            let approval = crate::computer_use::initial_approval();
            guest_computer_use::install(self.app, &self.id, approval)?;
            record.setup_version = Some(templates::setup_version());
            record.computer_use_version = Some(templates::computer_use_version_for(approval));
            record.computer_use_approval = Some(approval);
            record.computer_use_desired_approval = Some(approval);
            self.mark(record, |setup| setup.computer_use = true)?;
        }
        if !record.setup.clipboard {
            self.say("Setting up the clipboard")?;
            guest_clipboard::install(self.app, &self.id)?;
            self.mark(record, |setup| setup.clipboard = true)?;
        }
        if !record.setup.roots {
            self.say("Creating workspace folders")?;
            let (layout, current) = layout_and_record(self.app, &self.id)?;
            let output = guest_access::run(
                &layout,
                &current,
                "mkdir -p /Users/silo/workspace /Users/silo/Downloads && test -d /Users/silo/workspace && test -d /Users/silo/Downloads",
                None,
                GUEST_COMMAND,
            )?;
            self.log_command("workspace folders", &output);
            if output.status != 0 {
                return Err(Stop::Failed(
                    "Silo could not create the computer's workspace folders.".into(),
                ));
            }
            self.mark(record, |setup| setup.roots = true)?;
        }
        // A copy of a template made with another computer use brings it up to date here,
        // before the computer is handed over.
        if record.setup.computer_use && record.computer_use_stale() {
            self.say("Updating Computer Use")?;
            let approval = record
                .computer_use_approval
                .unwrap_or_else(crate::computer_use::initial_approval);
            guest_computer_use::install(self.app, &self.id, approval)?;
            record.computer_use_version = Some(templates::computer_use_version_for(approval));
            record.computer_use_approval = Some(approval);
            record.computer_use_desired_approval = Some(approval);
            self.mark(record, |_| {})?;
        }
        if record.setup.guest_steps_complete() {
            self.say("Setting up network and trust")?;
            stage_bootstrap(record, |record| {
                super::proxy::stage_guest_bootstrap(self.app, record, self.layout)
            })?;
            // Only now is the computer ready: a failure above leaves the flag unset, so a
            // retry runs this step again over the temporary NAT boot.
            self.mark(record, |setup| setup.proxy_bootstrap = true)?;
        }
        self.shut_down()?;
        if record.imported {
            record.imported = false;
            self.mark(record, |_| {})?;
        }
        Ok(())
    }

    /// The account, its automatic login and SSH: a first boot so macOS writes its
    /// first-boot state, the offline edit, and a second boot to finish what only
    /// the running guest can.
    fn create_account(&self, record: &mut Record) -> Result<(), Stop> {
        let account = guest_access::ensure_account(self.layout)?;
        if let Some(log) = &self.log {
            log.hide(&account.password);
        }
        let image = record.restore_image.clone();
        let mut settle = FIRST_BOOT_SETTLE;
        for attempt in 1..=FIRST_BOOT_ATTEMPTS {
            self.say("Starting macOS for the first time")?;
            self.first_boot(settle)?;
            self.say("Creating the account")?;
            let release = image.as_ref().map(|image| offline_setup::Release {
                version: &image.version,
                build: &image.build,
            });
            match offline_setup::run(&self.layout.disk(), &account, release) {
                Err(message)
                    if message == offline_setup::FIRST_BOOT_INCOMPLETE
                        && attempt < FIRST_BOOT_ATTEMPTS =>
                {
                    settle *= 2;
                }
                result => {
                    result?;
                    break;
                }
            }
        }
        self.say("Starting macOS")?;
        self.boot(Login::Password)?;
        self.say("Finishing the account")?;
        self.finalize(&account)?;
        self.set_recovery_language()?;
        self.shut_down()?;
        self.mark(record, |setup| setup.account = true)
    }

    /// Gives a copy of a template its own password, key, host keys, keychain and name,
    /// then restarts it to check that it came up as its own computer. A run that ends
    /// early is repeated whole: the template's key stays valid until the last command of
    /// the script, and a copy that already has its own key skips the script.
    fn personalize(&self, record: &mut Record) -> Result<(), Stop> {
        let data = app_data(self.app)?;
        // A resumed personalization writes as much as a first one, so it needs the space too.
        let _space = match self.reservation.take() {
            Some(held) => held,
            None => templates::reserve_space(&data, templates::COPY_ESTIMATE)?,
        };
        // A copy of a template holds a lease on it; a fork has the credentials of its source.
        let (lease, access) = if record.inherited_access {
            (None, checkpoints::inherited_access(self.layout))
        } else {
            let name = record
                .template
                .clone()
                .ok_or_else(|| Stop::Failed(templates::TEMPLATE_GONE.into()))?;
            let lease = templates::lease_named(&data, &name)?;
            let access = lease.template.access_dir();
            (Some(lease), access)
        };
        if !access.join("id_ed25519").exists() {
            return Err(Stop::Failed(if record.inherited_access {
                INHERITED_ACCESS_GONE.into()
            } else {
                templates::TEMPLATE_GONE.into()
            }));
        }
        let own = guest_access::ensure_account(self.layout)?;
        if let Some(log) = &self.log {
            log.hide(&own.password);
        }
        let template_login = self.layout.clone().with_access(access);
        if let (Some(log), Ok(template)) = (&self.log, guest_access::account(&template_login)) {
            log.hide(&template.password);
        }
        self.say("Personalizing the computer")?;
        let mut shortfall = None;
        // The backing file's length is the target, whatever the record says after an attempt.
        let requested = personalize::requested_gib(&self.layout.disk())?;
        // A fork has its source's disk size: there is nothing to grow.
        let template_gib = lease
            .as_ref()
            .map_or(requested, |lease| lease.template.meta.disk_gib);
        let grow = requested > template_gib;
        let mut unallocated = None;
        let mut reconciled = None;
        // The guest's host keys are about to change.
        guest_access::reset_host_keys(self.layout)?;
        self.start_machine()?;
        if self.wait_for_login(&template_login, record)? == LoginKey::Template {
            let script = personalize::script(&guest_access::public_key(&own)?, &record.name, grow);
            let output = guest_access::run(
                &template_login,
                record,
                &personalize::command(&script),
                Some(&personalize::input(
                    &own.password,
                    &guest_access::account(&template_login)?.password,
                )),
                PERSONALIZE_COMMAND,
            )?;
            self.log_command("personalization script", &output);
            let outcome =
                personalize::outcome(output.status, &output.stdout).map_err(Stop::Failed)?;
            unallocated = outcome.unallocated;
            // The script replaced the host keys, and every new connection presents them.
            guest_access::reset_host_keys(self.layout)?;
            if !guest_access::probe(self.layout, record) {
                return Err(Stop::Failed(
                    "Silo could not log in to the computer with its own key.".into(),
                ));
            }
        } else if grow {
            // The script ran in an earlier attempt, whose measurement was lost.
            let output = guest_access::run(
                self.layout,
                record,
                &personalize::command(&personalize::grow_script()),
                None,
                PERSONALIZE_COMMAND,
            )?;
            unallocated = personalize::unallocated(&output.stdout);
        }
        if grow {
            let disk = personalize::reconcile_disk(requested, template_gib, unallocated);
            if disk.short {
                self.log("personalization: the disk could not be expanded");
                shortfall = Some(format!(
                    "Silo could not expand the disk to {requested} GiB, so the computer has {} GiB. It is otherwise ready to start.",
                    disk.gib
                ));
            }
            reconciled = Some(disk.gib);
        }
        self.shut_down()?;
        guest_access::reset_host_keys(self.layout)?;
        self.say("Checking the computer")?;
        self.boot(Login::Key)?;
        self.verify_sip_disabled()?;
        self.verify_personalized()?;
        // A fully provisioned copy will use FileHandle networking as soon as
        // personalization completes. Stage its first-boot route and System keychain
        // trust before that transition. Incomplete copies stay on NAT until the final
        // provisioning step stages the bootstrap below.
        let staged = record.setup.guest_steps_complete();
        if staged {
            self.say("Setting up network and trust")?;
            stage_bootstrap(record, |record| {
                super::proxy::stage_guest_bootstrap(self.app, record, self.layout)
            })?;
        }
        self.shut_down()?;
        // Only a verified computer records the size it really has.
        if let Some(gib) = reconciled.filter(|gib| *gib != record.disk_gib) {
            record.disk_gib = gib;
        }
        record.inherited_access = false;
        self.mark(record, |setup| {
            setup.needs_personalizing = false;
            setup.proxy_bootstrap = staged;
        })?;
        // Its own credentials are the only ones left.
        let _ = std::fs::remove_dir_all(checkpoints::inherited_access(self.layout));
        // The computer is usable and complete; the message stays as its failure detail.
        shortfall.map_or(Ok(()), |message| Err(Stop::Failed(message)))
    }

    /// An imported disk has already had its managed password hash changed offline.
    /// Its first setup boot accepts that password, installs this computer's new
    /// host key and authorization, then proves key login before any other setup runs.
    fn personalize_import(&self, record: &mut Record) -> Result<(), Stop> {
        let account = guest_access::account(self.layout)?;
        if let Some(log) = &self.log {
            log.hide(&account.password);
        }
        self.say("Personalizing the imported computer")?;
        boot_import_with_host_key_reset(self.layout, || self.boot(Login::Password))?;
        finalize_import_guest(self.layout, record, &account, self.log.as_ref())
            .map_err(Stop::Failed)?;
        let output = guest_access::run(
            self.layout,
            record,
            "/usr/bin/sudo -n true",
            None,
            GUEST_COMMAND,
        )?;
        self.log_command("imported key login with sudo -n", &output);
        if output.status != 0 {
            return Err(Stop::Failed(
                "Silo could not log in to the imported computer with its new key.".into(),
            ));
        }
        self.shut_down()?;
        self.mark(record, |setup| setup.needs_personalizing = false)
    }

    /// Waits until the guest answers a login with the template's key or its own.
    fn wait_for_login(&self, template_login: &Layout, record: &Record) -> Result<LoginKey, Stop> {
        let deadline = Instant::now() + SSH_WAIT;
        loop {
            self.check()?;
            if guest_access::probe(self.layout, record) {
                return Ok(LoginKey::Own);
            }
            if guest_access::probe(template_login, record) {
                return Ok(LoginKey::Template);
            }
            if Instant::now() >= deadline {
                return Err(Stop::Failed(
                    "The computer did not accept SSH logins in time.".into(),
                ));
            }
            std::thread::sleep(SSH_POLL);
        }
    }

    /// Checks a restarted copy: LCU's receipt came along, and macOS logged in on its own
    /// with the new password.
    fn verify_personalized(&self) -> Result<(), Stop> {
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        let deadline = Instant::now() + LOGIN_WAIT;
        loop {
            self.check()?;
            let output =
                guest_access::run(&layout, &record, GUEST_STATE_COMMAND, None, GUEST_COMMAND)?;
            let state = guest_state(&output.stdout);
            if !state.receipt {
                return Err(Stop::Failed(
                    "The copy of the template has no computer use installed.".into(),
                ));
            }
            if state.console_user.as_deref() == Some(guest_access::USER) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Stop::Failed(
                    "The computer did not log in on its own after it was personalized.".into(),
                ));
            }
            std::thread::sleep(SSH_POLL);
        }
    }

    /// Runs the root script with the account's password, which is the only
    /// credential a new guest has, then checks that the key and `sudo -n` work.
    fn finalize(&self, account: &guest_access::GuestAccount) -> Result<(), Stop> {
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        finalize_guest(&layout, &record, account, self.log.as_ref()).map_err(Stop::Failed)
    }

    /// Stores English and a US keyboard layout in the running guest's NVRAM, which
    /// is where Recovery reads its language: it follows the language of the Mac
    /// that runs the computer otherwise, and Silo reads its screen in English.
    fn set_recovery_language(&self) -> Result<(), Stop> {
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        let output = guest_access::run_cancellable(
            &layout,
            &record,
            RECOVERY_LANGUAGE_COMMAND,
            GUEST_COMMAND,
            &|| self.cancelled(),
        )
        .map_err(|message| {
            if self.cancelled() {
                Stop::Cancelled
            } else {
                Stop::Failed(message)
            }
        })?;
        self.log_command("nvram prev-lang:kbd", &output);
        if output.status != 0 {
            return Err(Stop::Failed(
                "Silo could not set the language Recovery uses.".into(),
            ));
        }
        Ok(())
    }

    /// Starts the computer, sets the language Recovery uses and shuts it down.
    fn prepare_recovery_language(&self) -> Result<(), Stop> {
        self.say("Preparing Recovery")?;
        self.boot(Login::Key)?;
        self.set_recovery_language()?;
        self.shut_down()
    }

    fn verify_sip_disabled(&self) -> Result<(), Stop> {
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        let output = guest_access::run(
            &layout,
            &record,
            "/usr/bin/csrutil status",
            None,
            GUEST_COMMAND,
        )?;
        self.log_command("csrutil status", &output);
        if sip_disabled(&output.stdout) {
            Ok(())
        } else {
            Err(Stop::Failed(
                "System Integrity Protection is still on in the computer.".into(),
            ))
        }
    }

    /// Starts the machine without a display.
    fn start_machine(&self) -> Result<(), Stop> {
        self.check()?;
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        offline_setup::ensure_detached(&layout.disk())?;
        engine::start(self.app, &record, &layout)?;
        Ok(())
    }

    /// Boots the computer without a display and waits for SSH.
    fn boot(&self, login: Login) -> Result<(), Stop> {
        self.start_machine()?;
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        let cancelled = || self.cancelled();
        match login {
            Login::Key => guest_access::wait_for_ssh(&layout, &record, SSH_WAIT, &cancelled),
            Login::Password => {
                guest_access::wait_for_password_ssh(&layout, &record, SSH_WAIT, &cancelled)
            }
        }
        .map_err(|message| {
            if self.cancelled() {
                Stop::Cancelled
            } else {
                Stop::Failed(message)
            }
        })?;
        Ok(())
    }

    /// Lets macOS write its first-boot state for `settle` after the guest has an address,
    /// then stops it hard, as Lume does.
    fn first_boot(&self, settle: Duration) -> Result<(), Stop> {
        self.check()?;
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        offline_setup::ensure_detached(&layout.disk())?;
        engine::start(self.app, &record, &layout)?;
        let deadline = Instant::now() + FIRST_BOOT_ADDRESS;
        while guest_access::guest_address(&record.mac_address).is_err() {
            self.check()?;
            if Instant::now() >= deadline {
                return Err(Stop::Failed(
                    "The computer did not start for the first time.".into(),
                ));
            }
            std::thread::sleep(MACHINE_POLL);
        }
        let settled = Instant::now() + settle;
        while Instant::now() < settled {
            self.check()?;
            std::thread::sleep(MACHINE_POLL);
        }
        engine::force_stop(self.app, &self.id)?;
        self.wait_stopped(FORCED_STOP_WAIT)
    }

    /// Asks macOS to shut down and waits; a guest that ignores it is stopped hard.
    fn shut_down(&self) -> Result<(), Stop> {
        self.say("Shutting down")?;
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        // The connection ends with the guest, so its outcome says nothing.
        let _ = guest_access::run(
            &layout,
            &record,
            "/usr/bin/sudo -n /sbin/shutdown -h now",
            None,
            GUEST_COMMAND,
        );
        if self.wait_stopped(SHUTDOWN_WAIT).is_err() {
            engine::force_stop(self.app, &self.id)?;
            self.wait_stopped(FORCED_STOP_WAIT)?;
        }
        Ok(())
    }

    /// Waits until the framework holds no machine for this computer.
    fn wait_stopped(&self, timeout: Duration) -> Result<(), Stop> {
        let deadline = Instant::now() + timeout;
        loop {
            let samples = engine::machine_samples(self.app)?;
            match samples.iter().find(|sample| sample.id == self.id) {
                None => return Ok(()),
                Some(sample)
                    if matches!(
                        sample.state,
                        engine::MachineState::Stopped | engine::MachineState::Failed
                    ) =>
                {
                    // The framework reported the end, but its delegate has not released the machine.
                    super::machine_stopped(self.app, &self.id, sample.generation, None);
                }
                Some(_) => {}
            }
            if Instant::now() >= deadline {
                return Err(Stop::Failed("The computer did not stop in time.".into()));
            }
            std::thread::sleep(MACHINE_POLL);
        }
    }

    /// Stops the computer's machine after a failed or cancelled setup and returns
    /// only once the framework holds no machine for it. Until then the computer
    /// stays in setup, so it can be neither deleted nor reported as stopped.
    fn ensure_stopped(&self) {
        loop {
            let held = engine::machine_states(self.app)
                .map_or(true, |states| states.iter().any(|(id, _)| *id == self.id));
            if !held {
                return;
            }
            set_detail(self.app, &self.id, "Stopping the computer");
            let _ = engine::force_stop(self.app, &self.id);
            if self.wait_stopped(FORCED_STOP_WAIT).is_ok() {
                return;
            }
            std::thread::sleep(MACHINE_POLL);
        }
    }
}

/// The finished steps of a setup in words, for the log.
fn setup_summary(setup: &SetupProgress) -> String {
    let done: Vec<&str> = [
        (setup.account, "account"),
        (setup.sip, "sip"),
        (setup.developer_tools, "developer tools"),
        (setup.computer_use, "computer use"),
        (setup.clipboard, "clipboard"),
        (setup.roots, "workspace folders"),
        (!setup.needs_personalizing, "personalized"),
    ]
    .into_iter()
    .filter_map(|(finished, name)| finished.then_some(name))
    .collect();
    if done.is_empty() {
        "nothing done yet".into()
    } else {
        format!("{} done", done.join(", "))
    }
}

/// Stages the guest's network and trust through `stage`. The caller records the result
/// only after this returns `Ok`, so a failure leaves the computer not set up and Retry
/// available.
fn stage_bootstrap(
    record: &Record,
    stage: impl FnOnce(&Record) -> Result<(), String>,
) -> Result<(), Stop> {
    stage(record).map_err(Stop::Failed)
}

/// Whether the computer's setup has nothing left to run, so Retry has no work to do.
fn setup_finished(record: &Record, developer_tools_version: &str) -> bool {
    record.setup.sip
        && record.setup.developer_tools
        && !developer_tools_outdated(record, developer_tools_version)
        && record.setup.computer_use
        && record.setup.clipboard
        && record.setup.roots
        && !record.computer_use_stale()
        && record.setup.proxy_bootstrap
}

/// Which key a copy of a template answers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginKey {
    /// The template's: the personalization has not run.
    Template,
    /// Its own: the personalization ran.
    Own,
}

/// Prints whether LCU's receipt exists, who owns the console and whether `sudo -n` works.
const GUEST_STATE_COMMAND: &str = "/bin/test -f \"$HOME/Library/Application Support/Silo/computer-use-receipt.json\" && /bin/echo SILO_RECEIPT; /usr/bin/stat -f 'SILO_CONSOLE=%Su' /dev/console; /usr/bin/sudo -n true && /bin/echo SILO_SUDO";

#[derive(Debug, PartialEq, Eq)]
struct GuestState {
    receipt: bool,
    console_user: Option<String>,
}

fn guest_state(stdout: &str) -> GuestState {
    GuestState {
        receipt: stdout.lines().any(|line| line.trim() == "SILO_RECEIPT"),
        console_user: stdout
            .lines()
            .find_map(|line| line.trim().strip_prefix("SILO_CONSOLE="))
            .map(str::to_string),
    }
}

/// How the first connection to a boot proves who it is.
#[derive(Clone, Copy)]
enum Login {
    /// The account's key, installed by the finalization.
    Key,
    /// The account's password, for a guest whose key is not installed yet.
    Password,
}

/// Finishes the account inside a booted guest that only accepts the password.
fn finalize_guest(
    layout: &Layout,
    record: &Record,
    account: &guest_access::GuestAccount,
    log: Option<&SetupLog>,
) -> Result<(), String> {
    let public_key = guest_access::public_key(account)?;
    let script = base64_encode(offline_setup::finalization_script(&public_key).as_bytes());
    let command =
        format!("/usr/bin/sudo -S -p '' /bin/sh -c 'echo {script} | /usr/bin/base64 -D | /bin/sh'");
    let password = format!("{}\n", account.password);
    let output = guest_access::run_with_password(
        layout,
        record,
        &command,
        Some(password.as_bytes()),
        GUEST_COMMAND,
    )?;
    if output.status != 0 || !output.stdout.contains("MARKER_OWNER=0:0") {
        if let Some(log) = log {
            log.command(
                "account script",
                output.status,
                &output.stdout,
                &output.stderr,
            );
        }
        return Err(finalization_failure(output.status));
    }
    let output = guest_access::run(layout, record, "/usr/bin/sudo -n true", None, GUEST_COMMAND)?;
    if output.status != 0 {
        if let Some(log) = log {
            log.command(
                "key login with sudo -n",
                output.status,
                &output.stdout,
                &output.stderr,
            );
        }
        return Err(
            "Silo could not log in to the computer with its key and run administrator commands."
                .into(),
        );
    }
    Ok(())
}

/// Imported finalization can rotate the guest's SSH host keys before its result
/// reaches the owner. Clear any previous pin before each retry's password boot
/// so `accept-new` can learn the key from the current disk state.
fn boot_import_with_host_key_reset(
    layout: &Layout,
    boot: impl FnOnce() -> Result<(), Stop>,
) -> Result<(), Stop> {
    guest_access::reset_host_keys(layout)?;
    boot()
}

/// Finish a rekeyed archive import using the new password once, then verify the
/// newly installed device key. The source key is never read or used.
fn finalize_import_guest(
    layout: &Layout,
    record: &Record,
    account: &guest_access::GuestAccount,
    log: Option<&SetupLog>,
) -> Result<(), String> {
    let public_key = guest_access::public_key(account)?;
    let script = base64_encode(
        offline_setup::import_finalization_script(&public_key, &record.name).as_bytes(),
    );
    let command =
        format!("/usr/bin/sudo -S -p '' /bin/sh -c 'echo {script} | /usr/bin/base64 -D | /bin/sh'");
    let password = format!("{}\n", account.password);
    let output = guest_access::run_with_password(
        layout,
        record,
        &command,
        Some(password.as_bytes()),
        GUEST_COMMAND,
    )?;
    if output.status != 0 || !output.stdout.contains("MARKER_OWNER=0:0") {
        if let Some(log) = log {
            log.command(
                "import account script",
                output.status,
                &output.stdout,
                &output.stderr,
            );
        }
        return Err(finalization_failure(output.status));
    }
    guest_access::reset_host_keys(layout)?;
    Ok(())
}

/// A one-sentence reason for a failed account script; the details go to the log.
fn finalization_failure(status: i32) -> String {
    if status == offline_setup::PREBOOT_FAILED {
        "Silo could not finish setting up the account: diskutil could not update the preboot volume.".into()
    } else {
        "Silo could not finish setting up the account in the computer.".into()
    }
}

fn sip_disabled(csrutil_status: &str) -> bool {
    csrutil_status.to_ascii_lowercase().contains("disabled")
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_setup_does_not_skip_a_missing_or_old_developer_tools_fingerprint() {
        let mut record = store::new_record(
            &store::CreateRequest {
                name: "mac-one".into(),
                cpus: 4,
                memory_gib: 8,
                disk_gib: 64,
            },
            "02:00:00:00:00:01".into(),
        );
        record.setup.developer_tools = true;
        let current = developer_tools::version();
        assert!(developer_tools_outdated(&record, &current));
        record.developer_tools_version = Some(current.clone());
        assert!(!developer_tools_outdated(&record, &current));
    }

    fn finished_record() -> Record {
        let mut record = store::new_record(
            &store::CreateRequest {
                name: "mac-one".into(),
                cpus: 4,
                memory_gib: 8,
                disk_gib: 64,
            },
            "02:00:00:00:00:01".into(),
        );
        record.installed = true;
        record.setup = SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: true,
            needs_personalizing: false,
            proxy_bootstrap: false,
        };
        record.developer_tools_version = Some(developer_tools::version());
        let approval = crate::computer_use::initial_approval();
        record.computer_use_approval = Some(approval);
        record.computer_use_version = Some(templates::computer_use_version_for(approval));
        record
    }

    #[test]
    fn proxy_bootstrap_is_staged_only_after_all_guest_setup_steps_are_ready() {
        let complete = SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: true,
            needs_personalizing: true,
            proxy_bootstrap: false,
        };
        assert!(complete.guest_steps_complete());
        assert!(!SetupProgress {
            developer_tools: false,
            ..complete
        }
        .guest_steps_complete());
        assert!(!SetupProgress {
            roots: false,
            ..complete
        }
        .guest_steps_complete());
    }

    /// A staging failure after the last setup flag must leave the computer retryable, and
    /// the retry must reach the staging step and finish the setup in the same session.
    #[test]
    fn a_failed_bootstrap_after_the_last_setup_flag_leaves_setup_incomplete_until_retry() {
        let mut record = finished_record();
        // Every guest step is done; only the bootstrap remains.
        assert!(record.setup.guest_steps_complete());
        assert!(!setup_finished(&record, &developer_tools::version()));

        let failure = stage_bootstrap(&record, |_| Err("copy failed".into()));
        assert!(matches!(failure, Err(Stop::Failed(message)) if message == "copy failed"));
        // Nothing was recorded: not set up, Retry is admitted, and Start is refused.
        assert!(!record.setup_complete());
        assert!(store::setup_allowed(store::State::Failed, &record).is_ok());
        assert!(store::setup_allowed(store::State::Stopped, &record).is_ok());
        assert!(!setup_finished(&record, &developer_tools::version()));
        // The retry boots over the temporary Apple NAT, never behind the gateway.
        assert!(!engine::boots_behind_gateway(&record));

        let mut staged = 0;
        stage_bootstrap(&record, |_| {
            staged += 1;
            Ok(())
        })
        .map_err(|_| ())
        .unwrap();
        assert_eq!(staged, 1);
        record.setup.proxy_bootstrap = true;

        assert!(record.setup_complete());
        assert!(setup_finished(&record, &developer_tools::version()));
        assert!(store::setup_allowed(store::State::Stopped, &record).is_err());
        assert!(engine::boots_behind_gateway(&record));
    }

    #[test]
    fn the_setup_log_names_the_finished_steps() {
        let mut setup = SetupProgress {
            needs_personalizing: true,
            proxy_bootstrap: false,
            ..SetupProgress::default()
        };
        assert_eq!(setup_summary(&setup), "nothing done yet");
        setup.account = true;
        setup.sip = true;
        setup.developer_tools = true;
        setup.roots = true;
        assert_eq!(
            setup_summary(&setup),
            "account, sip, developer tools, workspace folders done"
        );
    }

    #[test]
    fn imported_password_boot_forgets_the_previous_host_key_before_retrying() {
        let temp = tempfile::tempdir().unwrap();
        let layout = Layout {
            dir: temp.path().to_path_buf(),
            access: None,
        };
        let known_hosts = layout.dir.join("guest-access/known_hosts");
        std::fs::create_dir_all(known_hosts.parent().unwrap()).unwrap();
        std::fs::write(&known_hosts, b"inherited guest host key").unwrap();

        let first_boot = boot_import_with_host_key_reset(&layout, || {
            assert!(
                !known_hosts.exists(),
                "the inherited key must not pin this boot"
            );
            std::fs::write(&known_hosts, b"host key learned from the guest").unwrap();
            Ok(())
        });
        assert!(first_boot.is_ok());
        assert!(known_hosts.is_file());

        let result = boot_import_with_host_key_reset(&layout, || {
            assert!(
                !known_hosts.exists(),
                "a retry must discard a stale pin first"
            );
            Err(Stop::Failed("simulated interrupted finalization".into()))
        });
        assert!(
            matches!(result, Err(Stop::Failed(message)) if message == "simulated interrupted finalization")
        );
    }

    #[test]
    fn the_finalization_script_survives_the_shell_round_trip() {
        use base64::Engine;
        let script = offline_setup::finalization_script("ssh-ed25519 AAAA test");
        let encoded = base64_encode(script.as_bytes());
        assert!(!encoded.contains('\''));
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(decoded, script.as_bytes());
    }

    #[test]
    fn a_failed_account_script_gives_one_sentence_without_raw_output() {
        assert_eq!(
            finalization_failure(offline_setup::PREBOOT_FAILED),
            "Silo could not finish setting up the account: diskutil could not update the preboot volume."
        );
        let other = finalization_failure(1);
        assert!(other.ends_with('.') && !other.contains('\n') && !other.contains("chown"));
    }

    #[test]
    fn the_guest_state_is_read_from_the_marker_lines() {
        assert_eq!(
            guest_state("SILO_RECEIPT\nSILO_CONSOLE=silo\nSILO_SUDO\n"),
            GuestState {
                receipt: true,
                console_user: Some("silo".into())
            }
        );
        assert_eq!(
            guest_state("SILO_CONSOLE=root\n"),
            GuestState {
                receipt: false,
                console_user: Some("root".into())
            }
        );
        assert_eq!(
            guest_state(""),
            GuestState {
                receipt: false,
                console_user: None
            }
        );
    }

    #[test]
    fn csrutil_output_decides_whether_protection_is_off() {
        assert!(sip_disabled(
            "System Integrity Protection status: disabled.\n"
        ));
        assert!(!sip_disabled(
            "System Integrity Protection status: enabled.\n"
        ));
    }

    fn live_layout() -> (Layout, Record) {
        let dir = std::path::PathBuf::from(std::env::var("SILO_LIVE_DIR").expect("SILO_LIVE_DIR"));
        let request = store::CreateRequest {
            name: "live".into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
        };
        let mac = std::fs::read_to_string(dir.join("mac-address.txt")).unwrap();
        let mut record = store::new_record(&request, mac.trim().into());
        record.id = "live".into();
        (Layout { dir, access: None }, record)
    }

    /// Patches the disk of a clone. Run by hand: `SILO_LIVE_DIR=<clone> cargo test live_offline -- --ignored`.
    #[test]
    #[ignore = "needs a clone of an installed computer"]
    fn live_offline() {
        let (layout, _) = live_layout();
        let account = guest_access::ensure_account(&layout).unwrap();
        let result = offline_setup::run(
            &layout.disk(),
            &account,
            Some(offline_setup::Release {
                version: "26.6.2",
                build: "25G83",
            }),
        );
        println!("offline setup: {result:?}");
        if std::env::var("SILO_LIVE_EXPECT_INCOMPLETE").is_ok() {
            assert_eq!(
                result,
                Err(offline_setup::FIRST_BOOT_INCOMPLETE.to_string())
            );
        } else {
            result.unwrap();
        }
    }

    /// Finishes the account of a clone that is booted and reachable. Run by hand.
    #[test]
    #[ignore = "needs a running clone"]
    fn live_finalize() {
        let (layout, record) = live_layout();
        let account = guest_access::ensure_account(&layout).unwrap();
        let address = guest_access::wait_for_password_ssh(
            &layout,
            &record,
            Duration::from_secs(300),
            &|| false,
        )
        .unwrap();
        println!("password login works at {address}");
        finalize_guest(&layout, &record, &account, None).unwrap();
        println!("finalized; key login and sudo -n work");
    }
}
