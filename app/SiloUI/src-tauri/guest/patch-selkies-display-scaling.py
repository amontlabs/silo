#!/usr/bin/env python3
"""Apply Silo's source-hash-guarded Selkies 2.0.0 XFCE display scaling fix."""
import hashlib
import os
from pathlib import Path
import stat
import sys
import tempfile


SOURCE_SHA256 = {
    'amd64': 'b00e3f43be55ece8ad5ad0a589eb236e8d1f303db96297fe33ac925ef388ab84',
    'arm64': 'b00e3f43be55ece8ad5ad0a589eb236e8d1f303db96297fe33ac925ef388ab84',
}
PATCHED_SHA256 = {
    'amd64': '3a53dbbfa9609f90924e665d05ea6315ce2e0fd0df651cfe986fb90ec4dd34d9',
    'arm64': '3a53dbbfa9609f90924e665d05ea6315ce2e0fd0df651cfe986fb90ec4dd34d9',
}

# XFCE has no fractional scaling, so a client density that is a whole multiple
# of 96 (at least 2x) is applied as a window scaling factor with the font DPI
# at 96, as the MATE path does, instead of only raising the font DPI.
DOCSTRING_OLD = b'''    """Apply DPI and a DPI-scaled cursor size via xfconf-query for XFCE.

    Commands run inside the live XFCE session environment when it can be
    found, so they reach the session's own D-Bus bus.

    Returns:
        True when both settings were applied.
    """
'''
DOCSTRING_NEW = b'''    """Apply DPI, window scaling and a scaled cursor size via xfconf-query for XFCE.

    A density that is a whole multiple of 96 from 2x up becomes an integer
    ``WindowScalingFactor`` with the font DPI left at 96; any other density
    rides on the font DPI alone. The window manager theme follows the scale
    when it is one of the stock Default themes.

    Commands run inside the live XFCE session environment when it can be
    found, so they reach the session's own D-Bus bus.

    Returns:
        True when the DPI, window scale and cursor size were applied.
    """
'''
SETTINGS_OLD = b'''    cmd_dpi = [
        "xfconf-query", "-c", "xsettings", "-p", "/Xft/DPI",
        "-s", str(dpi_value), "--create", "-t", "int"
    ]
    if not await run_command(
        cmd_dpi,
        f"Successfully set XFCE DPI to {dpi_value} using xfconf-query.",
        "Failed to set XFCE DPI using xfconf-query"
    ):
        return False

    cursor_size = int(round(dpi_value / 96 * 32))
    logger.debug(f"Attempting to set cursor size to: {cursor_size} (based on DPI {dpi_value})")
    cmd_cursor = [
        "xfconf-query", "-c", "xsettings", "-p", "/Gtk/CursorThemeSize",
        "-s", str(cursor_size), "--create", "-t", "int"
    ]
    if not await run_command(
        cmd_cursor,
        f"Successfully set cursor size to {cursor_size}",
        "Failed to set cursor size using xfconf-query"
    ):
        return False

    return True
'''
SETTINGS_NEW = b'''    scale = dpi_value // 96 if dpi_value >= 192 and dpi_value % 96 == 0 else 1
    font_dpi = dpi_value // scale
    # GTK hands the cursor size to Xcursor without the window scale, and the
    # captured cursor bitmap is divided by the viewer density, so size it from
    # the full density.
    cursor_size = int(round(dpi_value / 96 * 32))

    async def read_setting(channel: str, prop: str) -> Optional[str]:
        try:
            process = await subprocess.create_subprocess_exec(
                "xfconf-query", "-c", channel, "-p", prop,
                env=session_env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE
            )
            stdout, _stderr = await _communicate_or_kill(process)
            return stdout.decode().strip() if process.returncode == 0 else None
        except Exception as e:
            logger.error(f"Error reading xfconf property {channel}{prop}: {e}")
            return None

    cmd_scale = [
        "xfconf-query", "-c", "xsettings", "-p", "/Gdk/WindowScalingFactor",
        "-s", str(scale), "--create", "-t", "int"
    ]
    cmd_dpi = [
        "xfconf-query", "-c", "xsettings", "-p", "/Xft/DPI",
        "-s", str(font_dpi), "--create", "-t", "int"
    ]
    # Lower the scale before the DPI and raise it after, so the session never
    # renders at the product of the old and the new factor.
    previous_scale = await read_setting("xsettings", "/Gdk/WindowScalingFactor")
    scale_first = previous_scale is not None and previous_scale.isdigit() and int(previous_scale) > scale
    steps = [
        (cmd_scale,
         f"Successfully set XFCE window scaling factor to {scale} using xfconf-query.",
         "Failed to set XFCE window scaling factor using xfconf-query"),
        (cmd_dpi,
         f"Successfully set XFCE DPI to {font_dpi} using xfconf-query.",
         "Failed to set XFCE DPI using xfconf-query"),
    ]
    if not scale_first:
        steps.reverse()
    for cmd, success_msg, failure_msg in steps:
        if not await run_command(cmd, success_msg, failure_msg):
            return False

    logger.debug(f"Attempting to set cursor size to: {cursor_size} (based on DPI {dpi_value})")
    cmd_cursor = [
        "xfconf-query", "-c", "xsettings", "-p", "/Gtk/CursorThemeSize",
        "-s", str(cursor_size), "--create", "-t", "int"
    ]
    if not await run_command(
        cmd_cursor,
        f"Successfully set cursor size to {cursor_size}",
        "Failed to set cursor size using xfconf-query"
    ):
        return False

    # Best effort: a custom window manager theme is left alone.
    wm_theme = "Default-xhdpi" if scale >= 2 else "Default"
    current_theme = await read_setting("xfwm4", "/general/theme")
    if current_theme in ("Default", "Default-hdpi", "Default-xhdpi") and current_theme != wm_theme:
        await run_command(
            ["xfconf-query", "-c", "xfwm4", "-p", "/general/theme", "-s", wm_theme],
            f"Successfully set xfwm4 theme to {wm_theme}",
            "Failed to set xfwm4 theme using xfconf-query"
        )

    return True
'''
REGEX_OLD = b'''_XFCONF_DPI = re.compile(r'name="DPI"[^>]*value="(-?\\d+)"')
'''
REGEX_NEW = b'''_XFCONF_DPI = re.compile(r'name="DPI"[^>]*value="(-?\\d+)"')
_XFCONF_SCALE = re.compile(r'name="WindowScalingFactor"[^>]*value="(-?\\d+)"')
'''
DOC_OLD = b'''    Read from where `set_dpi` puts it: an XFCE session's xfconf channel as
    xfconfd persists it, else the server's resource database and then the'''
