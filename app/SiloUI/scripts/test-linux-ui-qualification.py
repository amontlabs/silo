#!/usr/bin/env python3
"""Run bounded, production-WebKit Linux UI qualification against a task-owned app.

This path uses the packaged Tauri app, its real IPC, and the GTK file chooser.
It deliberately keeps setup external: callers provide isolated HOME/XDG roots,
the exact app build, and bundled runtime paths for the matching Linux guest.
"""
import hashlib
import json
import os
from pathlib import Path
from channel_names import channel_for_identifier
import secrets
import shlex
import signal
import shutil
import socket
import subprocess
import time

from selenium import webdriver
from selenium.common.exceptions import ElementClickInterceptedException, ElementNotInteractableException, StaleElementReferenceException
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.options import BaseOptions
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.support.ui import Select, WebDriverWait


class Options(BaseOptions):
    _ignore_local_proxy = True

    @property
    def default_capabilities(self):
        return {}

    def to_capabilities(self):
        return {"tauri:options": {"application": str(Path(os.environ["SILO_LINUX_APPLICATION"]).resolve())}}


def require_path(name, *, directory=False):
    raw = os.environ.get(name)
    if not raw:
        raise RuntimeError(f"Set {name} to the task-owned qualification path")
    path = Path(raw).resolve()
    if directory and not path.is_dir():
        raise RuntimeError(f"{name} must be an existing directory: {path}")
    if not directory and not path.exists():
        raise RuntimeError(f"{name} does not exist: {path}")
    return path


def under(path, parent):
    try:
        path.relative_to(parent)
        return True
    except ValueError:
        return False


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def prepare_environment():
    root = require_path("SILO_LINUX_UI_ROOT", directory=True)
    if not root.name.startswith("silo-linux-ui-qualification-"):
        raise RuntimeError("SILO_LINUX_UI_ROOT must be a task-owned silo-linux-ui-qualification-* directory")
    app = require_path("SILO_LINUX_APPLICATION")
    if app.name != "AppRun" and app.suffix != ".AppImage":
        raise RuntimeError("SILO_LINUX_APPLICATION must be the exact AppRun or AppImage under test")
    if not under(app, root):
        raise RuntimeError("SILO_LINUX_APPLICATION must be inside the isolated task root")
    expected_hash = os.environ.get("SILO_LINUX_APPLICATION_SHA256")
    if not expected_hash or sha256_file(app) != expected_hash:
        raise RuntimeError("Set SILO_LINUX_APPLICATION_SHA256 to the verified exact build hash")
    package = require_path("SILO_LINUX_APPIMAGE")
    if not under(package, root):
        raise RuntimeError("SILO_LINUX_APPIMAGE must be inside the isolated task root")
    package_hash = os.environ.get("SILO_LINUX_APPIMAGE_SHA256")
    if not package_hash or sha256_file(package) != package_hash:
        raise RuntimeError("Set SILO_LINUX_APPIMAGE_SHA256 to the verified package hash")
    home = require_path("HOME", directory=True)
    if not under(home, root):
        raise RuntimeError("HOME must be inside the task root")
    for key in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"):
        value = os.environ.get(key)
        if not value:
            raise RuntimeError(f"Set isolated {key} inside the task root")
        path = Path(value).resolve()
        if not under(path, root):
            raise RuntimeError(f"{key} must be inside the task root")
        path.mkdir(parents=True, exist_ok=True)
        if key == "XDG_RUNTIME_DIR":
            path.chmod(0o700)
    app_id = os.environ.get("SILO_LINUX_APPLICATION_ID", "")
    if not app_id or any(char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-" for char in app_id):
        raise RuntimeError("Set SILO_LINUX_APPLICATION_ID to the packaged app's exact identifier")
    evidence = root / "evidence"
    evidence.mkdir(exist_ok=True)
    app_data = Path(os.environ["XDG_DATA_HOME"]) / app_id
    computer_name = os.environ.get("SILO_LINUX_COMPUTER_NAME", "linux-legacy-source")
    if app_data.exists() and any(app_data.iterdir()):
        if os.environ.get("SILO_LINUX_REUSE_FIXTURE") != "1":
            raise RuntimeError("Use a fresh task-owned XDG data root; existing migration/runtime state must not be replaced")
        runtime = app_data / "runtime"
        operation = runtime / "configuration-operation.json"
        # A reused fixture may predate the first launch that renames its files and keys.
        inventory = next((runtime / name for name in ("computers.json", "machines.json") if (runtime / name).is_file()), None)
        if operation.is_file():
            request = json.loads(operation.read_text()).get("request", {})
            computers = request.get("computers", request.get("machines", []))
        elif inventory is not None:
            saved = json.loads(inventory.read_text())
            computers = saved.get("computers", saved.get("machines", []))
        else:
            computers = []
        sources = [item for item in computers if item.get("name") == computer_name]
        if len(sources) != 1:
            raise RuntimeError("Refusing fixture reuse unless task state contains exactly one expected source")
        computer = sources[0]
        expected = {"name": computer_name, "cpus": 2, "maxCPUs": 2, "memoryGiB": 2,
                    "maxMemoryGiB": 2, "workspaceStorageGiB": 20, "runtimeStorageGiB": 40}
        if any(computer.get(key) != value for key, value in expected.items()) or not computer.get("desktop"):
            raise RuntimeError("Refusing to reuse task state that differs from the expected desktop source")
    settings = Path(os.environ["XDG_CONFIG_HOME"]) / app_id / "settings.json"
    settings.parent.mkdir(parents=True, exist_ok=True)
    settings.write_text(json.dumps({"schemaVersion": 1, "settings": {
        "onboardingComplete": True, "launchAtLogin": False, "startComputersAtLaunch": False,
    }, "onboardingDraft": None}))
    return root, evidence, app_id


def preflight_native_chooser():
    import pyatspi  # noqa: F401
    import gi

    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk  # noqa: F401


def click(browser, wait, by, selector):
    def attempt(_):
        node = browser.find_element(by, selector)
        browser.execute_script("arguments[0].scrollIntoView({block:'center'});", node)
        if not node.is_displayed() or not node.is_enabled():
            return False
        node.click()
        return True
    wait.until(attempt)


def app_window(browser, wait):
    def ready(_):
        for handle in browser.window_handles:
            browser.switch_to.window(handle)
            if "native-status" not in browser.current_url and browser.find_elements(By.ID, "application-nav-computers"):
                return True
        return False
    wait.until(ready)


def start_driver(environment, evidence):
    driver_path = shutil.which("tauri-driver")
    if not driver_path:
        raise RuntimeError("Install tauri-driver 2.0.6 in the task-owned environment")
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        native_port = probe.getsockname()[1]
    log = (evidence / "tauri-driver.log").open("w")
    process = subprocess.Popen([driver_path, "--port", str(port), "--native-port", str(native_port)],
                               env=environment, stdout=log, stderr=subprocess.STDOUT)
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=1):
                with socket.create_connection(("127.0.0.1", native_port), timeout=1):
                    browser = webdriver.Remote(f"http://127.0.0.1:{port}", options=Options())
                    return process, log, browser
        except OSError:
            if process.poll() is not None:
                raise RuntimeError("tauri-driver exited; inspect evidence/tauri-driver.log")
            time.sleep(.2)
    process.terminate()
    process.wait(timeout=10)
    raise RuntimeError("tauri-driver did not open both WebDriver ports")


