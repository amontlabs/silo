#!/usr/bin/env python3
"""Exercise the production Linux WebKit app through external WebDriver.

No IPC mocks, app test flags, installed credentials or existing Silo data are used.
Run with xvfb-run -a dbus-run-session -- python3 scripts/test-linux-desktop.py.
"""
import json
import hashlib
import os
from pathlib import Path
import signal
import secrets
import shlex
import shutil
import socket
import subprocess
import tempfile
import time

from selenium import webdriver
from selenium.common.exceptions import ElementClickInterceptedException, ElementNotInteractableException, InvalidSessionIdException, StaleElementReferenceException, WebDriverException
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.options import BaseOptions
from selenium.webdriver.support.ui import WebDriverWait
from channel_names import channel_names

ROOT = Path(__file__).resolve().parent.parent
EVIDENCE = Path(os.environ.get("SILO_LINUX_EVIDENCE", ROOT / "test-results/linux"))
EVIDENCE.mkdir(parents=True, exist_ok=True)


class Options(BaseOptions):
    # Selenium 4.18 reads this attribute for custom desktop capabilities.
    _ignore_local_proxy = True

    @property
    def default_capabilities(self):
        return {}

    def to_capabilities(self):
        return {"tauri:options": {"application": str(Path(os.environ.get("SILO_LINUX_APPLICATION", ROOT / "src-tauri/target/debug/silo-ui")))}}


def stop_test_app(environment):
    """Stop only this test's Silo process after WebDriver closes its window."""
    application = Path(environment.get("SILO_LINUX_APPLICATION", ROOT / "src-tauri/target/debug/silo-ui")).resolve()
    executable_paths = {application}
    if application.name == "AppRun":
        executable_paths.add((application.parent / "usr/bin/silo-ui").resolve())
    expected_xdg = {key: environment[key] for key in ("XDG_CONFIG_HOME", "XDG_DATA_HOME")}

    def owned_process(pid):
        proc = Path("/proc") / str(pid)
        try:
            values = dict(entry.split("=", 1) for entry in proc.joinpath("environ").read_bytes().decode(errors="surrogateescape").split("\0") if "=" in entry)
            executable = proc.joinpath("exe").resolve(strict=True)
            stat = proc.joinpath("stat").read_text().rsplit(")", 1)[1].split()
        except (OSError, ValueError, IndexError):
            return None
        if any(values.get(key) != value for key, value in expected_xdg.items()):
            return None
        if application.suffix == ".AppImage":
            if executable.name != "silo-ui" or Path(values.get("APPIMAGE", "")).resolve() != application:
                return None
        elif executable not in executable_paths:
            return None
        return stat[19]  # Linux /proc stat field 22: process start time.

    for proc in Path("/proc").iterdir():
        if not proc.name.isdecimal():
            continue
        pid = int(proc.name)
        started = owned_process(pid)
        if started is None or owned_process(pid) != started:
            continue
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            continue
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if owned_process(pid) != started:
                break
            try:
                state = proc.joinpath("stat").read_text().rsplit(")", 1)[1].split()[0]
            except OSError:
                break
            if state == "Z":
                break
            time.sleep(.1)
        else:
            raise RuntimeError(f"Smoke app PID {pid} did not exit after SIGTERM")
    # A relaunch that still finds the single-instance D-Bus name exits at once,
    # and WebDriver then waits for a window that never opens.
    identifier = environment.get("SILO_LINUX_APPLICATION_ID", channel_names()["development"]["identifier"])
    name = f"{identifier}.SingleInstance"

    def bus(method):
        result = subprocess.run(["dbus-send", "--session", "--print-reply", "--dest=org.freedesktop.DBus",
                                 "/org/freedesktop/DBus", f"org.freedesktop.DBus.{method}", f"string:{name}"],
                                env=environment, capture_output=True, text=True, timeout=5)
        return result.stdout if result.returncode == 0 else None

    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if "boolean false" in (bus("NameHasOwner") or ""):
            return
        time.sleep(.1)
    owner = (bus("GetConnectionUnixProcessID") or "").split()
    pid = owner[-1] if owner else "unknown"
    try:
        status = (Path("/proc") / pid / "status").read_text()
    except OSError:
        status = "unavailable"
    raise RuntimeError(f"{name} is still owned by PID {pid} after the smoke app stopped:\n{status}")


