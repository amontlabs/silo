#!/usr/bin/env python3
"""Apply Silo's source-hash-guarded Selkies 2.0.0 display scaling and frame size fixes."""
import hashlib
import os
from pathlib import Path
import stat
import sys
import tempfile


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
SCALING_REPLACEMENTS = (
    (DOCSTRING_OLD, DOCSTRING_NEW), (SETTINGS_OLD, SETTINGS_NEW),
    (REGEX_OLD, REGEX_NEW), (DOC_OLD, DOC_NEW), (READ_OLD, READ_NEW),
)

# A screen above H.264 level 5.2's maximum frame size makes the browsers'
# decoders refuse the stream, so every size a client can request is fitted.
FIT_HELPER_OLD = b'''def parse_resize_dims(res_str: str) -> Optional[Tuple[int, int]]:
'''
FIT_HELPER_NEW = b'''# H.264 level 5.2's maximum frame size in 16x16 macroblocks, the largest the browsers' H.264 decoders accept.
MAX_FRAME_MACROBLOCKS = 36864


def fit_frame_size(w: int, h: int) -> Tuple[int, int]:
    """Scale ``w`` x ``h`` down, keeping the aspect ratio, until it fits the
    maximum H.264 frame size; each side is rounded down to even.

    Returns:
        ``(w, h)`` unchanged when it already fits.
    """
    def macroblocks(width: int, height: int) -> int:
        return -(-width // 16) * -(-height // 16)

    if macroblocks(w, h) <= MAX_FRAME_MACROBLOCKS:
        return w, h
    scale = (MAX_FRAME_MACROBLOCKS * 256 / (w * h)) ** 0.5
    while True:
        fitted_w, fitted_h = max(2, int(w * scale)) & ~1, max(2, int(h * scale)) & ~1
        if macroblocks(fitted_w, fitted_h) <= MAX_FRAME_MACROBLOCKS:
            return fitted_w, fitted_h
        scale *= 0.99


def parse_resize_dims(res_str: str) -> Optional[Tuple[int, int]]:
'''
FIT_PARSE_OLD = b'''    w, h = min(w, 7680) & ~1, min(h, 4320) & ~1
    if w <= 0 or h <= 0:
        return None
    return w, h
'''
FIT_PARSE_NEW = b'''    w, h = min(w, 7680) & ~1, min(h, 4320) & ~1
    if w <= 0 or h <= 0:
        return None
    return fit_frame_size(w, h)
'''
FIT_REPLACEMENTS = (
    (FIT_HELPER_OLD, FIT_HELPER_NEW), (FIT_PARSE_OLD, FIT_PARSE_NEW),
)
# Kept for tests that exercise the whole display_utils rewrite.
REPLACEMENTS = SCALING_REPLACEMENTS + FIT_REPLACEMENTS

IMPORT_OLD = b'''    parse_resize_dims,
    cursor_size_for_dpi,
'''
IMPORT_NEW = b'''    parse_resize_dims,
    fit_frame_size,
    cursor_size_for_dpi,
'''
INITIAL_OLD = b'''                    target_h = old_display_height if old_display_height > 0 else 768
                if target_w % 2 != 0: target_w -= 1
'''
INITIAL_NEW = b'''                    target_h = old_display_height if old_display_height > 0 else 768
                target_w, target_h = fit_frame_size(target_w, target_h)
                if target_w % 2 != 0: target_w -= 1
'''
WEBSOCKETS_REPLACEMENTS = ((IMPORT_OLD, IMPORT_NEW), (INITIAL_OLD, INITIAL_NEW))

# Each patched module with its pinned 2.0.0 source hash, the hash after the
# patch, and the fragments rewritten. `earlier` maps the result of a previous
# version of this patch to the fragments still to apply on top of it.
MODULES = {
    'display_utils.py': {
        'source': 'b00e3f43be55ece8ad5ad0a589eb236e8d1f303db96297fe33ac925ef388ab84',
        'patched': '5e31a80db3511e2a16830c8dcd5bbe3ccc5003fec227e1bbb64716f3c9d1ce84',
        'replacements': REPLACEMENTS,
        'earlier': {
            '3a53dbbfa9609f90924e665d05ea6315ce2e0fd0df651cfe986fb90ec4dd34d9': FIT_REPLACEMENTS,
        },
    },
    'websockets_mode.py': {
        'source': '1d86b0092fc9f24ccec1e862bf94309021b9136344916ac7836565e1a4c4cebc',
        'patched': '5de8017e68901b42090b1e5252d7774f5164bc7cf1c60139583775af7cef4319',
        'replacements': WEBSOCKETS_REPLACEMENTS,
        'earlier': {},
    },
}
SUPPORTED_ARCHITECTURES = ('amd64', 'arm64')


def transform_source(source, replacements=REPLACEMENTS):
    if any(source.count(old) != 1 for old, _ in replacements):
        raise ValueError('Pinned Selkies module does not match the expected source fragments')
    updated = source
    for old, new in replacements:
        updated = updated.replace(old, new, 1)
    if any(updated.count(new) != 1 for _, new in replacements):
        raise ValueError('Selkies patch did not produce the expected result')
    return updated


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read_module(path, name):
    path = Path(path)
    if not path.is_file() or path.is_symlink():
        raise ValueError(f'Selkies module {name} is missing or not a regular file')
    return path.read_bytes()


def plan_file(path, name, spec):
    """The patched bytes for one module, or None when it is already patched."""
    source = read_module(path, name)
    digest = sha256(source)
    if digest == spec['patched']:
        return None
    if digest == spec['source']:
        replacements = spec['replacements']
    elif digest in spec['earlier']:
        replacements = spec['earlier'][digest]
    else:
        raise ValueError(f'Selkies module {name} source hash is not the pinned 2.0.0 module')
    updated = transform_source(source, replacements)
    if sha256(updated) != spec['patched']:
        raise ValueError(f'Selkies patch result hash mismatch for {name}')
    return updated


def write_atomically(path, updated):
    path = Path(path)
    original = path.stat()
    fd, temporary = tempfile.mkstemp(prefix='.selkies-patch-', dir=path.parent)
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


def patch_package(directory, architecture, modules=None):
    """Patch every module in ``directory``; nothing is written unless all match."""
    modules = MODULES if modules is None else modules
    if architecture not in SUPPORTED_ARCHITECTURES:
        raise ValueError('Unsupported Selkies package architecture')
    directory = Path(directory)
    plans = {name: plan_file(directory / name, name, spec) for name, spec in modules.items()}
    for name, updated in plans.items():
        if updated is not None:
            write_atomically(directory / name, updated)
    return 'already patched' if all(u is None for u in plans.values()) else 'patched'


def find_package():
    modules = list(Path('/opt/selkies/lib').glob('python*/site-packages/selkies/display_utils.py'))
    if len(modules) != 1:
        raise ValueError('Expected exactly one installed Selkies 2.0.0 display module')
    return modules[0].parent


def main(argv):
    if len(argv) != 1:
        raise ValueError('Usage: patch-selkies-display-scaling.py amd64|arm64')
    print(f"Selkies 2.0.0 {argv[0]} display scaling {patch_package(find_package(), argv[0])}")


if __name__ == '__main__':
    try:
        main(sys.argv[1:])
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