def native_tree(root):
    yield root
    for child in root:
        if child is not None:
            yield from native_tree(child)


def native_picker(browser, wait, evidence, destination, name):
    import pyatspi
    import gi

    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk

    def find_dialog():
        for app in pyatspi.Registry.getDesktop(0):
            try:
                for node in native_tree(app):
                    if node.getRoleName() in ("dialog", "file chooser") and node.name == "Choose where to export":
                        return node
            except Exception:
                continue
        return None

    def screenshot(path):
        window = Gdk.get_default_root_window()
        pixbuf = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
        if pixbuf is None:
            raise RuntimeError("Could not capture the actual native chooser screen")
        pixbuf.savev(str(path), "png", [], [])

    def action(node):
        try:
            actions = node.queryAction()
            for index in range(actions.nActions):
                if actions.getName(index) in ("click", "press", "activate", "select"):
                    if actions.doAction(index):
                        return True
        except NotImplementedError:
            # GTK exposes Places rows as list items without Action. Click only
            # the row's AT-SPI-reported screen bounds, never a guessed offset.
            bounds = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
            x = bounds.x + bounds.width // 2
            y = bounds.y + bounds.height // 2
            subprocess.run(["xdotool", "mousemove", "--sync", str(x), str(y), "click", "1"], check=True)
            return True
        return False

    click(browser, wait, By.ID, "application-nav-computers")
    wait.until(lambda _: browser.find_element(By.ID, "application-panel-computers").is_displayed())
    click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='More actions for {name}']")
    click(browser, wait, By.CSS_SELECTOR, f"[role='menuitem'][aria-label='Export {name}']")
    wait.until(lambda _: find_dialog() is not None)
    dialog = find_dialog()
    screenshot(evidence / "native-folder-picker-before.png")
    tree_dump = []
    for node in native_tree(dialog):
        try:
            tree_dump.append({"role": node.getRoleName(), "name": node.name,
                              "showing": node.getState().contains(pyatspi.STATE_SHOWING)})
        except Exception:
            continue
    (evidence / "native-folder-picker-atspi.json").write_text(json.dumps(tree_dump, indent=2))

    # The chooser opens at Recent. The captured accessibility tree exposes an
    # actual Home place, so navigate there and select the destination row from
    # its parent instead of typing a guessed path into the native dialog.
    home = next((node for node in native_tree(dialog)
                 if node.getRoleName() == "label" and node.name == "Home"), None)
    while home is not None and home.getRoleName() != "list item":
        home = home.parent
    if home is None or not action(home):
        screenshot(evidence / "native-folder-picker-home-unavailable.png")
        raise AssertionError("GTK chooser did not expose an activatable Home place")
    row = None
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        dialog = find_dialog()
        if dialog is None:
            break
        for node in native_tree(dialog):
            try:
                if (node.name == destination.name
                        and node.getState().contains(pyatspi.STATE_SHOWING)):
                    row = node
                    break
            except Exception:
                continue
        if row:
            break
        time.sleep(.25)
    if row is None:
        visible_tree = []
        dialog = find_dialog()
        if dialog is not None:
            for node in native_tree(dialog):
                try:
                    if node.getState().contains(pyatspi.STATE_SHOWING):
                        visible_tree.append({"role": node.getRoleName(), "name": node.name})
                except Exception:
                    continue
            (evidence / "native-folder-picker-home-atspi.json").write_text(json.dumps(visible_tree, indent=2))
        screenshot(evidence / "native-folder-picker-row-missing.png")
        raise AssertionError(f"GTK chooser did not expose selected directory row {destination.name!r}; inspect saved AT-SPI tree and screenshot")
    if not action(row):
        row.queryComponent().grabFocus()
        subprocess.run(["xdotool", "key", "space"], check=True)
    screenshot(evidence / "native-folder-picker-row-selected.png")
    dialog = find_dialog()
    accept = next((node for node in native_tree(dialog)
                   if node.getRoleName() == "push button" and node.name in ("Open", "_Open", "Select", "_Select")), None)
    if accept is None or not action(accept):
        screenshot(evidence / "native-folder-picker-accept-missing.png")
        raise AssertionError("GTK chooser had no activatable Open/Select control after the existing row was selected")
    wait.until(lambda _: find_dialog() is None)
    canonical = str(destination.resolve(strict=True))
    data = Path(os.environ["XDG_DATA_HOME"]) / os.environ["SILO_LINUX_APPLICATION_ID"] / "backup-history.json"
    wait.until(lambda _: data.is_file() and json.loads(data.read_text()).get("destination") == canonical)
    browser.save_screenshot(str(evidence / "native-folder-picker-accepted.png"))
    return data


