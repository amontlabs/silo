import importlib.util
import io
import json
import os
import stat
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / 'src-tauri/guest/macos/silo-cua-register.py'
ZSH = SCRIPT.with_name('silo-computer-use.zsh')


def load(home):
    with mock.patch.dict(os.environ, {'HOME': str(home)}):
        spec = importlib.util.spec_from_file_location('silo_cua_register', SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    return module


class RegisterTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.home = Path(self.directory.name)
        self.bin = self.home / 'bin'
        self.bin.mkdir()
        self.module = load(self.home)
        self.module.SEARCH[:] = [str(self.bin)]

    def install(self, name):
        path = self.bin / name
        path.write_text('#!/bin/sh\n')
        path.chmod(path.stat().st_mode | stat.S_IXUSR)

    def apply(self, approval):
        return self.module.apply(approval)

    def test_ask_registers_without_an_approval_line(self):
        self.install('claude')
        self.install('codex')
        result = self.apply('ask')
        self.assertEqual(result['agents'], ['claude-code', 'codex'])
        self.assertEqual(result['pending'], [])
        state = json.loads(self.module.CLAUDE_STATE.read_text())
        self.assertEqual(state['mcpServers']['cua-driver'], {
            'type': 'stdio', 'command': '/usr/local/bin/cua-driver', 'args': ['mcp'],
            'env': {'CUA_DRIVER_RS_TELEMETRY_ENABLED': '0', 'CUA_DRIVER_RS_UPDATE_CHECK': 'false'}})
        self.assertFalse(self.module.CLAUDE_SETTINGS.exists())
        codex = tomllib.loads(self.module.CODEX_CONFIG.read_text())
        section = codex['mcp_servers']['cua-driver']
        self.assertEqual((section['command'], section['args']), ('/usr/local/bin/cua-driver', ['mcp']))
        self.assertNotIn('default_tools_approval_mode', section)
        self.assertEqual(section['env']['CUA_DRIVER_RS_TELEMETRY_ENABLED'], '0')

    def test_silos_own_server_is_added_beside_the_driver_and_removed_alone(self):
        self.install('claude')
        self.apply('ask')
        code, result = self.main('server-add', '--name', 'silo', '--command', '/usr/local/bin/silo', '--arg', 'mcp')
        self.assertEqual((code, result['registered'], result['failed']), (0, ['claude-code', 'codex'], []))
        servers = json.loads(self.module.CLAUDE_STATE.read_text())['mcpServers']
        self.assertEqual(servers['silo'], {'type': 'stdio', 'command': '/usr/local/bin/silo', 'args': ['mcp'], 'env': {}})
        self.assertIn('cua-driver', servers)
        codex = tomllib.loads(self.module.CODEX_CONFIG.read_text())['mcp_servers']
        self.assertEqual((codex['silo']['command'], codex['silo']['args']), ('/usr/local/bin/silo', ['mcp']))
        self.assertIn('cua-driver', codex)
        code, result = self.main('server-remove', '--name', 'silo')
        self.assertEqual((code, result['removed']), (0, ['claude-code', 'codex']))
        self.assertNotIn('silo', json.loads(self.module.CLAUDE_STATE.read_text())['mcpServers'])
        self.assertIn('cua-driver', json.loads(self.module.CLAUDE_STATE.read_text())['mcpServers'])
        self.assertNotIn('silo', tomllib.loads(self.module.CODEX_CONFIG.read_text())['mcp_servers'])
        self.assertIn('cua-driver', tomllib.loads(self.module.CODEX_CONFIG.read_text())['mcp_servers'])

    def test_a_server_name_or_command_that_is_not_plain_is_refused(self):
        for name, command in (('cua-driver', '/x'), ('Silo', '/x'), ('silo', 'silo')):
            with mock.patch('sys.stdout', new=io.StringIO()), mock.patch('sys.stderr', new=io.StringIO()):
                code = self.module.main(['server-add', '--name', name, '--command', command])
            self.assertEqual(code, 1, (name, command))
        self.assertFalse(self.module.CLAUDE_STATE.exists())

    def test_auto_adds_the_approval_lines_and_ask_removes_only_those(self):
        self.install('claude')
        self.install('codex')
        self.module.CLAUDE_SETTINGS.parent.mkdir(parents=True)
        self.module.CLAUDE_SETTINGS.write_text(json.dumps({'permissions': {'allow': ['Bash(ls)']}}))
        self.apply('auto')
        settings = json.loads(self.module.CLAUDE_SETTINGS.read_text())
        self.assertEqual(settings['permissions']['allow'], ['Bash(ls)', 'mcp__cua-driver'])
        self.assertIn('default_tools_approval_mode = "approve"', self.module.CODEX_CONFIG.read_text())
        # Running it again changes nothing.
        before = (self.module.CLAUDE_SETTINGS.read_text(), self.module.CODEX_CONFIG.read_text())
        self.apply('auto')
        self.assertEqual(before, (self.module.CLAUDE_SETTINGS.read_text(), self.module.CODEX_CONFIG.read_text()))
        self.apply('ask')
        settings = json.loads(self.module.CLAUDE_SETTINGS.read_text())
        self.assertEqual(settings['permissions']['allow'], ['Bash(ls)'])
        self.assertNotIn('approve', self.module.CODEX_CONFIG.read_text())

    def main(self, *argv):
        """Runs the command line; returns `(exit code, result object)`."""
        with mock.patch('sys.stdout', new=io.StringIO()) as out, mock.patch('sys.stderr', new=io.StringIO()):
            code = self.module.main(list(argv))
        return code, json.loads(out.getvalue())

    def test_an_agent_that_is_not_installed_is_registered_all_the_same(self):
        # Codex is not installed: its configuration is written anyway, so a Codex installed into
        # the running computer finds the server at its first start without another login.
        self.install('claude')
        code, result = self.main('apply', '--approval', 'auto')
        self.assertEqual(code, 0)
        self.assertEqual((result['agents'], result['pending'], result['failed']), (['claude-code', 'codex'], [], []))
        codex = tomllib.loads(self.module.CODEX_CONFIG.read_text())
        self.assertEqual(codex['mcp_servers']['cua-driver']['default_tools_approval_mode'], 'approve')
        self.assertEqual(self.module.reconcile(), {'reconcile': 'idle'})

    def test_an_absent_agent_whose_files_cannot_be_written_is_pending_until_reconcile(self):
        self.install('claude')
        # `~/.codex` is a file, so Codex's configuration cannot be created.
        self.module.CODEX_CONFIG.parent.write_text('')
        code, result = self.main('apply', '--approval', 'auto')
        self.assertEqual(code, 0, 'an absent agent does not fail the setup')
        self.assertEqual((result['agents'], result['pending'], result['failed']), (['claude-code'], ['codex'], []))
        self.module.RECEIPT.parent.mkdir(parents=True, exist_ok=True)
        self.module.RECEIPT.write_text(json.dumps({'schemaVersion': 2, 'state': 'ready', 'agents': ['claude-code'],
                                                   'pending': ['codex']}))
        self.assertEqual(self.module.reconcile(), {'reconcile': 'done', 'agents': ['claude-code'], 'pending': ['codex']})
        self.module.CODEX_CONFIG.parent.unlink()
        result = self.module.reconcile()
        self.assertEqual((result['agents'], result['pending']), (['claude-code', 'codex'], []))
        self.assertIn('default_tools_approval_mode = "approve"', self.module.CODEX_CONFIG.read_text())
        receipt = json.loads(self.module.RECEIPT.read_text())
        self.assertEqual((receipt['agents'], receipt['pending'], receipt['state']), (['claude-code', 'codex'], [], 'ready'))
        self.assertEqual(self.module.reconcile(), {'reconcile': 'idle'})

    def test_an_installed_agent_whose_files_cannot_be_updated_fails_the_setup(self):
        # Claude Code is installed with the auto rule in place, and its state file cannot be
        # rewritten: switching to `ask` would leave the rule in force, so the setup must fail.
        self.install('claude')
        self.install('codex')
        self.module.CLAUDE_SETTINGS.parent.mkdir(parents=True)
        self.module.CLAUDE_SETTINGS.write_text(json.dumps({'permissions': {'allow': ['mcp__cua-driver']}}))
        self.module.CLAUDE_STATE.write_text('[]')
        code, result = self.main('apply', '--approval', 'ask')
        self.assertEqual(code, 1)
        self.assertEqual((result['agents'], result['pending'], result['failed']), (['codex'], ['claude-code'], ['claude-code']))
        settings = json.loads(self.module.CLAUDE_SETTINGS.read_text())
        self.assertEqual(settings['permissions']['allow'], ['mcp__cua-driver'], 'the stale rule is what the failure reports')

    def test_lcu_leftovers_are_removed(self):
        self.install('claude')
        self.install('codex')
        self.module.CLAUDE_STATE.write_text(json.dumps({'mcpServers': {'lcu': {'command': 'node'}, 'other': {}}}))
        self.module.CLAUDE_SETTINGS.parent.mkdir(parents=True)
        self.module.CLAUDE_SETTINGS.write_text(json.dumps({
            'permissions': {'allow': ['mcp__lcu', 'Bash(ls)'], 'deny': ['mcp__lcu__host_only']},
            'hooks': {'Stop': [{'hooks': [{'type': 'command', 'command': '/Users/silo/.local/share/lcu/current/bin/lcu hook'}]},
                               {'hooks': [{'type': 'command', 'command': 'echo keep'}]}]}}))
        self.module.CODEX_CONFIG.parent.mkdir(parents=True)
        self.module.CODEX_CONFIG.write_text(
            '[mcp_servers.lcu]\ncommand = "lcu"\n\n[mcp_servers.lcu.env]\nA = "b"\n\n[mcp_servers.keep]\ncommand = "k"\n')
        self.apply('ask')
        state = json.loads(self.module.CLAUDE_STATE.read_text())
        self.assertEqual(sorted(state['mcpServers']), ['cua-driver', 'other'])
        settings = json.loads(self.module.CLAUDE_SETTINGS.read_text())
        self.assertEqual(settings['permissions'], {'allow': ['Bash(ls)'], 'deny': []})
        self.assertEqual(len(settings['hooks']['Stop']), 1)
        codex = self.module.CODEX_CONFIG.read_text()
        self.assertNotIn('lcu', codex)
        self.assertEqual(sorted(tomllib.loads(codex)['mcp_servers']), ['cua-driver', 'keep'])

    def test_lcus_mcp_lifecycle_hooks_and_their_codex_trust_are_removed(self):
        # The hooks `lcu setup` wrote: MCP tool hooks for its `lcu` server, and Codex's trust for them.
        self.install('claude')
        self.install('codex')
        lcu_hook = lambda tool: {'type': 'mcp_tool', 'server': 'lcu', 'tool': tool, 'input': {'session_id': '${session_id}'}}
        self.module.CLAUDE_SETTINGS.parent.mkdir(parents=True)
        self.module.CLAUDE_SETTINGS.write_text(json.dumps({
            'permissions': {'deny': []},
            'hooks': {'PreToolUse': [{'matcher': 'mcp__lcu__js|mcp__lcu__js_reset', 'hooks': [lcu_hook('set_turn_context')]}],
                      'Stop': [{'hooks': [lcu_hook('turn_ended')]},
                               {'hooks': [{'type': 'mcp_tool', 'server': 'lcu-notes', 'tool': 'turn_ended'}]}],
                      'StopFailure': [{'hooks': [lcu_hook('turn_ended')]}],
                      'SubagentStop': [{'hooks': [lcu_hook('turn_ended')]}]}}))
        config = str(self.module.CODEX_CONFIG)
        hook = '[{ hooks = [{ type = "mcp_tool", server = "lcu", tool = "turn_ended", input = { turn_id = "${turn_id}" } }] }]'
        self.module.CODEX_CONFIG.parent.mkdir(parents=True)
        self.module.CODEX_CONFIG.write_text(
            '[hooks]\nInterrupt = %s\nStop = %s\nSubagentStop = %s\n\n' % (hook, hook, hook)
            + ''.join('[hooks.state."%s:%s:0:0"]\ntrusted_hash = "sha256:00"\n\n' % (config, event)
                      for event in ('subagent_stop', 'stop', 'interrupt', 'session_start', 'user_prompt_submit'))
            + '[hooks.state."/Users/silo/project/.codex/config.toml:stop:0:0"]\ntrusted_hash = "sha256:11"\n\n'
            + '[hooks.SessionStart]\ncommand = "echo keep"\n')
        self.apply('ask')
        settings = json.loads(self.module.CLAUDE_SETTINGS.read_text())
        self.assertEqual(settings['hooks'], {'Stop': [{'hooks': [{'type': 'mcp_tool', 'server': 'lcu-notes', 'tool': 'turn_ended'}]}]})
        text = self.module.CODEX_CONFIG.read_text()
        self.assertNotIn('server = "lcu"', text)
        codex = tomllib.loads(text)
        self.assertEqual(sorted(codex['hooks']), ['SessionStart', 'state'])
        self.assertEqual(sorted(codex['hooks']['state']),
                         sorted(['%s:session_start:0:0' % config, '/Users/silo/project/.codex/config.toml:stop:0:0']))
        self.assertEqual(sorted(codex['mcp_servers']), ['cua-driver'])

    def test_lcus_codex_hooks_go_but_hooks_sharing_their_event_stay(self):
        self.install('codex')
        lcu = '{ type = "mcp_tool", server = "lcu", tool = "turn_ended", input = { turn_id = "${turn_id}" } }'
        mine = '{ type = "command", command = "echo \\"done, [ok]\\"" }'
        self.module.CODEX_CONFIG.parent.mkdir(parents=True)
        self.module.CODEX_CONFIG.write_text(
            '[hooks]\n'
            'Stop = [{ hooks = [%s] }, { hooks = [%s] }]  # mine\n' % (lcu, mine)
            + 'SubagentStop = [{ matcher = "x", hooks = [%s, %s] }]\n' % (lcu, mine)
            + 'Interrupt = [{ hooks = [%s] }]\n' % lcu)
        self.apply('ask')
        text = self.module.CODEX_CONFIG.read_text()
        self.assertNotIn('server = "lcu"', text)
        self.assertIn('# mine', text)
        hooks = tomllib.loads(text)['hooks']
        command = {'type': 'command', 'command': 'echo "done, [ok]"'}
        self.assertEqual(hooks, {'Stop': [{'hooks': [command]}],
                                 'SubagentStop': [{'matcher': 'x', 'hooks': [command]}]})

    def test_removal_keeps_rules_and_hooks_that_merely_mention_lcu(self):
        self.install('claude')
        self.install('codex')
        self.module.CLAUDE_SETTINGS.parent.mkdir(parents=True)
        self.module.CLAUDE_SETTINGS.write_text(json.dumps({
            'permissions': {'allow': ['mcp__lcutils', 'Read(~/lcu-notes/**)', 'mcp__lcu', 'mcp__lcu__host_only',
                                      'mcp__lcutils__run', 'Bash(~/.local/bin/lcu-notes:*)']},
            'hooks': {'Stop': [{'hooks': [{'type': 'command', 'command': 'cp ~/Library/Logs/silo.log ~/lcu-archive/'}]},
                               {'hooks': [{'type': 'command', 'command': '/Users/silo/.local/share/lcu/current/bin/lcu hook'}]},
                               {'hooks': [{'type': 'command', 'command': '/Users/silo/.local/bin/lcu hook'}]},
                               {'hooks': [{'type': 'command', 'command': 'open /Applications/ChatGPT.app'}]}]}}))
        self.module.CODEX_CONFIG.parent.mkdir(parents=True)
        self.module.CODEX_CONFIG.write_text(
            '[mcp_servers.lcutils]\ncommand = "lcutils"\n\n[mcp_servers.lcu]\ncommand = "lcu"\n\n'
            '[hooks.archive]\ncommand = "tar -C ~/lcu-archive"\n\n[hooks.lcu]\ncommand = "~/.local/state/lcu/hook"\n')
        self.apply('ask')
        settings = json.loads(self.module.CLAUDE_SETTINGS.read_text())
        self.assertEqual(settings['permissions']['allow'],
                         ['mcp__lcutils', 'Read(~/lcu-notes/**)', 'mcp__lcutils__run', 'Bash(~/.local/bin/lcu-notes:*)'])
        self.assertEqual([h['hooks'][0]['command'] for h in settings['hooks']['Stop']],
                         ['cp ~/Library/Logs/silo.log ~/lcu-archive/'])
        codex = tomllib.loads(self.module.CODEX_CONFIG.read_text())
        self.assertEqual(sorted(codex['mcp_servers']), ['cua-driver', 'lcutils'])
        self.assertEqual(sorted(codex['hooks']), ['archive'])

    def test_removal_does_not_create_files(self):
        self.module.remove_legacy()
        self.assertFalse(self.module.CLAUDE_STATE.exists() or self.module.CODEX_CONFIG.exists())

    def test_privacy_is_persisted_in_cua_drivers_config(self):
        self.module.CUA_HOME.mkdir()
        (self.module.CUA_HOME / 'config.json').write_text(json.dumps({'other': 1, 'telemetry_enabled': True}))
        self.module.privacy()
        config = json.loads((self.module.CUA_HOME / 'config.json').read_text())
        self.assertEqual(config, {'other': 1, 'telemetry_enabled': False, 'update_check_enabled': False})

    def test_a_config_that_is_not_an_object_fails_that_agent_only(self):
        self.install('claude')
        self.install('codex')
        self.module.CLAUDE_STATE.write_text('[]')
        result = self.apply('ask')
        self.assertEqual((result['agents'], result['failed']), (['codex'], ['claude-code']))
        self.assertEqual(result['pending'], ['claude-code'])
        self.assertEqual(self.main('apply', '--approval', 'ask')[0], 1, 'an installed agent that failed fails the run')


class ScriptSourceTest(unittest.TestCase):
    def test_the_script_runs_the_helper_and_pins_the_app_identity(self):
        text = ZSH.read_text()
        for needle in ('silo-cua-register.py', 'com.trycua.driver', 'doctor --json', 'permissions status --json',
                       'CUA_DRIVER_RS_TELEMETRY_ENABLED=0', 'CUA_DRIVER_RS_UPDATE_CHECK=false', 'codesign --verify --deep --strict'):
            self.assertIn(needle, text.replace('$(pinned bundleIdentifier)', 'com.trycua.driver'))
        self.assertNotIn('lcu setup', text)
        self.assertNotIn('--runtime-only', text)
        # Only LCU's own executables leave ~/.local/bin, never every name that starts with "lcu".
        self.assertNotIn('bin/lcu*', text)
        self.assertIn('legacy_executables', text)

    def test_the_lock_matches_the_facts_of_the_release(self):
        lock = json.loads(ZSH.with_name('cua-driver-lock.json').read_text())
        self.assertEqual((lock['schemaVersion'], lock['version'], lock['teamIdentifier'], lock['bundleIdentifier']),
                         (1, '0.38.0', 'YCK386LBJ7', 'com.trycua.driver'))
        asset = lock['assets']['universal']
        self.assertEqual(asset['sha256'], '121cf20cafd2cfa69dd720e8967350c6406b22e558be6889de7a1ae3343b6d4b')
        self.assertTrue(asset['url'].endswith('/cua-driver-rs-v0.38.0/cua-driver-rs-0.38.0-darwin-universal.tar.gz'))
        self.assertEqual(asset['bytes'], 85040507)


if __name__ == '__main__':
    unittest.main()
