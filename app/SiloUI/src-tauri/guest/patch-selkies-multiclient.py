#!/usr/bin/env python3
"""Apply Silo's source-hash-guarded Selkies 2.0.0 multi-client admission patch.

Selkies 2.0.0 lets one page hold the primary display: a second page that asks
for it kills the first with a "Superseded" close the client reads as final.
Upstream's alternative, the shared viewer, is read-only and needs a page built
for it. This patch keeps the page and decides on the server:

- The first page holds the display. A later page that asks for it while the
  holder is alive becomes a peer. It joins through the shared-viewer path (same
  per-client bounded fan-out, keyframe throttle and pause rules) and may also
  type, point, use the clipboard, turn audio on and off for itself and ask for a
  screen size or density (the screen is shared, so the last accepted request
  wins and every page adapts). Encoder settings stay with the holder.
- When the holder leaves, the oldest peer is handed the display at once (a page
  reload overlaps the old and new sockets the same way).
- STOP_AUDIO and START_AUDIO are per page; the capture stops with its last listener.
- All pages reach the guest from its loopback, so the per-address reconnect
  debounce, which would refuse a second page opening within 500 ms, is off.

It runs after patch-selkies-display-scaling.py and accepts that patch's result.
"""
import hashlib
import os
from pathlib import Path
import stat
import sys
import tempfile


CONSTANTS_OLD = b'''VIEWER_RESUME_MIN_INTERVAL_S = 1.0
'''
CONSTANTS_NEW = b'''VIEWER_RESUME_MIN_INTERVAL_S = 1.0
# Silo: a peer may send the upstream collaborator set (keyboard, mouse, clipboard)
# and what a shared viewer's page never sends: audio on/off, screen size, density.
SILO_PEER_EXTRA_PREFIXES = ("START_AUDIO", "STOP_AUDIO", "r,", "s,")
# Silo: pages that turned audio off for themselves.
audio_paused_clients: set = set()
'''

DEBOUNCE_OLD = b'''        self.RECONNECT_DEBOUNCE_MS = 500
'''
DEBOUNCE_NEW = b'''        # Silo: every page reaches the guest from its loopback.
        self.RECONNECT_DEBOUNCE_MS = 0
'''

FANOUT_OLD = b'''                primary_viewers = self.clients - secondary_websockets
'''
FANOUT_NEW = b'''                primary_viewers = self.clients - secondary_websockets - audio_paused_clients
'''

LISTENERS_OLD = b'''        listeners = self.clients - secondary_ws
        if exclude is not None:
            listeners.discard(exclude)
        return listeners
'''
LISTENERS_NEW = b'''        listeners = self.clients - secondary_ws - audio_paused_clients
        if exclude is not None:
            listeners.discard(exclude)
        return listeners

    def _silo_demote_to_peer(self, websocket: web.WebSocketResponse, perms: dict) -> bool:
        """Make a page asking for the primary display a peer when a live page holds it.

        The role becomes "viewer" with a peer mark, so every upstream viewer
        rule applies, and the dispatch adds the peer's extra messages.

        Returns:
            True when the page joined the holder's stream.
        """
        entry = self.display_clients.get('primary')
        holder = entry.get('ws') if entry else None
        if holder is None or holder is websocket or holder.closed or holder not in self.clients:
            return False
        perms['role'] = 'viewer'
        perms['peer'] = True
        return True

    async def _silo_adopt_primary(self, departing: web.WebSocketResponse) -> bool:
        """Hand the primary display to the oldest peer when its holder leaves.

        The peer already renders the stream, so nothing restarts: it becomes a
        controller with fresh acknowledgement state, and its handler picks up
        the display id on its next message.

        Returns:
            True when a peer took the display.
        """
        entry = self.display_clients.get('primary')
        if self.is_secure_mode or entry is None or entry.get('ws') is not departing:
            return False
        peers = [ws for ws in self.clients
                 if (client_permissions.get(ws) or {}).get('peer') and not ws.closed]
        if not peers:
            return False
        adopter = min(peers, key=lambda ws: client_permissions[ws].get('connected_at', 0.0))
        perms = client_permissions[adopter]
        perms['role'] = 'controller'
        perms.pop('peer', None)
        perms['adopt_display'] = 'primary'
        entry['ws'] = adopter
        entry['acknowledged_frame_id'] = -1
        entry['acked_sent_at'] = None
        entry['unacked_since'] = None
        entry['stall_gated_at'] = None
        entry['sent_timestamps'].clear()
        entry['rtt_samples'].clear()
        entry['smoothed_rtt'] = 0.0
        return True
'''