def create_source_archive(browser, wait, evidence, name, history_path):
    # Accepting the export folder in the native chooser starts the export of
    # only the named computer, so the archive is awaited here.
    deadline = time.monotonic() + 900
    while time.monotonic() < deadline:
        destination = Path(json.loads(history_path.read_text())["destination"])
        matches = [path for path in destination.glob("*.silo-backup") if path.is_file()]
        if matches:
            archive = max(matches, key=lambda path: path.stat().st_mtime_ns)
            digest = sha256_file(archive)
            browser.save_screenshot(str(evidence / "source-only-backup-complete.png"))
            (evidence / "source-only-backup.json").write_text(json.dumps({
                "source": name, "archive": str(archive), "sha256": digest,
                "destination": str(archive.parent),
            }, indent=2))
            return archive
        time.sleep(1)
    raise TimeoutError("Production UI export did not produce an archive within 15 minutes")


def open_or_create_editor_computer(browser, wait, name):
    click(browser, wait, By.ID, "application-nav-computers")
    wait.until(lambda _: browser.find_element(By.ID, "application-panel-computers").is_displayed())
    matches = browser.find_elements(By.CSS_SELECTOR, f"[data-computer-name='{name}']")
    if matches:
        return
    click(browser, wait, By.XPATH, "//button[normalize-space()='Add']")
    click(browser, wait, By.XPATH, "//*[@role='menuitem' and normalize-space()='New computer']")
    editor = browser.find_element(By.CSS_SELECTOR, "[data-testid^='computer-editor-']")
    field = editor.find_element(By.CSS_SELECTOR, "input[aria-label='Computer name']")
    field.clear()
    field.send_keys(name)
    for label, value in (("CPU limit", "2"), ("CPU ceiling", "2"), ("Memory limit", "2"),
                         ("Memory ceiling", "2"), ("Workspace storage", "20"), ("Runtime storage", "40")):
        Select(editor.find_element(By.CSS_SELECTOR, f"select[aria-label='{label}']")).select_by_value(value)
    desktop = editor.find_element(By.CSS_SELECTOR, "[role='checkbox'][aria-label='Linux desktop']")
    if desktop.get_attribute("aria-checked") != "true":
        desktop.click()
    click(browser, wait, By.XPATH, "//button[normalize-space()='Save']")
    wait.until(lambda _: browser.find_elements(By.CSS_SELECTOR, f"[data-computer-name='{name}']"))


