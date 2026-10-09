"""Opt-in real GNOME service checks, outside the production application."""
import json
import os
from pathlib import Path
import subprocess
import time

from selenium.webdriver.common.by import By
from channel_names import channel_for_identifier, channel_names


def verify(browser, wait, environment, evidence):
    names = channel_names()
    identifier = environment.get("SILO_LINUX_APPLICATION_ID", names["production"]["identifier"])
    product_name = channel_for_identifier(identifier)["productName"]
    import gi
    gi.require_version("Gio", "2.0")
    from gi.repository import Gio, GLib
    import pyatspi

    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    def call(destination, path, interface, method, signature=None, args=None):
        parameters = GLib.Variant(signature, args) if signature else None
        return bus.call_sync(destination, path, interface, method, parameters, None,
                             Gio.DBusCallFlags.NONE, 5000, None).unpack()
    def prop(destination, path, interface, name):
        return call(destination, path, "org.freedesktop.DBus.Properties", "Get", "(ss)", (interface, name))[0]
    def nodes(root):
        yield root
        for child in root:
            if child is not None:
                yield from nodes(child)
    def find_native(predicate, app_name=None):
        for app in pyatspi.Registry.getDesktop(0):
            if app_name is not None and app.name != app_name:
                continue
            try:
                for node in nodes(app):
                    if predicate(node):
                        return node
            except (RuntimeError, GLib.Error):
                continue
        return None
    def action(node):
        actions = node.queryAction()
        for index in range(actions.nActions):
            if actions.getName(index) in ("click", "press", "activate"):
                assert actions.doAction(index)
                return
        raise AssertionError("Native control has no activation action")

    checks = []
    result_path = evidence / "gnome-services.json"
    result_path.write_text(json.dumps({"passed": False, "checks": []}))
    (evidence / "gnome-notification.png").unlink(missing_ok=True)
    server = call("org.freedesktop.Notifications", "/org/freedesktop/Notifications",
                  "org.freedesktop.Notifications", "GetServerInformation")
    assert server[0] == "gnome-shell" and server[1] == "GNOME", server
    checks.append(f"Real {server[0]} {server[2]} notification service is present")
    watcher = "org.kde.StatusNotifierWatcher"
    item_names = prop(watcher, "/StatusNotifierWatcher", watcher, "RegisteredStatusNotifierItems")
    items = []
    for name in item_names:
        destination, slash, suffix = name.partition("/")
        path = "/" + suffix if slash else "/StatusNotifierItem"
        if prop(destination, path, "org.kde.StatusNotifierItem", "Title") == product_name:
            items.append((destination, path))
    assert len(items) == 1, items
    destination, path = items[0]
    menu = prop(destination, path, "org.kde.StatusNotifierItem", "Menu")
    layout = call(destination, menu, "com.canonical.dbusmenu", "GetLayout", "(iias)", (0, -1, []))[1]
    def menu_nodes(node):
        yield node
        for child in node[2]:
            yield from menu_nodes(child)
    open_item = next(node for node in menu_nodes(layout) if node[1].get("label") == f"Open {product_name}")
    assert any(node[1].get("label") == f"Quit {product_name}" for node in menu_nodes(layout))
    def main_visible():
        return find_native(lambda node: node.getRoleName() == "frame" and node.name == product_name and node.getState().contains(pyatspi.STATE_SHOWING))
    wait.until(lambda _: main_visible())
    browser.find_element(By.CSS_SELECTOR, "button[aria-label='Close window']").click()
    wait.until(lambda _: not main_visible())
    call(destination, menu, "com.canonical.dbusmenu", "Event", "(isvu)",
         (open_item[0], "clicked", GLib.Variant("i", 0), 0))
    wait.until(lambda _: main_visible())
    wait.until(lambda _: browser.find_element(By.ID, "application-nav-computers").is_displayed())
    checks.append("Ubuntu AppIndicator registers Silo and its real Open Silo menu action reopens the closed window")

    browser.find_element(By.ID, "application-nav-computers").click()
    wait.until(lambda _: browser.find_element(By.ID, "application-panel-computers").is_displayed())
    browser.find_element(By.XPATH, "//section[@id='application-panel-computers']//button[normalize-space()='Add']").click()
    browser.find_element(By.XPATH, "//*[@role='menuitem'][normalize-space()='Import computer…']").click()
    picker = "Choose a Silo export to import"
    dialog = wait.until(lambda _: find_native(lambda node: node.getRoleName() in ("dialog", "file chooser") and picker in node.name))
    cancel = next(node for node in nodes(dialog) if node.getRoleName() == "push button" and node.name in ("Cancel", "_Cancel"))
    action(cancel)
    wait.until(lambda _: not find_native(lambda node: node.getRoleName() in ("dialog", "file chooser") and picker in node.name))
    wait.until(lambda _: browser.find_element(By.XPATH, "//section[@id='application-panel-computers']//button[normalize-space()='Add']").is_enabled())
    assert "Import failed" not in browser.find_element(By.TAG_NAME, "body").text
    checks.append("Production import picker opens a real native dialog and Cancel returns without starting an import")

    # A corrupt file only in the harness-owned XDG directory exercises the actual
    # background health transition and native notification adapter. No fake bus,
    # notification API, production hook, credentials or existing computer is used.
    metadata = Path(environment["XDG_DATA_HOME"]) / identifier / "runtime/computers.json"
    original = metadata.read_bytes() if metadata.exists() else None
    output_path = evidence / "gnome-notification-bus.log"
    with output_path.open("w") as output:
        monitor = subprocess.Popen(["dbus-monitor", "--session", "type='method_call',interface='org.freedesktop.Notifications',member='Notify'"],
                                   stdout=output, stderr=subprocess.STDOUT)
        try:
            time.sleep(32)  # Establish one normal production 30-second health observation.
            metadata.parent.mkdir(parents=True, exist_ok=True)
            metadata.write_text("{invalid isolated test metadata")
            deadline = time.monotonic() + 40
            while time.monotonic() < deadline:
                if "Health checks unavailable" in output_path.read_text():
                    break
                time.sleep(.25)
            else:
                raise AssertionError("Production health error did not reach GNOME Notifications")
            banner = wait.until(lambda _: find_native(
                lambda node: "Health checks unavailable" in node.name
                and node.getState().contains(pyatspi.STATE_SHOWING)
                and node.getState().contains(pyatspi.STATE_VISIBLE), "gnome-shell"))
            assert banner is not None
            capture = "import gi; gi.require_version('Gdk','3.0'); from gi.repository import Gdk; w=Gdk.get_default_root_window(); Gdk.pixbuf_get_from_window(w,0,0,w.get_width(),w.get_height()).savev(__import__('sys').argv[1],'png',[],[])"
            subprocess.run(["python3", "-c", capture, str(evidence / "gnome-notification.png")],
                           env={**os.environ, "GDK_BACKEND": "x11"}, check=True, timeout=10)
            checks.append("A real Silo health failure is delivered and displayed by GNOME Notifications")
        finally:
            if original is None:
                metadata.unlink(missing_ok=True)
            else:
                metadata.write_bytes(original)
            monitor.terminate()
            monitor.wait(timeout=5)
    quit_item = next(node for node in menu_nodes(layout) if node[1].get("label") == f"Quit {product_name}")
    call(destination, menu, "com.canonical.dbusmenu", "Event", "(isvu)",
         (quit_item[0], "clicked", GLib.Variant("i", 0), 0))
    wait.until(lambda _: not main_visible())
    wait.until(lambda _: not any(item.startswith(destination) for item in prop(watcher, "/StatusNotifierWatcher", watcher, "RegisteredStatusNotifierItems")))
    checks.append("The real Quit Silo tray action removes the native window and tray item")
    result_path.write_text(json.dumps({"passed": True, "checks": checks}, indent=2))
    return checks