LOOP_OLD = b'''            async for msg in websocket:
                # autoping is off: answer PING here, feed PONG to the uplink gauge.
'''
LOOP_NEW = b'''            async for msg in websocket:
                if client_display_id is None:
                    # Silo: this page was handed the display after its holder left.
                    client_display_id = (client_permissions.get(websocket) or {}).pop('adopt_display', None)
                # autoping is off: answer PING here, feed PONG to the uplink gauge.
'''

GATE_OLD = b'''                        if not message.startswith(allowed_viewer_prefixes):
                            # A viewer's blur/visibility noise (kr would clobber the
                            # controller's held modifiers) is refused silently: a warning
                            # per blur floods the log.
                            if not message.startswith(VIEWER_SILENT_DROP_PREFIXES):
'''
GATE_NEW = b'''                        if perms.get("peer"):
                            allowed_viewer_prefixes = (
                                allowed_viewer_prefixes + VIEWER_COLLAB_EXTRA_PREFIXES
                                + SILO_PEER_EXTRA_PREFIXES)
                        if not message.startswith(allowed_viewer_prefixes):
                            # A viewer's blur/visibility noise (kr would clobber the
                            # controller's held modifiers) is refused silently: a warning
                            # per blur floods the log.
                            if perms.get("peer"):
                                # A peer's page also sends encoder and acknowledgement
                                # traffic; the holder's settings stand.
                                pass
                            elif not message.startswith(VIEWER_SILENT_DROP_PREFIXES):
'''

ADMISSION_OLD = b'''                            client_perms = client_permissions.get(websocket)
                            client_role = client_perms.get("role") if client_perms else "controller"
'''
ADMISSION_NEW = b'''                            client_perms = client_permissions.get(websocket)
                            client_role = client_perms.get("role") if client_perms else "controller"
                            if (client_role == "controller" and display_id == 'primary'
                                    and client_perms and not self.is_secure_mode
                                    and self._silo_demote_to_peer(websocket, client_perms)):
                                client_role = "viewer"
'''

START_VIDEO_OLD = b'''                            # A shared client needs a decode entry point (its own reset plus
                            # an IDR), not a pipeline rebuild, unless nothing runs.
'''
START_VIDEO_NEW = b'''                            if (client_permissions.get(websocket) or {}).get("peer"):
                                # Silo: a peer's page expects the acknowledgement a viewer's ignores.
                                try:
                                    await websocket.send_str("VIDEO_STARTED")
                                except (ConnectionResetError, OSError, RuntimeError):
                                    pass
                            # A shared client needs a decode entry point (its own reset plus
                            # an IDR), not a pipeline rebuild, unless nothing runs.
'''

START_AUDIO_OLD = b'''                    elif message == "START_AUDIO":
                        async def _handle_start_audio_request():
'''
START_AUDIO_NEW = b'''                    elif message == "START_AUDIO":
                        audio_paused_clients.discard(websocket)
                        async def _handle_start_audio_request():
'''
AUDIO_STARTED_OLD = b'''                                        await _broadcast_to_clients(self.clients, "AUDIO_STARTED", per_client_timeout=2.0)
'''
AUDIO_STARTED_NEW = b'''                                        self.clients -= await _broadcast_to_clients(
                                            self.clients - audio_paused_clients, "AUDIO_STARTED",
                                            per_client_timeout=2.0)
'''
STOP_AUDIO_OLD = b'''                            data_logger.debug("Received STOP_AUDIO")
                            if self.is_pcmflux_capturing:
                                await self._stop_pcmflux_pipeline()
                            if self.clients:
                                await _broadcast_to_clients(self.clients, "AUDIO_STOPPED", per_client_timeout=2.0)
'''
STOP_AUDIO_NEW = b'''                            data_logger.debug("Received STOP_AUDIO")
                            audio_paused_clients.add(websocket)
                            if self._audio_listeners():
                                # Silo: another page still listens.
                                await _broadcast_to_clients(
                                    self.clients, "AUDIO_STOPPED", per_client_timeout=2.0,
                                    only=id(websocket))
                            else:
                                audio_paused_clients.update(self.clients)
                                if self.is_pcmflux_capturing:
                                    await self._stop_pcmflux_pipeline()
                                if self.clients:
                                    await _broadcast_to_clients(self.clients, "AUDIO_STOPPED", per_client_timeout=2.0)
'''