def runtime_context(name):
    for key in ("SILO_LINUX_MSB", "SILO_LINUX_MSB_LIBRARY"):
        require_path(key)
    app_data = Path(os.environ["XDG_DATA_HOME"]) / os.environ["SILO_LINUX_APPLICATION_ID"]
    generation = app_data / "runtime-generation.json"
    directory = "runtime"
    if generation.is_file():
        record = json.loads(generation.read_text())
        directory = record["directory"]
    storage_home = app_data / directory / "microsandbox"
    home = Path(os.environ["HOME"])
    alias = home / channel_for_identifier(os.environ["SILO_LINUX_APPLICATION_ID"])["stateDir"] / hashlib.sha256(os.fsencode(storage_home)).hexdigest()[:12]
    return app_data, storage_home, alias


def wait_for_preserved_setup(browser, wait, evidence, name):
    app_data, _, _ = runtime_context(name)
    generation = app_data / "runtime-generation.json"
    directory = "runtime"
    if generation.is_file():
        directory = json.loads(generation.read_text())["directory"]
    runtime = app_data / directory
    operation = runtime / "configuration-operation.json"
    metadata = runtime / "computers.json"
    activity = runtime / "setup-activity.json"
    deadline = time.monotonic() + int(os.environ.get("SILO_LINUX_SETUP_WAIT_SECONDS", "1800"))
    last_record = None
    next_report = 0.0
    progress_path = evidence / "setup-recovery-progress.jsonl"
    while time.monotonic() < deadline:
        configured = False
        if metadata.is_file():
            try:
                configured = any(item.get("name") == name for item in json.loads(metadata.read_text()).get("computers", []))
            except (OSError, json.JSONDecodeError):
                pass
        if configured and not operation.exists():
            return
        now = time.monotonic()
        if now >= next_report:
            current = None
            if activity.is_file():
                try:
                    events = json.loads(activity.read_text())
                    if events:
                        event = events[-1]
                        current = {key: event.get(key) for key in ("step", "computer", "message", "timestamp", "elapsedSeconds")}
                except (OSError, json.JSONDecodeError):
                    pass
            record = {"elapsedSeconds": round(now - (deadline - int(os.environ.get("SILO_LINUX_SETUP_WAIT_SECONDS", "1800")))),
                      "operationPending": operation.exists(), "computerConfigured": configured, "activity": current}
            progress_path.open("a").write(json.dumps(record) + "\n")
            print(json.dumps({"setupRecovery": record}), flush=True)
            next_report = now + 30
        time.sleep(2)
    raise TimeoutError(f"Preserved desktop setup did not complete within the bounded wait: {progress_path}")


def guest(name, command, *, user="silo", timeout=120):
    _, _, alias = runtime_context(name)
    environment = dict(os.environ)
    environment.update({"MSB_HOME": str(alias), "MSB_PATH": os.environ["SILO_LINUX_MSB"],
                        "MSB_LIBKRUNFW_PATH": os.environ["SILO_LINUX_MSB_LIBRARY"],
                        "SILO_GITHUB": '{"version":1,"owners":[]}'})
    result = subprocess.run([
        environment["MSB_PATH"], "exec", name, "--no-start", "--no-tty", "--user", user,
        "--env", "USER=silo", "--env", "LOGNAME=silo", "--env", "DISPLAY=:1",
        "--", "/bin/sh", "-lc", command,
    ], env=environment, capture_output=True, text=True, timeout=timeout)
    if result.returncode != 0:
        raise AssertionError(f"Guest command failed for {name}: {result.stderr[-1200:]}")
    return result.stdout.strip()


def desktop_window(browser, wait, name):
    original = browser.current_window_handle
    click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='More actions for {name}']")
    click(browser, wait, By.CSS_SELECTOR, f"[role='menuitem'][aria-label='Open {name} desktop']")
    def attached(_):
        for handle in browser.window_handles:
            if handle == original:
                continue
            browser.switch_to.window(handle)
            if any(node.text.strip() == name for node in browser.find_elements(By.TAG_NAME, "h1")):
                if browser.find_elements(By.CSS_SELECTOR, "[aria-label='Linux desktop display']"):
                    return True
        return False
    wait.until(attached)
    return browser.current_window_handle