def run():
    if os.environ.get("SILO_LINUX_LIFECYCLE") == "1":
        return run_lifecycle()
    report = []
    passed = False
    with tempfile.TemporaryDirectory(prefix="silo-linux-ui-") as temporary:
        environment = dict(os.environ)
        environment["HOME"] = str(Path(temporary) / "home")
        Path(environment["HOME"]).mkdir(mode=0o700)
        names = channel_names()
        identifier = environment.get("SILO_LINUX_APPLICATION_ID", names["development"]["identifier"])
        environment["SILO_LINUX_APPLICATION_ID"] = identifier
        for kind in ["CONFIG", "DATA", "CACHE"]:
            directory = Path(temporary) / kind.lower()
            directory.mkdir()
            environment[f"XDG_{kind}_HOME"] = str(directory)
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            native_port = probe.getsockname()[1]
        driver_path = shutil.which("tauri-driver")
        if not driver_path:
            raise RuntimeError("Install tauri-driver 2.0.6 before running this test")
        with (EVIDENCE / "desktop-driver.log").open("w") as output:
            process = subprocess.Popen([driver_path, "--port", str(port), "--native-port", str(native_port)], env=environment,
                                       stdout=output, stderr=subprocess.STDOUT)
            browser = None
            try:
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    try:
                        with socket.create_connection(("127.0.0.1", port), timeout=1):
                            with socket.create_connection(("127.0.0.1", native_port), timeout=1):
                                break
                    except OSError:
                        if process.poll() is not None:
                            raise RuntimeError("tauri-driver exited; inspect desktop-driver.log")
                        time.sleep(.1)
                browser = webdriver.Remote(f"http://127.0.0.1:{port}", options=Options())
                wait = WebDriverWait(browser, 45, ignored_exceptions=(StaleElementReferenceException, ElementClickInterceptedException, ElementNotInteractableException))
                def click(by, value):
                    def attempt(_):
                        node = browser.find_element(by, value)
                        if not node.is_displayed() or not node.is_enabled():
                            return False
                        node.click()
                        return True
                    wait.until(attempt)
                # There are two production WebViews. Choose the actual main window.
                def main_window(_):
                    for handle in browser.window_handles:
                        browser.switch_to.window(handle)
                        if "native-status" not in browser.current_url:
                            return True
                    return False
                wait.until(main_window)
                application = Path(environment.get("SILO_LINUX_APPLICATION", ROOT / "src-tauri/target/debug/silo-ui")).resolve()
                if application.suffix == ".AppImage":
                    def appimage_root(_):
                        for proc in Path("/proc").iterdir():
                            if not proc.name.isdecimal():
                                continue
                            try:
                                values = dict(entry.split("=", 1) for entry in proc.joinpath("environ").read_bytes().decode(errors="surrogateescape").split("\0") if "=" in entry)
                                executable = proc.joinpath("exe").resolve(strict=True)
                                root = Path(values.get("APPDIR", "")).resolve(strict=True)
                            except (OSError, ValueError):
                                continue
                            if executable.name == "silo-ui" and Path(values.get("APPIMAGE", "")).resolve() == application:
                                return root
                        return False

                    appdir = wait.until(appimage_root)
                    tools_dir = appdir / "usr/libexec/silo/tools"
                    required_tools = ("msb", "git", "git-lfs")
                    assert all((tools_dir / tool).is_file() for tool in required_tools), f"APPDIR {appdir} lacks bundled tools"
                    report.append(f"Live AppImage process APPDIR resolves to {appdir}; all three managed tool files are present")
                wait.until(lambda _: "Dependencies" in browser.find_element(By.TAG_NAME, "body").text)
                wait.until(lambda _: "Checking" not in browser.find_element(By.TAG_NAME, "body").text)
                body = browser.find_element(By.TAG_NAME, "body").text
                tools_group = browser.find_element(By.CSS_SELECTOR, "button[aria-label='Bundled tools']")
                assert tools_group.find_elements(By.CSS_SELECTOR, "[aria-label='All checks passed']"), body
                tools_group.click()
                for tool_name in ("MicroSandbox runtime", "Git", "Git LFS"):
                    wait.until(lambda _, name=tool_name: any(
                        node.is_displayed() for node in browser.find_elements(By.XPATH, f"//*[normalize-space()='{name}']")
                    ))
                    item_name = browser.find_element(By.XPATH, f"//*[normalize-space()='{tool_name}']")
                    item_row = item_name.find_element(By.XPATH, "../..")
                    assert item_row.find_elements(By.CSS_SELECTOR, "[aria-label='Checked']"), tool_name
                assert "Applications" in body, body
                assert "Silo could not load" not in body, body
                report.append("Production onboarding loaded through native WebKit and IPC")
                browser.save_screenshot(str(EVIDENCE / "onboarding.png"))
                if not Path("/dev/kvm").exists():
                    assert "KVM" in body, body
                    button = browser.find_element(By.XPATH, "//button[normalize-space()='Continue']")
                    assert not button.is_enabled(), "Missing KVM must never allow onboarding to continue"
                    report.append("Missing KVM is visible and prevents Continue")
                # Error guidance is retained on retry; the page must not crash or lose its route.
                retries = browser.find_elements(By.XPATH, "//button[contains(., 'Check again') or contains(., 'Retry')]")
                if retries:
                    retries[0].click()
                    wait.until(lambda _: "Applications" in browser.find_element(By.TAG_NAME, "body").text)
                    report.append("Dependency retry keeps onboarding visible")
                browser.quit()
                browser = None
                stop_test_app(environment)
                # Persisted fixture is isolated in this test's XDG directory. This
                # exercises the real empty-app state even on hosts without KVM.
                settings = Path(environment["XDG_CONFIG_HOME"]) / identifier / "settings.json"
                settings.parent.mkdir(parents=True, exist_ok=True)
                settings.write_text(json.dumps({"schemaVersion": 1, "settings": {
                    "onboardingComplete": True, "launchAtLogin": False,
                    "startComputersAtLaunch": False,
                }, "onboardingDraft": None}))
                browser = webdriver.Remote(f"http://127.0.0.1:{port}", options=Options())
                wait = WebDriverWait(browser, 45, ignored_exceptions=(StaleElementReferenceException, ElementClickInterceptedException, ElementNotInteractableException))
                wait.until(main_window)
                wait.until(lambda _: browser.find_element(By.ID, "application-nav-settings"))
                for page in ["computers", "github", "secrets", "settings"]:
                    click(By.ID, f"application-nav-{page}")
                    wait.until(lambda _: browser.find_element(By.ID, f"application-panel-{page}").is_displayed())
                    assert "Silo could not load" not in browser.find_element(By.TAG_NAME, "body").text
                    report.append(f"Native {page} page renders and keeps its route")
                click(By.ID, "application-nav-secrets")
                click(By.CSS_SELECTOR, "button[aria-label='Add secret']")
                form = browser.find_element(By.CSS_SELECTOR, "form[aria-label='Add secret']")
                form.find_element(By.CSS_SELECTOR, "button[type='submit']").click()
                invalid = form.find_elements(By.CSS_SELECTOR, "input[aria-invalid='true']")
                assert invalid, "Empty secret must show field-level validation"
                assert form.find_elements(By.CSS_SELECTOR, "[role='alert']"), "Explain invalid fields"
                invalid[0].send_keys(Keys.ESCAPE)
                wait.until(lambda _: not browser.find_elements(By.CSS_SELECTOR, "form[aria-label='Add secret']"))
                report.append("Secret form validates inline and Escape cancels without saving")
                click(By.ID, "application-nav-settings")
                login = browser.find_element(By.CSS_SELECTOR, "button[aria-label='Launch Silo at login']")
                wait.until(lambda _: login.is_enabled())
                login.click()
                entry = Path(environment["XDG_CONFIG_HOME"]) / "autostart" / f"{identifier}.desktop"
                wait.until(lambda _: entry.exists() and "Exec=" in entry.read_text() and "Hidden=true" not in entry.read_text())
                wait.until(lambda _: login.is_enabled() and login.get_attribute("aria-checked") == "true")
                login.click()
                wait.until(lambda _: "Hidden=true" in entry.read_text())
                report.append("Login preference writes and disables a real isolated XDG autostart entry")
                motion = browser.find_element(By.CSS_SELECTOR, "button[aria-label='Reduce motion']")
                previous = motion.get_attribute("aria-checked")
                # The first-run VM image progress card can cover the end of the page.
                motion.send_keys(Keys.SPACE)
                expected = "false" if previous == "true" else "true"
                wait.until(lambda _: motion.get_attribute("aria-checked") == expected)
                wait.until(lambda _: json.loads(settings.read_text())["settings"].get("reduceMotion") == (expected == "true"))
                browser.quit()
                browser = None
                stop_test_app(environment)
                browser = webdriver.Remote(f"http://127.0.0.1:{port}", options=Options())
                wait = WebDriverWait(browser, 45, ignored_exceptions=(StaleElementReferenceException, ElementClickInterceptedException, ElementNotInteractableException))
                wait.until(main_window)
                click(By.ID, "application-nav-settings")
                wait.until(lambda _: browser.find_element(By.CSS_SELECTOR, "button[aria-label='Reduce motion']").get_attribute("aria-checked") == expected)
                report.append("Settings survive a full native application relaunch")
                browser.save_screenshot(str(EVIDENCE / "settings.png"))
                if os.environ.get("SILO_LINUX_DESKTOP_SERVICES") == "gnome":
                    from linux_desktop_services import verify
                    report.extend(verify(browser, wait, environment, EVIDENCE))
                passed = True
            except Exception:
                if browser:
                    browser.save_screenshot(str(EVIDENCE / "failure.png"))
                    (EVIDENCE / "failure.txt").write_text(browser.find_element(By.TAG_NAME, "body").text)
                raise
            finally:
                if browser:
                    browser.quit()
                    stop_test_app(environment)
                process.terminate()
                process.wait(timeout=10)
                (EVIDENCE / "desktop.json").write_text(json.dumps({"passed": passed, "checks": report}, indent=2))
    for assertion in report:
        print("PASS: " + assertion)


