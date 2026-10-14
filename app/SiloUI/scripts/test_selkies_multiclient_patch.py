#!/usr/bin/env python3
"""Tests for Silo's Selkies 2.0.0 multi-client admission patch.

Three layers. The synthetic tests exercise the patcher's guards on small
fixtures. The pinned tests apply the patch to the real 2.0.0 sources (the
checkout the sibling patch tests use) and check hashes, compilation and the
hash chain with the display scaling patch. The behavior tests import the
patched ``websockets_mode.py`` with the capture stack stubbed and drive its
real ``ws_handler`` with fake sockets: no browser, display, VM or network.
"""
import asyncio
import hashlib
import importlib.util
import json
import logging
import os
from pathlib import Path
import shutil
import sys
import tempfile
import types
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
GUEST = ROOT / 'src-tauri/guest'
UPSTREAM_DIR = Path('/Users/polarzero/code/projects/selkies/src/selkies')
UPSTREAM_WEBSOCKETS = UPSTREAM_DIR / 'websockets_mode.py'
UPSTREAM_DISPLAY = UPSTREAM_DIR / 'display_utils.py'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


PATCH = load('selkies_multiclient_patch', GUEST / 'patch-selkies-multiclient.py')
DISPLAY = load('selkies_display_patch_for_chain', GUEST / 'patch-selkies-display-scaling.py')

SYNTHETIC = b'\n'.join(old for old, _ in PATCH.WEBSOCKETS_REPLACEMENTS)


def digest(value):
    return hashlib.sha256(value).hexdigest()


def synthetic_modules():
    return {'websockets_mode.py': {
        'source': digest(SYNTHETIC),
        'patched': digest(PATCH.transform_source(SYNTHETIC)),
        'replacements': PATCH.WEBSOCKETS_REPLACEMENTS,
    }}


class PatcherGuardTests(unittest.TestCase):
    def test_patch_package_is_idempotent_and_keeps_the_mode(self):
        modules = synthetic_modules()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'websockets_mode.py').write_bytes(SYNTHETIC)
            (root / 'websockets_mode.py').chmod(0o640)
            self.assertEqual(PATCH.patch_package(root, 'arm64', modules), 'patched')
            self.assertEqual(digest((root / 'websockets_mode.py').read_bytes()),
                             modules['websockets_mode.py']['patched'])
            self.assertEqual((root / 'websockets_mode.py').stat().st_mode & 0o777, 0o640)
            self.assertEqual(PATCH.patch_package(root, 'arm64', modules), 'already patched')
            self.assertEqual(os.listdir(root), ['websockets_mode.py'])

    def test_a_changed_module_is_refused_and_nothing_is_written(self):
        modules = synthetic_modules()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'websockets_mode.py').write_bytes(SYNTHETIC + b'# changed\n')
            with self.assertRaisesRegex(ValueError, 'source hash'):
                PATCH.patch_package(root, 'amd64', modules)
            self.assertEqual((root / 'websockets_mode.py').read_bytes(), SYNTHETIC + b'# changed\n')
            self.assertEqual(os.listdir(root), ['websockets_mode.py'])

    def test_a_symlink_or_missing_module_is_refused(self):
        modules = synthetic_modules()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, 'missing or not a regular file'):
                PATCH.patch_package(root, 'amd64', modules)
            (root / 'real.py').write_bytes(SYNTHETIC)
            (root / 'websockets_mode.py').symlink_to(root / 'real.py')
            with self.assertRaisesRegex(ValueError, 'missing or not a regular file'):
                PATCH.patch_package(root, 'amd64', modules)

    def test_the_pristine_module_asks_for_the_display_patch_first(self):
        modules = synthetic_modules()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'websockets_mode.py').write_bytes(b'pristine')
            with mock.patch.object(PATCH, 'PRISTINE_SHA256', digest(b'pristine')):
                with self.assertRaisesRegex(ValueError, 'display scaling patch'):
                    PATCH.patch_package(root, 'amd64', modules)

    def test_every_fragment_is_required_exactly_once(self):
        with self.assertRaisesRegex(ValueError, 'expected source fragments'):
            PATCH.transform_source(b'changed source')
        old = PATCH.WEBSOCKETS_REPLACEMENTS[0][0]
        with self.assertRaisesRegex(ValueError, 'expected source fragments'):
            PATCH.transform_source(SYNTHETIC.replace(old, b''))
        with self.assertRaisesRegex(ValueError, 'expected source fragments'):
            PATCH.transform_source(SYNTHETIC + b'\n' + old)

    def test_unsupported_architecture_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'architecture'):
            PATCH.patch_package(GUEST, 'riscv')

    def test_the_patched_text_holds_each_replacement_once(self):
        patched = PATCH.transform_source(SYNTHETIC)
        for _, new in PATCH.WEBSOCKETS_REPLACEMENTS:
            self.assertEqual(patched.count(new), 1)