def start_computer(browser, wait, name):
    row = browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{name}']")
    if "Running" not in row.text:
        click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Start {name}']")
        wait.until(lambda _: "Running" in browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{name}']").text)


def stop_computer(browser, wait, name):
    row = browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{name}']")
    if "Running" in row.text:
        click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='Stop {name}']")
        wait.until(lambda _: "stopped" in browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{name}']").text.lower())


def stop_test_app(environment):
    """Signal only this packaged app instance, identified by its XDG roots."""
    app = Path(environment["SILO_LINUX_APPLICATION"]).resolve()
    expected_data = environment["XDG_DATA_HOME"]
    expected_config = environment["XDG_CONFIG_HOME"]
    candidates = set()
    if app.name == "AppRun":
        candidates.add((app.parent / "usr/bin/silo-ui").resolve())
    for proc in Path("/proc").iterdir():
        if not proc.name.isdecimal():
            continue
        pid = int(proc.name)
        try:
            values = dict(item.split("=", 1) for item in proc.joinpath("environ").read_bytes().decode(errors="replace").split("\0") if "=" in item)
            executable = proc.joinpath("exe").resolve(strict=True)
            start = proc.joinpath("stat").read_text().rsplit(")", 1)[1].split()[19]
        except (OSError, ValueError, IndexError):
            continue
        owned = values.get("XDG_DATA_HOME") == expected_data and values.get("XDG_CONFIG_HOME") == expected_config
        if app.suffix == ".AppImage":
            owned = owned and executable.name == "silo-ui" and Path(values.get("APPIMAGE", "")).resolve() == app
        else:
            owned = owned and executable in candidates
        if not owned:
            continue
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            continue
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            try:
                if not proc.exists() or proc.joinpath("stat").read_text().rsplit(")", 1)[1].split()[19] != start:
                    break
            except OSError:
                break
            time.sleep(.1)
        else:
            raise RuntimeError(f"Task-owned Silo process {pid} did not exit after SIGTERM")


def checkpoint_panel(browser, wait, name):
    row = browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{name}']")
    if browser.find_elements(By.CSS_SELECTOR, f"section[aria-label='Checkpoints for {name}']"):
        return browser.find_element(By.CSS_SELECTOR, f"section[aria-label='Checkpoints for {name}']")
    click(browser, wait, By.CSS_SELECTOR, f"button[aria-label='More actions for {name}']")
    click(browser, wait, By.CSS_SELECTOR, f"[role='menuitem'][aria-label='Checkpoints for {name}']")
    return wait.until(lambda _: browser.find_element(By.CSS_SELECTOR, f"section[aria-label='Checkpoints for {name}']"))


def test_editor_lineage(browser, wait, evidence, name, main_handle):
    """Verify one unsaved Mousepad draft across full checkpoint, fork, and restore."""
    baseline = f"SILO_SAVED_BASELINE_{secrets.token_hex(6)}"
    draft = f"SILO_UNSAVED_DRAFT_{secrets.token_hex(6)}"
    changed = f"SILO_CHANGED_AFTER_CHECKPOINT_{secrets.token_hex(6)}"
    path = "/home/silo/silo-checkpoint-draft.txt"
    # The packaged app opens on Overview in the current UI. Older builds expose
    # the Computers panel as a navigation route, so only navigate when the
    # observed source row is not already present.
    if not browser.find_elements(By.CSS_SELECTOR, f"[data-computer-name='{name}']"):
        click(browser, wait, By.ID, "application-nav-computers")
        wait.until(lambda _: browser.find_element(By.ID, "application-panel-computers").is_displayed())
    start_computer(browser, wait, name)
    import_archive = os.environ.get("SILO_LINUX_IMPORT_ARCHIVE")
    if import_archive:
        import importlib.util
        group_test = Path(__file__).with_name("test-linux-snapshot-groups.py")
        spec = importlib.util.spec_from_file_location("linux_snapshot_groups", group_test)
        group = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(group)
        started_state = group.read_state(browser)
        started_record = group.records_for_state(started_state).get(name)
        if not started_record or started_record.get("state") != "running":
            raise AssertionError(f"The imported source did not reach Running after explicit Start: {started_record}")
        capacity = group.verify_computer_capacity(started_record)
        (evidence / "production-import-started-capacity.json").write_text(json.dumps({
            "archive": str(Path(import_archive).resolve()),
            "archiveSha256": os.environ["SILO_LINUX_IMPORT_ARCHIVE_SHA256"],
            "source": "linux-legacy-source", "imported": name,
            "stateAfterExplicitStart": started_record.get("state"), "capacity": capacity,
        }, indent=2) + "\n")
    guest(name, f"printf '%s\\n' {shlex.quote(baseline)} > {shlex.quote(path)}")
    guest(name, "command -v mousepad", timeout=30)
    tools_ready = guest(name, "command -v xdotool >/dev/null && command -v xclip >/dev/null && echo ready || echo missing")
    if tools_ready != "ready":
        guest(name, "apt-get update -qq && apt-get install -y --no-install-recommends xdotool xclip >/tmp/silo-ui-test-tools.log 2>&1", user="root", timeout=600)
    guest(name, f"(DISPLAY=:1 mousepad {shlex.quote(path)} >/tmp/silo-mousepad.log 2>&1 &)")
    viewer = desktop_window(browser, wait, name)
    screen = wait.until(lambda _: browser.find_element(By.CSS_SELECTOR, "[aria-label='Linux desktop display']"))
    screen.screenshot(str(evidence / "mousepad-baseline.png"))
    ActionChains(browser).move_to_element(screen).click().perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("a").key_up(Keys.CONTROL).send_keys(draft).perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("a").key_up(Keys.CONTROL).perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("c").key_up(Keys.CONTROL).perform()
    time.sleep(.3)
    before = guest(name, "xclip -selection clipboard -o")
    assert draft in before and baseline not in before, f"Mousepad did not display the unsaved draft: {before!r}"
    baseline_file = guest(name, f"cat {shlex.quote(path)}")
    assert baseline_file == baseline, "The UI edit unexpectedly changed the saved baseline file"
    screen.screenshot(str(evidence / "editor-unsaved-before-checkpoint.png"))
    (evidence / "clipboard-before-checkpoint.txt").write_text(before + "\n")
    (evidence / "saved-baseline.txt").write_text(baseline_file + "\n")

    panel = checkpoint_panel(browser, wait, name)
    title = "unsaved-mousepad-draft"
    panel.find_element(By.CSS_SELECTOR, "input[aria-label='Checkpoint name']").send_keys(title)
    click(browser, wait, By.XPATH, "//button[normalize-space()='Create checkpoint']")
    wait.until(lambda _: title in panel.text)

    app_data, _, _ = runtime_context(name)
    runtime = app_data / "runtime"
    generation = app_data / "runtime-generation.json"
    if generation.is_file():
        runtime = app_data / json.loads(generation.read_text())["directory"]
    computers_path = runtime / "computers.json"
    before_computers = json.loads(computers_path.read_text())["computers"]
    source_computer = next(item for item in before_computers if item["name"] == name)
    source_record = json.loads((runtime / "checkpoints" / f"{source_computer['id']}.json").read_text())
    checkpoint = next(item for item in source_record["checkpoints"] if item["name"] == title)
    if checkpoint["scope"] != "full" or not source_record.get("snapshotGroup"):
        raise AssertionError(f"Checkpoint does not identify a full source capture group: {checkpoint!r}")

    # Deliberately change the unsaved text after the full checkpoint and before
    # either fork or source restore.
    ActionChains(browser).move_to_element(screen).click().perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("a").key_up(Keys.CONTROL).send_keys(changed).perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("a").key_up(Keys.CONTROL).perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("c").key_up(Keys.CONTROL).perform()
    changed_value = guest(name, "xclip -selection clipboard -o")
    assert changed in changed_value and draft not in changed_value, f"Could not change the unsaved source buffer: {changed_value!r}"
    assert guest(name, f"cat {shlex.quote(path)}") == baseline, "Changing the buffer changed the saved baseline file"
    screen.screenshot(str(evidence / "editor-unsaved-changed-after-checkpoint.png"))
    (evidence / "clipboard-changed-after-checkpoint.txt").write_text(changed_value + "\n")

    browser.switch_to.window(main_handle)
    panel = checkpoint_panel(browser, wait, name)
    fork_name = f"{name}-fork-{secrets.token_hex(3)}"
    if any(item["name"] == fork_name for item in before_computers):
        raise AssertionError(f"Refusing a duplicate fork name: {fork_name}")
    click(browser, wait, By.XPATH, f"//li[.//*[normalize-space()='{title}']]//button[normalize-space()='Fork']")
    browser.find_element(By.CSS_SELECTOR, "input[aria-label='Fork name']").send_keys(fork_name)
    click(browser, wait, By.XPATH, "//button[normalize-space()='Create stopped fork']")
    wait.until(lambda _: browser.find_elements(By.CSS_SELECTOR, f"[data-computer-name='{fork_name}']"))
    fork = browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{fork_name}']")
    assert "Stopped" in fork.text or "stopped" in fork.text.lower(), fork.text
    after_computers = json.loads(computers_path.read_text())["computers"]
    fork_candidates = [item for item in after_computers if item["name"] == fork_name]
    new_computer_ids = {item["id"] for item in after_computers} - {item["id"] for item in before_computers}
    if len(fork_candidates) != 1 or fork_candidates[0]["id"] not in new_computer_ids:
        raise AssertionError(f"Fork did not create one new computer identity: {fork_candidates!r}")
    fork_computer = fork_candidates[0]
    fork_record = json.loads((runtime / "checkpoints" / f"{fork_computer['id']}.json").read_text())
    pending = fork_record.get("pendingCheckpointRestore") or {}
    if (pending.get("checkpointId") != checkpoint["id"] or pending.get("state") != "full"
            or pending.get("sourceComputer") != source_record["snapshotGroup"]):
        raise AssertionError(f"Fork is not pending the selected full capture: {pending!r}")
    (evidence / "editor-fork-lineage.json").write_text(json.dumps({
        "source": name, "sourceComputerId": source_computer["id"],
        "checkpointId": checkpoint["id"], "checkpointScope": checkpoint["scope"],
        "snapshotGroup": source_record["snapshotGroup"],
        "fork": fork_name, "forkComputerId": fork_computer["id"],
        "pendingCheckpointRestore": pending,
    }, indent=2))
    start_computer(browser, wait, fork_name)
    browser.switch_to.window(main_handle)
    fork_viewer = desktop_window(browser, wait, fork_name)
    screen = wait.until(lambda _: browser.find_element(By.CSS_SELECTOR, "[aria-label='Linux desktop display']"))
    ActionChains(browser).move_to_element(screen).click().perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("a").key_up(Keys.CONTROL).perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("c").key_up(Keys.CONTROL).perform()
    after_fork = guest(fork_name, "xclip -selection clipboard -o")
    assert draft in after_fork and changed not in after_fork, f"Fork did not restore the checkpoint draft: {after_fork!r}"
    assert guest(fork_name, f"cat {shlex.quote(path)}") == baseline, "Fork changed the saved baseline file"
    screen.screenshot(str(evidence / "editor-unsaved-after-fork.png"))
    (evidence / "clipboard-after-fork.txt").write_text(after_fork + "\n")

    browser.switch_to.window(main_handle)
    panel = checkpoint_panel(browser, wait, name)
    click(browser, wait, By.XPATH, f"//li[.//*[normalize-space()='{title}']]//button[normalize-space()='Restore']")
    click(browser, wait, By.XPATH, "//button[normalize-space()='Save recovery point and restore']")
    wait.until(lambda _: "stopped" in browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{name}']").text.lower())
    start_computer(browser, wait, name)
    browser.switch_to.window(viewer)
    screen = wait.until(lambda _: browser.find_element(By.CSS_SELECTOR, "[aria-label='Linux desktop display']"))
    ActionChains(browser).move_to_element(screen).click().perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("a").key_up(Keys.CONTROL).perform()
    ActionChains(browser).key_down(Keys.CONTROL).send_keys("c").key_up(Keys.CONTROL).perform()
    after_restore = guest(name, "xclip -selection clipboard -o")
    assert draft in after_restore and changed not in after_restore, f"Source restore did not recover the checkpoint draft: {after_restore!r}"
    assert guest(name, f"cat {shlex.quote(path)}") == baseline, "Restore changed the saved baseline file"
    screen.screenshot(str(evidence / "editor-unsaved-after-restore.png"))
    (evidence / "clipboard-after-source-restore.txt").write_text(after_restore + "\n")
    browser.switch_to.window(main_handle)
    stop_computer(browser, wait, fork_name)
    stop_computer(browser, wait, name)
    browser.switch_to.window(fork_viewer)


def run():
    root, evidence, app_id = prepare_environment()
    preflight_native_chooser()
    destination_value = os.environ.get("SILO_LINUX_ARCHIVE_DESTINATION")
    if not destination_value:
        raise RuntimeError("Set SILO_LINUX_ARCHIVE_DESTINATION to the task-owned archive folder")
    destination = Path(destination_value).resolve()
    if not under(destination, root):
        raise RuntimeError("SILO_LINUX_ARCHIVE_DESTINATION must be inside the task root")
    destination.mkdir(exist_ok=True)
    environment = dict(os.environ)
    environment["SILO_LINUX_APPLICATION_ID"] = app_id
    process = log = browser = None
    passed = False
    try:
        process, log, browser = start_driver(environment, evidence)
        ui_wait_seconds = int(os.environ.get("SILO_LINUX_UI_WAIT_SECONDS", "30"))
        wait = WebDriverWait(browser, ui_wait_seconds, ignored_exceptions=(StaleElementReferenceException, ElementClickInterceptedException, ElementNotInteractableException))
        app_window(browser, wait)
        wait.until(lambda _: "Silo could not load" not in browser.find_element(By.TAG_NAME, "body").text)
        main_handle = browser.current_window_handle
        name = os.environ.get("SILO_LINUX_COMPUTER_NAME", "linux-legacy-source")
        import_archive = os.environ.get("SILO_LINUX_IMPORT_ARCHIVE")
        if os.environ.get("SILO_LINUX_REUSE_FIXTURE") == "1":
            wait_for_preserved_setup(browser, wait, evidence, name)
        editor_only = os.environ.get("SILO_LINUX_EDITOR_ONLY") == "1"
        imported_fixture = os.environ.get("SILO_LINUX_IMPORTED_FIXTURE") == "1"
        if import_archive and not editor_only:
            # Reuse the production IPC and completion checks from the live
            # snapshot-group test; this imports the retained archive into a
            # clean XDG home without another export.
            import importlib.util
            group_test = Path(__file__).with_name("test-linux-snapshot-groups.py")
            spec = importlib.util.spec_from_file_location("linux_snapshot_groups", group_test)
            group = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(group)
            archive_path = Path(import_archive).resolve(strict=True)
            inspected = group.invoke(browser, "inspect_backup_archive", {"archivePath": str(archive_path)})
            archive_metadata = inspected.get("archive", {})
            if not inspected.get("valid") or archive_metadata.get("computers") != ["linux-legacy-source"]:
                raise AssertionError(f"The retained source archive failed production inspection: {inspected}")
            previous_operation = group.invoke(browser, "read_backup_state").get("operationId")
            group.invoke(browser, "start_restore", {
                "archivePath": str(archive_path), "newName": name, "sourceName": "linux-legacy-source",
            })
            group.operation_result(browser, "restore", previous_operation)
            restored_state = group.read_state(browser)
            restored = group.records_for_state(restored_state).get(name)
            if restored is None or restored.get("state") != "stopped":
                raise AssertionError(f"Production restore did not register a stopped desktop source: {restored}")
            capacity = group.verify_computer_capacity(restored)
            (evidence / "production-import.json").write_text(json.dumps({
                "archive": str(archive_path), "source": "linux-legacy-source", "imported": name,
                "state": restored.get("state"), "capacity": capacity,
            }, indent=2) + "\n")
            browser.save_screenshot(str(evidence / "production-import-stopped-overview.png"))
        elif editor_only:
            if imported_fixture:
                import importlib.util
                group_test = Path(__file__).with_name("test-linux-snapshot-groups.py")
                spec = importlib.util.spec_from_file_location("linux_snapshot_groups", group_test)
                group = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(group)
                archive_path = Path(import_archive or "").resolve(strict=True)
                inspected = group.invoke(browser, "inspect_backup_archive", {"archivePath": str(archive_path)})
                if not inspected.get("valid") or inspected.get("archive", {}).get("computers") != ["linux-legacy-source"]:
                    raise AssertionError(f"The preserved source archive failed production inspection: {inspected}")
                restored_state = group.read_state(browser)
                restored = group.records_for_state(restored_state).get(name)
                if restored is None or restored.get("state") != "stopped":
                    raise AssertionError(f"The imported desktop source is not stopped in the fresh runtime: {restored}")
                browser.save_screenshot(str(evidence / "production-import-stopped-overview.png"))
                start_computer(browser, wait, name)
                running_state = group.read_state(browser)
                restored = group.records_for_state(running_state)[name]
                capacity = group.verify_computer_capacity(restored)
                (evidence / "production-import.json").write_text(json.dumps({
                    "archive": str(archive_path),
                    "archiveSha256": os.environ["SILO_LINUX_IMPORT_ARCHIVE_SHA256"],
                    "source": "linux-legacy-source", "imported": name,
                    "stateAfterExplicitStart": restored.get("state"), "capacity": capacity,
                }, indent=2) + "\n")
                archive = archive_path
            else:
                backup_evidence = json.loads((evidence / "source-only-backup.json").read_text())
                archive_path = Path(backup_evidence["archive"])
                if (backup_evidence.get("source") != name or not archive_path.is_file()
                        or sha256_file(archive_path) != backup_evidence.get("sha256")):
                    raise RuntimeError("The preserved source-only production archive evidence is incomplete or changed")
            # The packaged app opens directly to its Overview list. The
            # Computers control is a disclosure, not an Overview route.
            if not imported_fixture:
                source_row = wait.until(lambda _: browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{name}']"))
                wait.until(lambda _: "stopped" in source_row.text.lower())
                browser.save_screenshot(str(evidence / "source-overview-stopped.png"))
                archive = archive_path
        else:
            open_or_create_editor_computer(browser, wait, name)
            click(browser, wait, By.ID, "application-nav-computers")
            source_row = wait.until(lambda _: browser.find_element(By.CSS_SELECTOR, f"[data-computer-name='{name}']"))
            wait.until(lambda _: "stopped" in source_row.text.lower())
            browser.save_screenshot(str(evidence / "source-overview-stopped.png"))
            history = native_picker(browser, wait, evidence, destination, name)
            archive = create_source_archive(browser, wait, evidence, name, history)
        test_editor_lineage(browser, wait, evidence, name, main_handle)
        report = {"passed": True, "app": str(Path(os.environ["SILO_LINUX_APPLICATION"]).resolve()),
                  "app_sha256": os.environ["SILO_LINUX_APPLICATION_SHA256"],
                  "appimage": str(Path(os.environ["SILO_LINUX_APPIMAGE"]).resolve()),
                  "appimage_sha256": os.environ["SILO_LINUX_APPIMAGE_SHA256"],
                  "source": name, "destination": str(destination.resolve()), "archive": str(archive),
                  "archive_sha256": os.environ.get("SILO_LINUX_IMPORT_ARCHIVE_SHA256") or sha256_file(archive),
                  "editor_lineage": True}
        (evidence / "ui-qualification.json").write_text(json.dumps(report, indent=2))
        passed = True
        print(json.dumps(report, indent=2))
    except Exception:
        if browser:
            try:
                browser.save_screenshot(str(evidence / "ui-qualification-failure.png"))
                (evidence / "ui-qualification-failure.txt").write_text(browser.find_element(By.TAG_NAME, "body").text)
            except Exception:
                pass
        raise
    finally:
        if browser:
            browser.quit()
        if process:
            process.terminate()
            process.wait(timeout=10)
        stop_test_app(environment)
        if log:
            log.close()
        (evidence / "ui-qualification-status.json").write_text(json.dumps({"passed": passed}, indent=2))


if __name__ == "__main__":
    run()