DOC_NEW = b'''    Read from where `set_dpi` puts it: an XFCE session's xfconf channel as
    xfconfd persists it (the font DPI times the window scaling factor), else
    the server's resource database and then the'''
READ_OLD = b'''        if found and found[-1] > 0:
            return found[-1] // (1024 if pattern is _XSETTINGS_DPI else 1)
'''
READ_NEW = b'''        if found and found[-1] > 0:
            density = found[-1] // (1024 if pattern is _XSETTINGS_DPI else 1)
            if pattern is _XFCONF_DPI:
                # The font DPI is stored apart from the window scale.
                scales = [int(v) for v in _XFCONF_SCALE.findall(text)]
                density *= scales[-1] if scales and scales[-1] > 0 else 1
            return density
'''
REPLACEMENTS = (
    (DOCSTRING_OLD, DOCSTRING_NEW), (SETTINGS_OLD, SETTINGS_NEW),
    (REGEX_OLD, REGEX_NEW), (DOC_OLD, DOC_NEW), (READ_OLD, READ_NEW),
)


def transform_source(source):
    if any(source.count(old) != 1 for old, _ in REPLACEMENTS):
        raise ValueError('Pinned Selkies display module does not match the expected source fragments')
    updated = source
    for old, new in REPLACEMENTS:
        updated = updated.replace(old, new, 1)
    if any(updated.count(new) != 1 for _, new in REPLACEMENTS):
        raise ValueError('Selkies display scaling patch did not produce the expected result')
    return updated


def patch_file(path, architecture):
    if architecture not in SOURCE_SHA256:
        raise ValueError('Unsupported Selkies package architecture')
    path = Path(path)
    if not path.is_file() or path.is_symlink():
        raise ValueError('Selkies display module is missing or not a regular file')
    source = path.read_bytes()
    digest = hashlib.sha256(source).hexdigest()
    if digest == PATCHED_SHA256[architecture]:
        return 'already patched'
    if digest != SOURCE_SHA256[architecture]:
        raise ValueError('Selkies display module source hash is not the pinned 2.0.0 module')
    updated = transform_source(source)
    if hashlib.sha256(updated).hexdigest() != PATCHED_SHA256[architecture]:
        raise ValueError('Selkies display scaling patch result hash mismatch')

    original = path.stat()
    fd, temporary = tempfile.mkstemp(prefix='.selkies-display-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as output:
            output.write(updated)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary, stat.S_IMODE(original.st_mode))
        os.chown(temporary, original.st_uid, original.st_gid)
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | getattr(os, 'O_DIRECTORY', 0))
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
    return 'patched'


def find_module():
    modules = list(Path('/opt/selkies/lib').glob('python*/site-packages/selkies/display_utils.py'))
    if len(modules) != 1:
        raise ValueError('Expected exactly one installed Selkies 2.0.0 display module')
    return modules[0]


def main(argv):
    if len(argv) != 1:
        raise ValueError('Usage: patch-selkies-display-scaling.py amd64|arm64')
    print(f"Selkies 2.0.0 {argv[0]} display scaling {patch_file(find_module(), argv[0])}")


if __name__ == '__main__':
    try:
        main(sys.argv[1:])
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