LEAVE_OLD = b'''            self.last_viewer_keyframe_request_times.pop(websocket, None)
            self.video_paused_clients.discard(websocket)
'''
LEAVE_NEW = b'''            self.last_viewer_keyframe_request_times.pop(websocket, None)
            self.video_paused_clients.discard(websocket)
            audio_paused_clients.discard(websocket)
'''
HANDOVER_OLD = b'''            if disconnected_display_id:
                # Deferred: a reloading page takes the entry over with its capture warm;
'''
HANDOVER_NEW = b'''            if disconnected_display_id == 'primary' and await self._silo_adopt_primary(websocket):
                data_logger.info("The primary display passed to the oldest peer page.")
                disconnected_display_id = None
            if disconnected_display_id:
                # Deferred: a reloading page takes the entry over with its capture warm;
'''

# A peer's size and density messages address the one primary display.
DISPATCH_OLD = b'''                            await self.input_handler.on_message(message, client_display_id, conn_id=id(websocket))
'''
DISPATCH_NEW = b'''                            await self.input_handler.on_message(
                                message,
                                client_display_id or (
                                    'primary' if (client_permissions.get(websocket) or {}).get("peer") else None),
                                conn_id=id(websocket))
'''

WEBSOCKETS_REPLACEMENTS = (
    (CONSTANTS_OLD, CONSTANTS_NEW), (DEBOUNCE_OLD, DEBOUNCE_NEW), (FANOUT_OLD, FANOUT_NEW),
    (LISTENERS_OLD, LISTENERS_NEW), (LOOP_OLD, LOOP_NEW), (GATE_OLD, GATE_NEW),
    (ADMISSION_OLD, ADMISSION_NEW), (START_VIDEO_OLD, START_VIDEO_NEW),
    (START_AUDIO_OLD, START_AUDIO_NEW), (AUDIO_STARTED_OLD, AUDIO_STARTED_NEW),
    (STOP_AUDIO_OLD, STOP_AUDIO_NEW), (DISPATCH_OLD, DISPATCH_NEW), (LEAVE_OLD, LEAVE_NEW),
    (HANDOVER_OLD, HANDOVER_NEW),
)

# The pinned 2.0.0 module, the module after patch-selkies-display-scaling.py (the one
# this patch accepts) and the module after this patch.
PRISTINE_SHA256 = '1d86b0092fc9f24ccec1e862bf94309021b9136344916ac7836565e1a4c4cebc'
MODULES = {
    'websockets_mode.py': {
        'source': '5de8017e68901b42090b1e5252d7774f5164bc7cf1c60139583775af7cef4319',
        'patched': '8e9540f558208c4d77975da290b23cc82c99b519c407293cd9a57343e3362a1d',
        'replacements': WEBSOCKETS_REPLACEMENTS,
    },
}
SUPPORTED_ARCHITECTURES = ('amd64', 'arm64')


def transform_source(source, replacements=WEBSOCKETS_REPLACEMENTS):
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
    if digest == PRISTINE_SHA256:
        raise ValueError(f'Selkies module {name} has not had the display scaling patch applied')
    if digest != spec['source']:
        raise ValueError(f'Selkies module {name} source hash is not the display-patched 2.0.0 module')
    updated = transform_source(source, spec['replacements'])
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
    modules = list(Path('/opt/selkies/lib').glob('python*/site-packages/selkies/websockets_mode.py'))
    if len(modules) != 1:
        raise ValueError('Expected exactly one installed Selkies 2.0.0 websockets module')
    return modules[0].parent


def main(argv):
    if len(argv) != 1:
        raise ValueError('Usage: patch-selkies-multiclient.py amd64|arm64')
    print(f"Selkies 2.0.0 {argv[0]} multi-client admission {patch_package(find_package(), argv[0])}")


if __name__ == '__main__':
    try:
        main(sys.argv[1:])
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