def run_lifecycle():
    """Exercise a supplied disposable legacy fixture through the real app UI."""
    required = ["HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "SILO_LINUX_APPLICATION", "SILO_LINUX_COMPUTER_NAME",
                "SILO_LINUX_MSB", "SILO_LINUX_MSB_LIBRARY"]
    missing = [name for name in required if not os.environ.get(name)]
    if missing:
        raise RuntimeError("Lifecycle mode needs explicit fixture paths: " + ", ".join(missing))
    environment = dict(os.environ)
    data_home = Path(environment["XDG_DATA_HOME"]).resolve()
    config_home = Path(environment["XDG_CONFIG_HOME"]).resolve()
    supplied_root = environment.get("SILO_LINUX_FIXTURE_ROOT")
    if supplied_root:
        fixture_root = Path(supplied_root).resolve()
        safe_parent = fixture_root.parent in (Path("/tmp"), Path("/home/siloqa"))
        if not safe_parent or not fixture_root.name.startswith("silo-linux-"):
            raise RuntimeError("SILO_LINUX_FIXTURE_ROOT must be a dedicated /tmp/silo-linux-* or /home/siloqa/silo-linux-* task directory")
    else:
        fixture_roots = [path for path in (data_home, *data_home.parents)
                         if path.parent == Path("/tmp") and path.name.startswith("silo-linux-checkpoint-")]
        if len(fixture_roots) != 1:
            raise RuntimeError("Lifecycle mode accepts only the task-owned /tmp/silo-linux-checkpoint-* fixture")
        fixture_root = fixture_roots[0]
    if not data_home.is_relative_to(fixture_root) or not config_home.is_relative_to(fixture_root):
        raise RuntimeError("Both XDG roots must be inside the same task-owned fixture root")
    in_container = Path("/.dockerenv").exists() or Path("/run/.containerenv").exists()
    expected_lima_hostname = environment.get("SILO_LINUX_TASK_LIMA_HOSTNAME")
    if not in_container and (not expected_lima_hostname or socket.gethostname() != expected_lima_hostname):
        raise RuntimeError("Outside a container, lifecycle mode requires an exact task-owned Lima hostname")
    if not Path(environment.get("HOME", "")).is_absolute():
        raise RuntimeError("The disposable container must provide its normal absolute HOME")
    names = channel_names()
    identifier = environment.get("SILO_LINUX_APPLICATION_ID", names["development"]["identifier"])
    state_dir_name = names["production" if identifier == names["production"]["identifier"] else "development"]["stateDir"]
    app_data = data_home / identifier
    app_config = config_home / identifier
    settings = app_config / "settings.json"
    # The fixture comes from an earlier build, so its inventory may still use the
    # earlier file name; the first launch converts it.
    seeded_inventory = [app_data / "runtime" / name for name in ("computers.json", "machines.json")]
    if not settings.is_file() or not any(path.is_file() for path in seeded_inventory):
        raise RuntimeError("The supplied isolated XDG roots do not contain the seeded settings and legacy computer metadata")
    if not Path(environment["SILO_LINUX_MSB"]).is_file() or not Path(environment["SILO_LINUX_MSB_LIBRARY"]).is_file():
        raise RuntimeError("The supplied bundled MicroSandbox executable or library is missing")
    if not Path(environment["SILO_LINUX_APPLICATION"]).exists():
        raise RuntimeError("SILO_LINUX_APPLICATION must identify the exact packaged application under test")
    name = environment["SILO_LINUX_COMPUTER_NAME"]
    archive_destination = Path(environment.get("SILO_LINUX_ARCHIVE_DESTINATION", fixture_root / "archives")).resolve()
    if not archive_destination.is_relative_to(fixture_root) or not archive_destination.is_dir():
        raise RuntimeError("SILO_LINUX_ARCHIVE_DESTINATION must be an existing directory inside the task fixture")
    # Keep XDG_CACHE_HOME isolated as well, while honoring the fixture's data and
    # configuration roots so the real migration code consumes its seeded state.
    cache_home = fixture_root / "xdg-cache"
    cache_home.mkdir(parents=True, exist_ok=True)
    environment["XDG_CACHE_HOME"] = str(cache_home)
    report = []
    passed = False
    suffix = environment.get("SILO_LINUX_RUN_SUFFIX", secrets.token_hex(4))
    if not suffix or len(suffix) > 8 or not all(character in "0123456789abcdefghijklmnopqrstuvwxyz" for character in suffix):
        raise RuntimeError("SILO_LINUX_RUN_SUFFIX must be 1-8 lowercase letters or digits")
    checkpoint_name = f"linux-lifecycle-v1-{suffix}"
    fork_name = f"linux-fork-{suffix}"
    recovery_name = f"linux-recovery-{suffix}"
    archive_name = f"archive-copy-{suffix}"
    source_only_path = f"/workspace/source-only-{suffix}.txt"
    fork_only_path = f"/workspace/fork-only-{suffix}.txt"
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        native_port = probe.getsockname()[1]
    driver_path = shutil.which("tauri-driver")
    if not driver_path:
        raise RuntimeError("Install tauri-driver 2.0.6 before running this test")
    with (EVIDENCE / "lifecycle-driver.log").open("w") as output:
        process = subprocess.Popen([driver_path, "--port", str(port), "--native-port", str(native_port)], env=environment,
                                   stdout=output, stderr=subprocess.STDOUT)
        browser = None
        try:
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=1):
                        with socket.create_connection(("127.0.0.1", native_port), timeout=1):
                            break
                except OSError:
                    if process.poll() is not None:
                        raise RuntimeError("tauri-driver exited; inspect lifecycle-driver.log")
                    time.sleep(.1)

            def connect():
                instance = webdriver.Remote(f"http://127.0.0.1:{port}", options=Options())
                wait = WebDriverWait(instance, 180, ignored_exceptions=(StaleElementReferenceException, ElementClickInterceptedException, ElementNotInteractableException))
                def main_window(_):
                    for handle in instance.window_handles:
                        instance.switch_to.window(handle)
                        if "native-status" not in instance.current_url:
                            return True
                    return False
                wait.until(main_window)
                return instance, wait, main_window

            def body_text(instance):
                return instance.find_element(By.TAG_NAME, "body").text

            def click(instance, wait, by, value):
                def attempt(_):
                    node = instance.find_element(by, value)
                    instance.execute_script("arguments[0].scrollIntoView({block: 'center', inline: 'nearest'});", node)
                    if not node.is_displayed() or not node.is_enabled():
                        return False
                    node.click()
                    return True
                wait.until(attempt)

            def has_button(instance, label):
                return any(node.is_displayed() and node.is_enabled()
                           for node in instance.find_elements(By.CSS_SELECTOR, f"button[aria-label='{label}']"))

            def ensure_checkpoints_open():
                panel = f"section[aria-label='Checkpoints for {name}']"
                if any(node.is_displayed() for node in browser.find_elements(By.CSS_SELECTOR, panel)):
                    return
                # Checkpoints is a row menu item, rendered only after its menu opens.
                quick = WebDriverWait(browser, 30, ignored_exceptions=(StaleElementReferenceException, ElementClickInterceptedException, ElementNotInteractableException))
                click(browser, quick, By.CSS_SELECTOR, f"button[aria-label='More actions for {name}']")
                click(browser, quick, By.CSS_SELECTOR, f"[role='menuitem'][aria-label='Checkpoints for {name}']")
                quick.until(lambda _: any(node.is_displayed() for node in browser.find_elements(By.CSS_SELECTOR, panel)))

            def phase(message):
                print(f"LIFECYCLE: {message}", flush=True)

            phase("attaching to the seeded legacy app and waiting for migration")
            try:
                browser, wait, main_window = connect()
            except InvalidSessionIdException:
                # Conversion can complete during the driver's first attach.
                time.sleep(.5)
                stop_test_app(environment)
                browser, wait, main_window = connect()
            try:
                wait.until(lambda _: (
                    "Migration stopped" in body_text(browser)
                    or "Updating your computers" in body_text(browser)
                    or "application-nav-computers" in browser.page_source
                ))
                initial_body = body_text(browser)
            except InvalidSessionIdException:
                # The retry may finish conversion and restart before this
                # session observes either its running or completed state.
                initial_body = ""
            if "Migration stopped" in initial_body:
                phase("retrying the preserved failed migration")
                click(browser, wait, By.XPATH, "//button[normalize-space(.)='Retry migration']")
                try:
                    wait.until(lambda _: "Updating your computers" in body_text(browser)
                               or "application-nav-computers" in browser.page_source)
                    phase("migration retry entered its running state")
                except InvalidSessionIdException:
                    phase("migration retry restarted the app before the old session could observe progress")
            # Migration converts the fixture and restarts Silo. Wait for the
            # production overview. Silo intentionally replaces its process after
            # conversion, so the old WebDriver session may disappear with it.
            try:
                if "Updating your computers" in initial_body:
                    browser.save_screenshot(str(EVIDENCE / "lifecycle-migration-gate.png"))
                    report.append("The real migration gate displayed during legacy conversion")
            except InvalidSessionIdException:
                pass
            def migrated_overview(_):
                text = body_text(browser)
                for failure in ("Migration stopped", "Migration needs attention", "Migration status is unavailable",
                                "selected converted runtime does not match"):
                    if failure in text:
                        raise AssertionError(f"Production migration gate failed: {text}")
                return "application-nav-computers" in browser.page_source or "Computers" in text
            try:
                wait.until(migrated_overview)
            except InvalidSessionIdException:
                # The migration's deliberate app restart invalidates the old
                # WebKit session. Start a fresh supported Tauri WebDriver session
                # against the same app and XDG roots, then verify the gate cleared.
                phase("migration restarted Silo; attaching to the new process")
                try:
                    browser.quit()
                except WebDriverException:
                    pass
                stop_test_app(environment)
                browser, wait, main_window = connect()
                wait.until(migrated_overview)
            assert "Migration status is unavailable" not in body_text(browser), body_text(browser)
            assert "Updating your computers" not in body_text(browser), body_text(browser)
            wait.until(lambda _: browser.find_element(By.ID, "application-nav-computers").is_displayed())
            wait.until(lambda _: has_button(browser, f"Start {name}") and "Stopped" in body_text(browser))
            phase("migration complete; source is visible and stopped")
            report.append("Legacy fixture passed the production migration gate and appears stopped in the real computers overview")
            browser.save_screenshot(str(EVIDENCE / "lifecycle-migrated-stopped.png"))
            if os.environ.get("SILO_LINUX_MIGRATION_ONLY") == "1":
                report.append("Migration-only run stopped after confirming the converted source in the production overview")
                passed = True
                return

            def guest_for(target, command, expected=None):
                storage_home = app_data / "runtime-checkpoints-converted/microsandbox"
                runtime_alias = Path(environment["HOME"]) / state_dir_name / hashlib.sha256(os.fsencode(storage_home)).hexdigest()[:12]
                if not runtime_alias.is_dir():
                    raise AssertionError(f"Converted runtime home alias is missing: {runtime_alias}")
                completed = subprocess.run([
                    environment["SILO_LINUX_MSB"], "exec", target, "--no-start", "--no-tty", "--user", "silo",
                    "--env", "USER=silo", "--env", "LOGNAME=silo", "--", "/bin/sh", "-lc", command,
                ], env={**environment, "MSB_HOME": str(runtime_alias),
                       "MSB_PATH": environment["SILO_LINUX_MSB"],
                       "MSB_LIBKRUNFW_PATH": environment["SILO_LINUX_MSB_LIBRARY"],
                       "SILO_GITHUB": '{"version":1,"owners":[]}'},
                   capture_output=True, text=True, timeout=45)
                if completed.returncode != 0:
                    raise AssertionError(f"Bundled CLI guest probe failed ({completed.returncode}): {completed.stderr[-1000:]}")
                if expected is not None:
                    assert completed.stdout.strip() == expected, completed.stdout
                return completed.stdout.strip()

            def guest(command, expected=None):
                return guest_for(name, command, expected)

            def fork_guest(command, expected=None):
                return guest_for(fork_name, command, expected)

            memory_marker = f"/dev/shm/silo-checkpoint-{suffix}"
            baseline_token = f"silo-baseline-{suffix}"
            changed_token = f"silo-changed-{suffix}"

            def assert_live_state(probe, value, pid, token):
                probe(f"test \"$(cat {shlex.quote(memory_marker)})\" = {shlex.quote(value)} && "
                      f"kill -0 {pid} && tr '\\000' ' ' < /proc/{pid}/cmdline | "
                      f"grep -F -- {shlex.quote(token)} >/dev/null && printf live", "live")

            marker = os.environ.get("SILO_LINUX_MARKER_PATH", "/workspace/linux-acceptance-marker.txt")
            baseline_marker = os.environ.get("SILO_LINUX_BASELINE_MARKER", "source-before-checkpoint")

            if checkpoint_id := os.environ.get("SILO_LINUX_RESTORE_SOURCE_CHECKPOINT"):
                # Recover a disposable source through Silo's normal Restore and
                # explicit Start after a failed export exposed a missing ancestor.
                metadata = json.loads((app_data / "runtime-checkpoints-converted/computers.json").read_text())
                matches = [computer for computer in metadata["computers"] if computer["name"] == name]
                assert len(matches) == 1, "Recovery source identity is ambiguous"
                computer_id = matches[0]["id"]
                checkpoint_record = json.loads((app_data / f"runtime-checkpoints-converted/checkpoints/{computer_id}.json").read_text())
                assert any(item["id"] == checkpoint_id and item["scope"] == "full"
                           for item in checkpoint_record["checkpoints"]), "Recovery requires a recorded full checkpoint"
                phase("restoring the stopped source from its verified durable full checkpoint")
                browser.set_script_timeout(240)
                result = browser.execute_async_script(
                    "const done = arguments[arguments.length - 1];"
                    "window.__TAURI_INTERNALS__.invoke('restore_checkpoint',"
                    "{computerId: arguments[0], checkpointId: arguments[1]})"
                    ".then(() => done({ok:true}), error => done({ok:false,error:String(error)}));",
                    computer_id, checkpoint_id,
                )
                assert result["ok"], result
                wait.until(lambda _: has_button(browser, f"Start {name}"))
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {name}']")
                wait.until(lambda _: has_button(browser, f"Stop {name}"))
                guest(f"cat {shlex.quote(marker)}", baseline_marker)
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {name}']")
                wait.until(lambda _: has_button(browser, f"Start {name}"))
                report.append("Supported Restore, explicit Start and Stop recovered the disposable source from a verified full checkpoint")
                passed = True
                return

            def invoke_native(command, arguments):
                browser.set_script_timeout(240)
                result = browser.execute_async_script(
                    "const done = arguments[arguments.length - 1];"
                    "window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1])"
                    ".then(value => done({ok: true, value}), error => done({ok: false, error: String(error)}));",
                    command, arguments,
                )
                assert result["ok"], f"{command}: {result['error']}"
                return result["value"]

            def read_application_state_when_idle():
                deadline = time.monotonic() + 30
                while True:
                    try:
                        return invoke_native("read_application_state", {})
                    except AssertionError as error:
                        if "SILO_SANDBOX_UPDATE_IN_PROGRESS" not in str(error) or time.monotonic() >= deadline:
                            raise
                        time.sleep(.2)

            def wait_backup_result(operation_name, previous_archive_paths=None):
                def completed(_):
                    state = invoke_native("read_backup_state", {})
                    operation = state.get("operation") or {}
                    if operation.get("kind") != "result" or operation.get("operation") != operation_name:
                        return False
                    if operation.get("outcome") != "success":
                        raise AssertionError(f"{operation_name} failed: {operation.get('message')}")
                    if previous_archive_paths is not None:
                        return bool(set(archive_destination.glob("*.silo-backup")) - previous_archive_paths)
                    return True
                WebDriverWait(browser, 240).until(completed)

            def archive_round_trip():
                use_ipc = os.environ.get("SILO_LINUX_ARCHIVE_IPC") == "1"

                phase("exporting a v4 archive through production backup IPC" if use_ipc else "exporting a v4 archive through the production Backup UI")
                click(browser, wait, By.ID, "application-nav-backup")
                wait.until(lambda _: browser.find_element(By.ID, "application-panel-backup").is_displayed())
                old_archives = set(archive_destination.glob("*.silo-backup"))
                click(browser, wait, By.XPATH, "//button[normalize-space()='Create backup…']")
                labels = browser.find_elements(By.XPATH, "//h3[normalize-space()='Create backup']/ancestor::li[1]//label[button[@role='checkbox']]")
                assert labels, "The production Backup page did not list any selectable computers"
                assert sum(label.text.strip().startswith(name) for label in labels) == 1, [label.text for label in labels]
                for label in labels:
                    checkbox = label.find_element(By.CSS_SELECTOR, "button[role='checkbox']")
                    selected = label.text.strip().startswith(name)
                    if (checkbox.get_attribute("aria-checked") == "true") != selected:
                        checkbox.click()
                wait.until(lambda _: sum(label.find_element(By.CSS_SELECTOR, "button[role='checkbox']").get_attribute("aria-checked") == "true" for label in labels) == 1)
                if use_ipc:
                    backup_state = invoke_native("read_backup_state", {})
                    assert backup_state["destination"] == str(archive_destination), backup_state
                    invoke_native("start_backup", {"destination": str(archive_destination), "computers": [name]})
                else:
                    click(browser, wait, By.XPATH, "//button[normalize-space()='Change…']")
                    choose_native_path(archive_destination, "Choose a backup destination")
                    wait.until(lambda _: browser.find_element(By.CSS_SELECTOR, "input[aria-label='Destination']").get_attribute("value") == str(archive_destination))
                    click(browser, wait, By.XPATH, "//button[normalize-space()='Review backup']")
                    click(browser, wait, By.XPATH, "//button[normalize-space()='Start backup']")
                WebDriverWait(browser, 240).until(lambda _: "Backup completed successfully" in body_text(browser))
                archives = sorted(set(archive_destination.glob("*.silo-backup")) - old_archives)
                assert archives, f"No v4 archive was written to {archive_destination}"
                with archives[0].open("rb") as package:
                    assert package.read(16) == b"SILO-BACKUP\0\0\0\0\0", "Backup magic differs from the v4 format"
                    assert int.from_bytes(package.read(4), "big") == 4, "Backup is not format v4"
                    manifest_size = int.from_bytes(package.read(8), "big")
                    assert 0 < manifest_size <= 1024 * 1024, "Backup manifest length is invalid"
                    manifest = json.loads(package.read(manifest_size))
                    assert manifest["schemaVersion"] == 4, manifest
                    assert [item["name"] for item in manifest["computers"]] == [name], manifest
                report.append(("Production backup command" if use_ipc else "Production Backup UI") + " exported a v4 archive containing only the migrated source")

                phase("importing the archive through production backup IPC" if use_ipc else "importing the archive through the production Backup UI")
                click(browser, wait, By.ID, "application-nav-computers")
                wait.until(lambda _: browser.find_element(By.ID, "application-panel-computers").is_displayed())
                click(browser, wait, By.ID, "application-nav-backup")
                if use_ipc:
                    inspected = invoke_native("inspect_backup_archive", {"archivePath": str(archives[0])})
                    assert inspected["valid"] and inspected["archive"]["computers"] == [name], inspected
                    invoke_native("start_restore", {"archivePath": str(archives[0]), "newName": archive_name, "sourceName": name})
                else:
                    click(browser, wait, By.XPATH, "//button[normalize-space()='Choose backup…']")
                    choose_native_path(archives[0], "Choose a Silo backup")
                    wait.until(lambda _: "Backup validated" in body_text(browser))
                    name_input = browser.find_element(By.XPATH, "//label[contains(., 'New computer name')]/input")
                    name_input.clear()
                    name_input.send_keys(archive_name)
                    click(browser, wait, By.XPATH, "//button[normalize-space()='Restore new computer']")
                WebDriverWait(browser, 240).until(lambda _: "Computer restored successfully" in body_text(browser))
                click(browser, wait, By.ID, "application-nav-computers")
                wait.until(lambda _: has_button(browser, f"Start {name}") and has_button(browser, f"Start {archive_name}"))
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {archive_name}']")
                wait.until(lambda _: has_button(browser, f"Stop {archive_name}"))
                archive_guest = lambda command, expected=None: guest_for(archive_name, command, expected)
                archive_guest(f"test \"$(cat {shlex.quote(marker)})\" = {shlex.quote(baseline_marker)} && "
                              f"test ! -e {shlex.quote(source_only_path)} && "
                              f"test ! -e {shlex.quote(fork_only_path)} && echo archive-ok", "archive-ok")
                report.append("Imported v4 archive started with original workspace bytes and no post-checkpoint files")
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {archive_name}']")
                wait.until(lambda _: has_button(browser, f"Start {archive_name}"))
                if use_ipc:
                    phase("exporting the source again after the first native capture")
                    previous = set(archive_destination.glob("*.silo-backup"))
                    invoke_native("start_backup", {"destination": str(archive_destination), "computers": [name]})
                    def second_export_done(_):
                        state = invoke_native("read_backup_state", {})
                        operation = state.get("operation") or {}
                        if operation.get("kind") == "result" and operation.get("outcome") == "failed":
                            raise AssertionError(f"Second export failed: {operation.get('message')}")
                        return (operation.get("kind") == "result" and operation.get("outcome") == "success"
                                and bool(set(archive_destination.glob("*.silo-backup")) - previous))
                    WebDriverWait(browser, 240).until(second_export_done)
                    report.append("A second source export completed after the first native capture, preserving its snapshot ancestry")
                    click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {name}']")
                    wait.until(lambda _: has_button(browser, f"Stop {name}"))
                    guest(f"cat {shlex.quote(marker)}", baseline_marker)
                    click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {name}']")
                    wait.until(lambda _: has_button(browser, f"Start {name}"))
                    report.append("The source still started with its original workspace bytes after repeated exports")
                browser.save_screenshot(str(EVIDENCE / "lifecycle-complete.png"))

            if os.environ.get("SILO_LINUX_RESUME_IMPORTED") == "1":
                phase("starting the already imported stopped computer from its disk snapshot")
                wait.until(lambda _: has_button(browser, f"Start {archive_name}"))
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {archive_name}']")
                wait.until(lambda _: has_button(browser, f"Stop {archive_name}"))
                guest_for(archive_name, f"test \"$(cat {shlex.quote(marker)})\" = {shlex.quote(baseline_marker)} && "
                          f"test ! -e {shlex.quote(source_only_path)} && "
                          f"test ! -e {shlex.quote(fork_only_path)} && echo archive-ok", "archive-ok")
                report.append("Imported v4 archive started from its disk snapshot with original workspace bytes")
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {archive_name}']")
                wait.until(lambda _: has_button(browser, f"Start {archive_name}"))
                phase("repeating source export after the retained native capture")
                previous = set(archive_destination.glob("*.silo-backup"))
                invoke_native("start_backup", {"destination": str(archive_destination), "computers": [name]})
                wait_backup_result("backup", previous)
                report.append("Repeated source export succeeded with native snapshot ancestry intact")
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {name}']")
                wait.until(lambda _: has_button(browser, f"Stop {name}"))
                guest(f"cat {shlex.quote(marker)}", baseline_marker)
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {name}']")
                wait.until(lambda _: has_button(browser, f"Start {name}"))
                report.append("Source still starts with original workspace bytes after repeated exports")
                browser.save_screenshot(str(EVIDENCE / "lifecycle-complete.png"))
                passed = True
                return

            if existing_archive := os.environ.get("SILO_LINUX_IMPORT_EXISTING_ARCHIVE"):
                archive = Path(existing_archive).resolve(strict=True)
                assert archive.is_relative_to(fixture_root) and archive.suffix == ".silo-backup"
                with archive.open("rb") as package:
                    assert package.read(16) == b"SILO-BACKUP\0\0\0\0\0"
                    assert int.from_bytes(package.read(4), "big") == 4
                    manifest_size = int.from_bytes(package.read(8), "big")
                    assert 0 < manifest_size <= 1024 * 1024
                    assert [item["name"] for item in json.loads(package.read(manifest_size))["computers"]] == [name]
                phase("importing the preserved verified v4 archive through production backup IPC")
                click(browser, wait, By.ID, "application-nav-backup")
                inspected = invoke_native("inspect_backup_archive", {"archivePath": str(archive)})
                assert inspected["valid"] and inspected["archive"]["computers"] == [name], inspected
                invoke_native("start_restore", {"archivePath": str(archive), "newName": archive_name, "sourceName": name})
                wait_backup_result("restore")
                click(browser, wait, By.ID, "application-nav-computers")
                wait.until(lambda _: has_button(browser, f"Start {archive_name}"))
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {archive_name}']")
                wait.until(lambda _: has_button(browser, f"Stop {archive_name}"))
                guest_for(archive_name, f"test \"$(cat {shlex.quote(marker)})\" = {shlex.quote(baseline_marker)} && "
                          f"test ! -e {shlex.quote(source_only_path)} && "
                          f"test ! -e {shlex.quote(fork_only_path)} && echo archive-ok", "archive-ok")
                report.append("The preserved v4 archive imported as a stopped computer and started with original workspace bytes")
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {archive_name}']")
                wait.until(lambda _: has_button(browser, f"Start {archive_name}"))
                phase("repeating source export to verify native snapshot ancestry remains available")
                previous = set(archive_destination.glob("*.silo-backup"))
                invoke_native("start_backup", {"destination": str(archive_destination), "computers": [name]})
                wait_backup_result("backup", previous)
                report.append("A second source export completed after the first durable native capture")
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {name}']")
                wait.until(lambda _: has_button(browser, f"Stop {name}"))
                guest(f"cat {shlex.quote(marker)}", baseline_marker)
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {name}']")
                wait.until(lambda _: has_button(browser, f"Start {name}"))
                report.append("Source still starts with original workspace bytes after repeated exports")
                browser.save_screenshot(str(EVIDENCE / "lifecycle-complete.png"))
                passed = True
                return

            if os.environ.get("SILO_LINUX_ARCHIVE_ONLY") == "1":
                phase("continuing with archive-only verification of the stopped source")
                archive_round_trip()
                passed = True
                return

            if os.environ.get("SILO_LINUX_RECOVERY_FORK_ONLY") == "1":
                phase("forking the recorded recovery checkpoint while its source runtime is absent")
                ensure_checkpoints_open()
                recovery_row = "//ol[@aria-label='Checkpoint history']/li[contains(., 'Recovery')]"
                click(browser, wait, By.XPATH, recovery_row + "[1]//button[normalize-space()='Fork']")
                browser.find_element(By.CSS_SELECTOR, "input[aria-label='Fork name']").send_keys(recovery_name)
                click(browser, wait, By.XPATH, "//button[normalize-space()='Create stopped fork']")
                wait.until(lambda _: has_button(browser, f"Start {recovery_name}") and
                           browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{recovery_name}']").is_displayed())
                report.append("Recovery checkpoint fork was created while the pending-restore source runtime was absent")
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {recovery_name}']")
                wait.until(lambda _: has_button(browser, f"Stop {recovery_name}"))
                recovery_guest = lambda command, expected=None: guest_for(recovery_name, command, expected)
                recovery_guest(f"cat {shlex.quote(marker)} && test -e {shlex.quote(source_only_path)} && echo source-only-ok",
                               "source-diverged\nsource-only-ok")
                recovery_guest(f"test ! -e {shlex.quote(fork_only_path)} && echo isolated", "isolated")
                report.append("Recovery fork retained the pre-restore workspace and source-only changes")
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {recovery_name}']")
                wait.until(lambda _: has_button(browser, f"Start {recovery_name}"))
                browser.save_screenshot(str(EVIDENCE / "lifecycle-recovery-fork-only.png"))
                passed = True
                return
            phase("starting the migrated source explicitly and checking its marker")
            click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {name}']")
            wait.until(lambda _: has_button(browser, f"Stop {name}"))
            assert guest(f"cat {shlex.quote(marker)}") == baseline_marker
            report.append("Explicit Start restored the migrated stopped computer and its workspace sentinel")
            baseline_pid = int(guest(f"printf baseline > {shlex.quote(memory_marker)}; "
                                     f"nohup sh -c 'while :; do sleep 1; done' {shlex.quote(baseline_token)} "
                                     "</dev/null >/dev/null 2>&1 & echo $!"))
            assert_live_state(guest, "baseline", baseline_pid, baseline_token)

            phase("opening the checkpoint panel in the production UI")
            ensure_checkpoints_open()
            phase("creating a full checkpoint in the production UI")
            checkpoint = browser.find_element(By.CSS_SELECTOR, "input[aria-label='Checkpoint name']")
            checkpoint.send_keys(checkpoint_name)
            click(browser, wait, By.XPATH, "//button[normalize-space()='Create checkpoint']")
            checkpoint_row = f"//ol[@aria-label='Checkpoint history']/li[contains(., '{checkpoint_name}')]"
            wait.until(lambda _: any(node.is_displayed() for node in browser.find_elements(By.XPATH, checkpoint_row)))
            report.append("Production checkpoint panel created and listed a full checkpoint")

            phase("forking the checkpoint and checking stopped-before-start behavior")
            before_fork = read_application_state_when_idle()
            source_matches = [computer for computer in before_fork["computers"]
                              if computer["configuration"]["name"] == name]
            assert len(source_matches) == 1, "The source computer identity is ambiguous before fork"
            source_computer = source_matches[0]
            assert not any(computer["configuration"]["name"] == fork_name for computer in before_fork["computers"]), \
                f"Fork name {fork_name} already exists; refusing to reuse a stale child"
            selected_checkpoint = next((item for item in source_computer.get("checkpoints", [])
                                        if item["name"] == checkpoint_name), None)
            assert selected_checkpoint is not None, "The selected full checkpoint is missing from source state"
            source_id = source_computer["configuration"]["id"]
            click(browser, wait, By.XPATH, checkpoint_row + "//button[normalize-space()='Fork']")
            browser.find_element(By.CSS_SELECTOR, "input[aria-label='Fork name']").send_keys(fork_name)
            click(browser, wait, By.XPATH, "//button[normalize-space()='Create stopped fork']")
            wait.until(lambda _: has_button(browser, f"Start {fork_name}") and
                       browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{fork_name}']").is_displayed())
            after_fork = read_application_state_when_idle()
            child_matches = [computer for computer in after_fork["computers"]
                             if computer["configuration"]["name"] == fork_name]
            assert len(child_matches) == 1, "The new stopped fork is missing or ambiguous"
            child = child_matches[0]
            pending = child.get("pendingCheckpointRestore")
            assert child["configuration"]["id"] != source_id, "Fork reused the source identity"
            assert child.get("state", "").casefold() == "stopped", "Fork did not remain stopped"
            assert pending and pending["checkpointId"] == selected_checkpoint["id"] and pending["state"] == "full", \
                f"Fork does not point at the newly selected full checkpoint: {pending}"
            report.append("Checkpoint fork was created and remained stopped until an explicit Start")
            phase("starting the fork and checking source/fork writable-disk independence")
            click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {fork_name}']")
            wait.until(lambda _: has_button(browser, f"Stop {fork_name}"))
            assert guest(f"cat {shlex.quote(marker)}") == baseline_marker
            assert_live_state(fork_guest, "baseline", baseline_pid, baseline_token)
            guest(f"printf '%s\\n' source-only > {shlex.quote(source_only_path)} && printf '%s\\n' source-diverged > {shlex.quote(marker)}")
            fork_guest(f"printf '%s\\n' fork-only > {shlex.quote(fork_only_path)} && printf '%s\\n' fork-diverged > {shlex.quote(marker)}")

            assert guest(f"test ! -e {shlex.quote(fork_only_path)} && cat {shlex.quote(marker)} && test -e {shlex.quote(source_only_path)} && echo source-only-ok", "source-diverged\nsource-only-ok")
            assert fork_guest(f"test ! -e {shlex.quote(source_only_path)} && cat {shlex.quote(marker)} && test -e {shlex.quote(fork_only_path)} && echo fork-only-ok", "fork-diverged\nfork-only-ok")
            report.append("Source and fork writable disks are independent for both copy-up and newly created files")
            report.append("Full checkpoint fork resumed the captured RAM marker and identifiable guest process")

            changed_pid = int(guest(f"printf changed > {shlex.quote(memory_marker)}; kill {baseline_pid}; "
                                    f"nohup sh -c 'while :; do sleep 1; done' {shlex.quote(changed_token)} "
                                    "</dev/null >/dev/null 2>&1 & echo $!"))
            assert_live_state(guest, "changed", changed_pid, changed_token)

            # Mutate only source after the checkpoint; restore must roll this back
            # and create an independently inspectable pre-restore recovery point.
            phase("stopping the fork and restoring the source with a recovery checkpoint")
            click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {fork_name}']")
            wait.until(lambda _: has_button(browser, f"Start {fork_name}"))
            ensure_checkpoints_open()
            click(browser, wait, By.XPATH, checkpoint_row + "//button[normalize-space()='Restore']")
            click(browser, wait, By.XPATH, "//button[normalize-space()='Save recovery point and restore']")
            wait.until(lambda _: has_button(browser, f"Start {name}") and any(
                row.is_displayed() for row in browser.find_elements(By.XPATH, "//ol[@aria-label='Checkpoint history']/li[contains(., 'Recovery')]")))
            report.append("Restore saved a recovery checkpoint and left the source stopped")
            phase("relaunching the app to verify stopped pending restore survives restart")
            browser.quit()
            browser = None
            stop_test_app(environment)
            browser, wait, main_window = connect()
            wait.until(lambda _: browser.find_element(By.ID, "application-nav-computers").is_displayed())
            assert "Migration" not in body_text(browser), body_text(browser)
            wait.until(lambda _: has_button(browser, f"Start {name}") and has_button(browser, f"Start {fork_name}"))
            report.append("Full app relaunch kept the converted generation and both computers stopped; migration did not rerun")
            phase("forking the recovery checkpoint to verify the pre-restore state")
            ensure_checkpoints_open()
            recovery_row = "//ol[@aria-label='Checkpoint history']/li[contains(., 'Recovery')]"
            click(browser, wait, By.XPATH, recovery_row + "[1]//button[normalize-space()='Fork']")
            browser.find_element(By.CSS_SELECTOR, "input[aria-label='Fork name']").send_keys(recovery_name)
            click(browser, wait, By.XPATH, "//button[normalize-space()='Create stopped fork']")
            wait.until(lambda _: has_button(browser, f"Start {recovery_name}"))
            click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {recovery_name}']")
            wait.until(lambda _: has_button(browser, f"Stop {recovery_name}"))
            recovery_guest = lambda command, expected=None: guest_for(recovery_name, command, expected)
            recovery_guest(f"cat {shlex.quote(marker)} && test -e {shlex.quote(source_only_path)} && echo source-only-ok", "source-diverged\nsource-only-ok")
            recovery_guest(f"test ! -e {shlex.quote(fork_only_path)} && echo isolated", "isolated")
            assert_live_state(recovery_guest, "changed", changed_pid, changed_token)
            report.append("Recovery checkpoint fork resumed the pre-restore disk, RAM, and guest process state")
            click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {recovery_name}']")
            try:
                WebDriverWait(browser, 30).until(lambda _: has_button(browser, f"Start {recovery_name}"))
            except TimeoutException:
                if "Stop failed: Another computer operation is still running." not in body_text(browser):
                    raise
                # The restored guest can answer probes before another native
                # computer operation releases the global lifecycle lock.
                click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {recovery_name}']")
                wait.until(lambda _: has_button(browser, f"Start {recovery_name}"))
                report.append("Recovery fork Stop succeeded after the runtime's transient busy response")
            phase("explicitly starting the restored source and verifying rollback bytes")
            click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {name}']")
            wait.until(lambda _: has_button(browser, f"Stop {name}"))
            assert guest(f"cat {shlex.quote(marker)}") == baseline_marker
            assert guest(f"test ! -e {shlex.quote(source_only_path)} && echo rollback-ok", "rollback-ok")
            assert_live_state(guest, "baseline", baseline_pid, baseline_token)
            report.append("Explicit Start after relaunch restored disk bytes, RAM marker, and original guest process")
            click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {name}']")
            wait.until(lambda _: has_button(browser, f"Start {name}"))

            archive_round_trip()
            passed = True
        except Exception as failure:
            if browser:
                try:
                    browser.save_screenshot(str(EVIDENCE / "lifecycle-failure.png"))
                    (EVIDENCE / "lifecycle-failure.txt").write_text(body_text(browser))
                    controls = []
                    for node in browser.find_elements(By.CSS_SELECTOR, "button[aria-label^='Start '], button[aria-label^='Stop '], button[aria-label^='More actions for ']"):
                        controls.append({"label": node.get_attribute("aria-label"), "displayed": node.is_displayed(),
                                         "enabled": node.is_enabled(), "disabled": node.get_attribute("disabled"),
                                         "html": node.get_attribute("outerHTML")[:1500]})
                    (EVIDENCE / "lifecycle-controls.json").write_text(json.dumps({"error": repr(failure), "controls": controls}, indent=2))
                except Exception:
                    pass
            raise
        finally:
            cleanup_errors = []
            if browser:
                for computer_name in (name, fork_name, recovery_name, archive_name):
                    try:
                        stops = browser.find_elements(By.CSS_SELECTOR, f"button[aria-label='Stop {computer_name}']")
                        if stops and stops[0].is_displayed() and stops[0].is_enabled():
                            stops[0].click()
                            WebDriverWait(browser, 45).until(lambda _: any(node.is_displayed() for node in browser.find_elements(By.CSS_SELECTOR, f"button[aria-label='Start {computer_name}']")))
                    except Exception as error:
                        cleanup_errors.append(f"{computer_name}: {error}")
                browser.quit()
            stop_test_app(environment)
            process.terminate()
            process.wait(timeout=10)
            if cleanup_errors:
                passed = False
                report.append("Cleanup failed: " + "; ".join(cleanup_errors))
            (EVIDENCE / "lifecycle.json").write_text(json.dumps({"passed": passed, "checks": report}, indent=2))
            if cleanup_errors:
                raise AssertionError("Lifecycle test could not stop all fixture computers: " + "; ".join(cleanup_errors))
    for assertion in report:
        print("PASS: " + assertion)


def choose_native_path(path, title):
    """Enter a path in the real GTK chooser opened by the production app."""
    xdotool = shutil.which("xdotool")
    if not xdotool:
        raise RuntimeError("Archive lifecycle needs xdotool to enter paths in the real GTK file chooser")
    found = subprocess.run([xdotool, "search", "--sync", "--onlyvisible", "--name", title],
                           check=True, timeout=20, capture_output=True, text=True)
    window = found.stdout.splitlines()[-1]
    subprocess.run([xdotool, "windowfocus", "--sync", window], check=True, timeout=5)
    subprocess.run([xdotool, "key", "--clearmodifiers", "ctrl+l"], check=True, timeout=5)
    subprocess.run([xdotool, "type", "--clearmodifiers", "--delay", "1", str(path)], check=True, timeout=10)
    subprocess.run([xdotool, "key", "--clearmodifiers", "Return"], check=True, timeout=5)
    if path.is_dir():
        # GTK first accepts the location entry and navigates into the folder.
        # A second Return confirms that folder in the native picker.
        time.sleep(.3)
        still_open = subprocess.run([xdotool, "search", "--onlyvisible", "--name", title], check=False,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if still_open.returncode == 0:
            subprocess.run([xdotool, "key", "--clearmodifiers", "Return"], check=True, timeout=5)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        result = subprocess.run([xdotool, "search", "--onlyvisible", "--name", title], check=False,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if result.returncode != 0:
            return
        time.sleep(.1)
    raise AssertionError(f"The real GTK chooser did not close after selecting {path}")


if __name__ == "__main__":
    run()