def display_patched(path=UPSTREAM_WEBSOCKETS):
    spec = DISPLAY.MODULES['websockets_mode.py']
    return DISPLAY.transform_source(path.read_bytes(), spec['replacements'])


@unittest.skipUnless(UPSTREAM_WEBSOCKETS.is_file(), 'pinned Selkies source checkout is not available')
class PinnedUpstreamTests(unittest.TestCase):
    def test_the_hash_chain_runs_pristine_display_multiclient(self):
        spec = PATCH.MODULES['websockets_mode.py']
        pristine = UPSTREAM_WEBSOCKETS.read_bytes()
        self.assertEqual(digest(pristine), PATCH.PRISTINE_SHA256)
        self.assertEqual(digest(pristine), DISPLAY.MODULES['websockets_mode.py']['source'])
        displayed = display_patched()
        self.assertEqual(digest(displayed), DISPLAY.MODULES['websockets_mode.py']['patched'])
        self.assertEqual(digest(displayed), spec['source'])
        patched = PATCH.transform_source(displayed)
        self.assertEqual(digest(patched), spec['patched'])
        compile(patched, 'websockets_mode.py', 'exec')

    def test_a_package_directory_takes_both_patches_in_order_and_again(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copy(UPSTREAM_DISPLAY, root / 'display_utils.py')
            shutil.copy(UPSTREAM_WEBSOCKETS, root / 'websockets_mode.py')
            with self.assertRaisesRegex(ValueError, 'display scaling patch'):
                PATCH.patch_package(root, 'arm64')
            self.assertEqual(DISPLAY.patch_package(root, 'arm64'), 'patched')
            self.assertEqual(PATCH.patch_package(root, 'arm64'), 'patched')
            self.assertEqual(PATCH.patch_package(root, 'arm64'), 'already patched')
            # The display patch runs again at every repair and image check and must
            # accept the module this patch changed.
            self.assertEqual(DISPLAY.patch_package(root, 'arm64'), 'already patched')

    def test_every_patched_piece_is_present(self):
        text = PATCH.transform_source(display_patched()).decode()
        for needle in ('def _silo_demote_to_peer', 'async def _silo_adopt_primary',
                       'audio_paused_clients: set = set()', 'self.RECONNECT_DEBOUNCE_MS = 0',
                       'SILO_PEER_EXTRA_PREFIXES = ("START_AUDIO", "STOP_AUDIO", "r,", "s,")'):
            self.assertEqual(text.count(needle), 1, needle)
        # The eviction is still there for every other display, and unreachable for the
        # primary while a live page holds it.
        self.assertIn('a new {display_id} client connected connection killed', text)


# ---------------------------------------------------------------- behavior

class FakeSocket:
    """The slice of aiohttp's WebSocketResponse that ``ws_handler`` uses."""

    def __init__(self, name):
        self.name = name
        self.queue = asyncio.Queue()
        self.sent = []
        self.closed = False
        self.close_reason = None

    def __aiter__(self):
        return self

    async def __anext__(self):
        item = await self.queue.get()
        if item is None:
            raise StopAsyncIteration
        return item

    async def send_str(self, text, *args, **kwargs):
        if self.closed:
            raise ConnectionResetError('closed')
        self.sent.append(text)

    async def send_bytes(self, data, *args, **kwargs):
        if self.closed:
            raise ConnectionResetError('closed')
        self.sent.append(bytes(data))

    async def close(self, code=1000, message=b''):
        self.closed = True
        self.close_reason = message
        self.queue.put_nowait(None)

    async def pong(self, data):
        pass

    def say(self, text):
        from aiohttp import WSMsgType
        self.queue.put_nowait(types.SimpleNamespace(type=WSMsgType.TEXT, data=text))

    def leave(self):
        """The peer goes away: the socket closes and its iteration ends."""
        self.closed = True
        self.queue.put_nowait(None)

    def texts(self):
        return [item for item in self.sent if isinstance(item, str)]

    def clear(self):
        self.sent.clear()


def settings_text(width=1000, height=700, **extra):
    payload = {'displayId': 'primary', 'initialClientWidth': width, 'initialClientHeight': height}
    payload.update(extra)
    return 'SETTINGS,' + json.dumps(payload)


async def settle(delay=0.05):
    await asyncio.sleep(delay)


@unittest.skipUnless(UPSTREAM_WEBSOCKETS.is_file(), 'pinned Selkies source checkout is not available')
class PatchedHandlerTests(unittest.IsolatedAsyncioTestCase):
    """The real patched ``ws_handler`` against fake sockets."""

    @classmethod
    def setUpClass(cls):
        # Selkies' own runtime dependencies, minus the capture stack the tests stub.
        for dependency in ('aiohttp', 'aiofiles', 'msgpack', 'PIL', 'psutil'):
            if importlib.util.find_spec(dependency) is None:
                raise unittest.SkipTest(f'{dependency} is not installed')
        cls.directory = tempfile.TemporaryDirectory()
        package = Path(cls.directory.name) / 'selkies'
        shutil.copytree(UPSTREAM_DIR, package)
        displayed = display_patched()
        spec = DISPLAY.MODULES['display_utils.py']
        (package / 'display_utils.py').write_bytes(
            DISPLAY.transform_source(UPSTREAM_DISPLAY.read_bytes(), spec['replacements']))
        (package / 'websockets_mode.py').write_bytes(PATCH.transform_source(displayed))
        cls.saved_path = list(sys.path)
        cls.saved_modules = {name: sys.modules[name] for name in list(sys.modules)
                             if name == 'selkies' or name.startswith('selkies.')}
        for name in cls.saved_modules:
            del sys.modules[name]
        stubs = {}
        for name in ('prometheus_client', 'watchdog', 'watchdog.events', 'watchdog.observers'):
            if name not in sys.modules:
                stubs[name] = sys.modules[name] = mock.MagicMock()
        cls.stubs = stubs
        sys.path.insert(0, cls.directory.name)
        # Selkies parses the command line when its settings module is imported.
        with mock.patch.object(sys, 'argv', ['selkies']), \
                mock.patch.dict(os.environ, {}, clear=False):
            import selkies.websockets_mode as module
        cls.module = module
        logging.disable(logging.CRITICAL)

    @classmethod
    def tearDownClass(cls):
        logging.disable(logging.NOTSET)
        for name in [n for n in sys.modules if n == 'selkies' or n.startswith('selkies.')]:
            del sys.modules[name]
        sys.modules.update(cls.saved_modules)
        for name in cls.stubs:
            sys.modules.pop(name, None)
        sys.path[:] = cls.saved_path
        cls.directory.cleanup()

    async def asyncSetUp(self):
        module = self.module
        module.client_permissions.clear()
        module.audio_paused_clients.clear()
        self.events = []
        self.fed = []
        server = module.DataStreamingServer()
        self.server = server

        async def nothing(*args, **kwargs):
            return None

        async def apply_settings(websocket, parsed, initial, role='controller'):
            self.events.append(('apply', websocket.name, role, initial))
            server.client_settings_received.set()

        async def input_message(message, display_id=None, conn_id=None):
            self.fed.append((message, display_id))
            if message.startswith('r,'):
                # The input handler hands a resize to the server's own handler.
                await server._handle_resize(message.split(',')[1], display_id)

        async def reconfigure():
            self.events.append(('reconfigure',))

        server._refresh_second_screen_capacity = nothing
        server.send_current_cursor = nothing
        server.broadcast_stream_resolution = nothing
        server._apply_client_settings = apply_settings
        server._apply_initial_audio_policy = nothing
        server._ensure_viewer_capture = nothing
        server._schedule_idr_for_display = lambda display: self.events.append(('idr', display))
        server.reconfigure_displays = reconfigure

        class InputStub:
            """The input handler's calls ``ws_handler`` makes; everything else is a no-op."""
            on_message = staticmethod(input_message)
            apps_available = staticmethod(lambda: True)
            installed_apps = staticmethod(lambda: [])

            def __getattr__(self, name):
                return nothing

        server.input_handler = InputStub()
        server._resource_monitor = types.SimpleNamespace(stop=nothing)
        server.RECONNECT_GRACE_S = 0.05
        server.app = types.SimpleNamespace(
            encoder='h264enc', framerate=60, display_width=1440, display_height=900)

        class NoSoundServer:
            def __init__(self, *args):
                pass

            async def open(self):
                return False

            async def aclose(self):
                pass

        patches = [
            mock.patch.object(module, 'AudioControl', NoSoundServer),
            mock.patch.object(module.capture_demand, 'sync', nothing),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        self.tasks = []
        self.addAsyncCleanup(self.drain)
        self.addresses = 0

    async def drain(self):
        crashed = []
        for task in self.tasks:
            if not task.done():
                task.cancel()
        for task in self.tasks:
            try:
                await task
            except asyncio.CancelledError:
                pass
            except BaseException as error:
                crashed.append(repr(error))
        # A handler that died on its own is a failure of the patched code, not of the test.
        self.assertEqual(crashed, [])

    async def connect(self, name):
        socket = FakeSocket(name)
        self.addresses += 1
        task = asyncio.create_task(self.server.ws_handler(socket, ('127.0.0.1', self.addresses)))
        self.tasks.append(task)
        await settle()
        return socket

    async def open_page(self, name, **size):
        socket = await self.connect(name)
        socket.say(settings_text(**size))
        await settle()
        return socket

    def holder(self):
        entry = self.server.display_clients.get('primary')
        return entry['ws'].name if entry else None

    def perms(self, socket):
        return self.module.client_permissions.get(socket)

    async def test_the_first_page_holds_the_display_and_the_second_joins_it(self):
        first = await self.open_page('first')
        self.assertEqual(self.holder(), 'first')
        self.assertEqual(self.perms(first)['role'], 'controller')
        first.clear()
        second = await self.open_page('second', width=800, height=600)
        self.assertEqual(self.holder(), 'first')
        self.assertFalse(first.closed)
        self.assertFalse(any(text.startswith('KILL') for text in first.texts()), first.texts())
        self.assertEqual(self.perms(second)['role'], 'viewer')
        self.assertTrue(self.perms(second)['peer'])
        self.assertIn('PIPELINE_RESETTING primary', second.texts())
        # Only the holder's SETTINGS reached the display.
        self.assertEqual([event for event in self.events if event[0] == 'apply'],
                         [('apply', 'first', 'controller', True)])
        self.assertIn(('idr', 'primary'), self.events)

    async def test_two_pages_opening_at_once_are_both_accepted(self):
        # The guest sees every Silo viewer as its own loopback; upstream's per-address
        # debounce would have refused the second one.
        first = await self.connect('first')
        second = await self.connect('second')
        self.assertFalse(first.closed)
        self.assertFalse(second.closed)
        self.assertEqual(self.server.RECONNECT_DEBOUNCE_MS, 0)
        self.assertEqual(len(self.server.clients), 2)

    async def test_three_pages_all_stay_connected(self):
        pages = [await self.open_page(name) for name in ('a', 'b', 'c')]
        self.assertEqual(self.holder(), 'a')
        self.assertFalse(any(page.closed for page in pages))
        self.assertEqual([bool(self.perms(page).get('peer')) for page in pages], [False, True, True])
        self.assertEqual(len(self.server.clients), 3)

    async def test_a_peer_types_uses_the_clipboard_and_sizes_the_shared_screen_but_not_the_encoder(self):
        await self.open_page('holder')
        peer = await self.open_page('peer')
        allowed = ['kd,65', 'ku,65', 'm,10,20,0,0', 'cws,text/plain,5', 'REQUEST_CLIPBOARD', 'cr', 'kh,65',
                   'r,800x600,primary', 's,192']
        refused = ['cmd,xterm', '_arg_fps,30', '_f,30', 'vb,5000', 'vp,0,0,1,1', 'CLIENT_FRAME_ACK 4',
                   'SET_NATIVE_CURSOR_RENDERING,1']
        for message in allowed + refused:
            peer.say(message)
        await settle(0.1)
        self.assertEqual([message for message, _ in self.fed], allowed)
        # Size and density address the one primary display, as the holder's do.
        self.assertEqual({display for _, display in self.fed}, {'primary'})

    async def test_alternating_resizes_from_both_pages_leave_both_connected(self):
        holder = await self.open_page('holder')
        peer = await self.open_page('peer')
        entry = self.server.display_clients['primary']
        for page, size in ((holder, '1000x700'), (peer, '800x600'), (holder, '1200x800'), (peer, '900x500')):
            page.say(f'r,{size},primary')
            await settle(0.05)
            width, height = map(int, size.split('x'))
            self.assertEqual((entry['width'], entry['height']), (width, height))
        self.assertEqual(self.events.count(('reconfigure',)), 4)
        self.assertEqual((self.server.app.display_width, self.server.app.display_height), (900, 500))
        self.assertFalse(holder.closed or peer.closed)
        self.assertFalse(any(text.startswith('KILL') for text in holder.texts() + peer.texts()))
        # The holder is still the display's owner, and a repeated size is a no-op.
        self.assertIs(entry['ws'], self.server.display_clients['primary']['ws'])
        peer.say('r,900x500,primary')
        await settle(0.05)
        self.assertEqual(self.events.count(('reconfigure',)), 4)

    async def test_the_holder_keeps_its_own_controls(self):
        holder = await self.open_page('holder')
        await self.open_page('peer')
        for message in ('r,800x600,primary', 's,192', 'kd,65'):
            holder.say(message)
        await settle(0.1)
        self.assertEqual([message for message, _ in self.fed], ['r,800x600,primary', 's,192', 'kd,65'])
        self.assertTrue(all(display == 'primary' for _, display in self.fed[:3]))

    async def test_a_peer_leaving_changes_nothing_for_the_holder(self):
        holder = await self.open_page('holder')
        peer = await self.open_page('peer')
        holder.clear()
        peer.leave()
        await settle(0.2)
        self.assertEqual(self.holder(), 'holder')
        self.assertFalse(holder.closed)
        self.assertEqual(self.server.clients, {holder})
        self.assertIn('primary', self.server.display_clients)
        self.assertNotIn(peer, self.module.client_permissions)

    async def test_the_holder_leaving_hands_the_display_to_the_oldest_peer(self):
        holder = await self.open_page('holder')
        oldest = await self.open_page('oldest')
        newest = await self.open_page('newest')
        holder.leave()
        await settle(0.2)
        self.assertEqual(self.holder(), 'oldest')
        self.assertEqual(self.perms(oldest)['role'], 'controller')
        self.assertNotIn('peer', self.perms(oldest))
        self.assertTrue(self.perms(newest)['peer'])
        self.assertFalse(oldest.closed)
        self.assertFalse(newest.closed)
        self.assertEqual(self.server.display_clients['primary']['acknowledged_frame_id'], -1)
        # The handover was immediate: no teardown of the display was scheduled.
        await settle(0.2)
        self.assertIn('primary', self.server.display_clients)

    async def test_the_new_holder_acknowledges_frames_and_sizes_the_screen(self):
        holder = await self.open_page('holder')
        peer = await self.open_page('peer')
        holder.leave()
        await settle(0.2)
        self.fed.clear()
        peer.say('CLIENT_FRAME_ACK 7')
        peer.say('r,900x500,primary')
        await settle(0.1)
        self.assertEqual(self.server.display_clients['primary']['acknowledged_frame_id'], 7)
        self.assertEqual(self.fed, [('r,900x500,primary', 'primary')])

    async def test_a_reload_overlap_ends_with_the_new_page_holding_the_display(self):
        old = await self.open_page('old')
        new = await self.open_page('new')
        self.assertEqual(self.holder(), 'old')
        old.leave()
        await settle(0.2)
        self.assertEqual(self.holder(), 'new')
        self.assertFalse(new.closed)
        # A later SETTINGS from the new holder is an ordinary controller update.
        self.events.clear()
        new.say(settings_text(width=1200, height=800))
        await settle(0.1)
        self.assertIn(('apply', 'new', 'controller', False), self.events)

    async def test_a_lone_page_that_leaves_still_tears_the_display_down(self):
        only = await self.open_page('only')
        only.leave()
        await settle(0.4)
        self.assertNotIn('primary', self.server.display_clients)
        self.assertEqual(self.server.clients, set())

    async def test_a_new_page_takes_over_a_display_whose_holder_has_gone(self):
        holder = await self.open_page('holder')
        holder.leave()
        await settle(0.01)
        # Within the reconnect grace the entry is still there for the dead socket.
        late = await self.open_page('late')
        self.assertEqual(self.holder(), 'late')
        self.assertEqual(self.perms(late)['role'], 'controller')
        self.assertNotIn('peer', self.perms(late))

    async def test_secure_mode_never_hands_the_display_over(self):
        holder = await self.open_page('holder')
        await self.open_page('peer')
        self.server.is_secure_mode = True
        self.assertFalse(await self.server._silo_adopt_primary(holder))
        self.assertIs(self.server.display_clients['primary']['ws'], holder)

    async def test_a_peer_start_video_is_acknowledged_for_the_controller_page(self):
        await self.open_page('holder')
        peer = await self.open_page('peer')
        self.server.capture_instances['primary'] = {'module': mock.MagicMock()}
        peer.clear()
        peer.say('STOP_VIDEO')
        await settle(0.05)
        self.assertIn('VIDEO_STOPPED', peer.texts())
        self.assertIn(peer, self.server.video_paused_clients)
        peer.say('START_VIDEO')
        await settle(0.1)
        self.assertIn('VIDEO_STARTED', peer.texts())
        self.assertNotIn(peer, self.server.video_paused_clients)

    # ---- audio

    def audio_server(self):
        server, module = self.server, self.module
        server.stopped = 0
        server.started = 0

        async def stop():
            server.stopped += 1
            server.is_pcmflux_capturing = False

        async def start():
            server.started += 1
            server.is_pcmflux_capturing = True
            return True

        server._stop_pcmflux_pipeline = stop
        server._start_pcmflux_pipeline = start
        server.is_pcmflux_capturing = True
        patch = mock.patch.object(module, 'PCMFLUX_AVAILABLE', True)
        patch.start()
        self.addCleanup(patch.stop)

    async def test_one_page_stopping_audio_leaves_the_others_listening(self):
        self.audio_server()
        holder = await self.open_page('holder')
        peer = await self.open_page('peer')
        holder.clear()
        peer.clear()
        peer.say('STOP_AUDIO')
        await settle(0.1)
        self.assertEqual(peer.texts().count('AUDIO_STOPPED'), 1)
        self.assertNotIn('AUDIO_STOPPED', holder.texts())
        self.assertEqual(self.server.stopped, 0)
        self.assertIn(peer, self.module.audio_paused_clients)
        self.assertNotIn(holder, self.module.audio_paused_clients)

    async def test_the_last_listener_stopping_audio_stops_the_capture(self):
        self.audio_server()
        holder = await self.open_page('holder')
        peer = await self.open_page('peer')
        peer.say('STOP_AUDIO')
        await settle(0.05)
        holder.clear()
        peer.clear()
        holder.say('STOP_AUDIO')
        await settle(0.1)
        self.assertEqual(self.server.stopped, 1)
        self.assertIn('AUDIO_STOPPED', holder.texts())
        self.assertIn('AUDIO_STOPPED', peer.texts())

    async def test_starting_audio_wakes_only_the_page_that_asked(self):
        self.audio_server()
        holder = await self.open_page('holder')
        peer = await self.open_page('peer')
        peer.say('STOP_AUDIO')
        holder.say('STOP_AUDIO')
        await settle(0.1)
        self.assertEqual(self.server.stopped, 1)
        holder.clear()
        peer.clear()
        peer.say('START_AUDIO')
        await settle(0.1)
        self.assertEqual(self.server.started, 1)
        self.assertIn('AUDIO_STARTED', peer.texts())
        self.assertNotIn('AUDIO_STARTED', holder.texts())
        self.assertNotIn(peer, self.module.audio_paused_clients)
        self.assertIn(holder, self.module.audio_paused_clients)

    async def test_audio_chunks_skip_a_page_that_turned_audio_off(self):
        self.audio_server()
        holder = await self.open_page('holder')
        peer = await self.open_page('peer')
        peer.say('STOP_AUDIO')
        await settle(0.05)
        holder.clear()
        peer.clear()
        self.server.pcmflux_audio_queue = asyncio.Queue()
        sender = asyncio.create_task(self.server._pcmflux_send_audio_chunks())
        self.tasks.append(sender)
        self.server.pcmflux_audio_queue.put_nowait({'data': b'\x01\x00chunk'})
        await settle(0.1)
        self.assertIn(b'\x01\x00chunk', holder.sent)
        self.assertNotIn(b'\x01\x00chunk', peer.sent)

    async def test_a_lone_page_keeps_the_original_audio_behaviour(self):
        self.audio_server()
        only = await self.open_page('only')
        only.clear()
        only.say('STOP_AUDIO')
        await settle(0.1)
        self.assertEqual(self.server.stopped, 1)
        self.assertEqual(only.texts().count('AUDIO_STOPPED'), 1)
        only.say('START_AUDIO')
        await settle(0.1)
        self.assertEqual(self.server.started, 1)
        self.assertEqual(only.texts().count('AUDIO_STARTED'), 1)

    async def test_a_page_that_leaves_is_forgotten_by_the_audio_state(self):
        self.audio_server()
        await self.open_page('holder')
        peer = await self.open_page('peer')
        peer.say('STOP_AUDIO')
        await settle(0.05)
        self.assertIn(peer, self.module.audio_paused_clients)
        peer.leave()
        await settle(0.1)
        self.assertNotIn(peer, self.module.audio_paused_clients)


if __name__ == '__main__':
    unittest.main()
