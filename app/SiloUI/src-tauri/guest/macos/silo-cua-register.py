#!/usr/bin/python3
"""Registers Cua Driver with the agents of a Silo macOS computer.

Silo copies this file next to `silo-computer-use.zsh` and the script runs it as the working
account (`silo`) with the Command Line Tools' Python 3 (3.9 or later; nothing newer is used):

  silo-cua-register.py apply --approval ask|auto    register, remove LCU's leftovers
  silo-cua-register.py reconcile                    register agents installed since `apply`
  silo-cua-register.py privacy                      switch Cua Driver's telemetry and update check off
  silo-cua-register.py server-add --name N --command C [--arg A]...
                                                    register another stdio MCP server (Silo's own)
  silo-cua-register.py server-remove --name N       take that server out of the agents

It registers the driver as a plain stdio MCP server named `cua-driver` for Claude Code
(`~/.claude.json`) and Codex (`~/.codex/config.toml`), exactly like the Linux helper does. The
command is the `cua-driver` link on PATH; the app launches its own daemon through
LaunchServices when the first MCP session starts. The approval switch adds or removes
`mcp__cua-driver` in Claude Code's `permissions.allow` and
`default_tools_approval_mode = "approve"` in Codex's section. Both files are written whether or
not the agent is installed yet, so an agent installed into a running computer finds the server
at its first start. `apply` exits 1 when an installed agent's configuration could not be
updated (the computer-use setup then fails instead of reporting the approval as applied); an
agent that is absent and whose files could not be written is pending, and the reconcile
LaunchAgent tries again at the next login.

State: `~/Library/Application Support/Silo/computer-use-agents.json` holds the approval and the
registered and pending agents; `reconcile` also updates `agents` and `pending` in the receipt.
Output: one JSON object on stdout. Agents hold the working account's full rights, so the
approval switch is a convenience, not a security boundary.
"""
import argparse
import json
import os
import re
import sys
import tempfile
import time
from pathlib import Path

HOME = Path(os.environ.get('HOME') or os.path.expanduser('~'))
SUPPORT = HOME / 'Library/Application Support/Silo'
AGENT_STATE = SUPPORT / 'computer-use-agents.json'
RECEIPT = SUPPORT / 'computer-use-receipt.json'
SERVER = 'cua-driver'
DRIVER = '/usr/local/bin/cua-driver'
CLAUDE_STATE = HOME / '.claude.json'
CLAUDE_SETTINGS = HOME / '.claude/settings.json'
CODEX_CONFIG = HOME / '.codex/config.toml'
CUA_HOME = HOME / '.cua-driver'
APPROVAL_RULE = 'mcp__' + SERVER
APPROVALS = ('ask', 'auto')
# The agents Silo registers, and the executable that shows each is installed.
AGENTS = {'claude-code': 'claude', 'codex': 'codex'}
SEARCH = [str(HOME / d) for d in ('.local/bin', '.bun/bin', '.cargo/bin', '.npm-global/bin')] + [
    '/usr/local/bin', '/opt/homebrew/bin', '/usr/bin', '/bin']
PRIVACY_ENV = {'CUA_DRIVER_RS_TELEMETRY_ENABLED': '0', 'CUA_DRIVER_RS_UPDATE_CHECK': 'false'}
# What the earlier LCU setup left in an agent's configuration. Only what LCU itself wrote: its
# MCP tools (`mcp__lcu`, `mcp__lcu__<tool>`) and commands under the paths the macOS setup
# installed it at (`~/.local/share/lcu`, `~/.local/state/lcu`, `~/.local/bin/lcu`, the Hermes
# plugin, the ChatGPT app). A user's own rule or hook that merely mentions "lcu" is kept.
LEGACY = re.compile(r'(?<![A-Za-z0-9_])mcp__lcu(?:__[A-Za-z0-9_-]+)?(?![A-Za-z0-9_-])'
                    r'|/\.local/(?:share|state)/lcu(?=/|$|[\s"\':])'
                    r'|/\.local/bin/lcu(?=$|[\s"\':])'
                    r'|/\.hermes/plugins/lcu-cua(?=/|$|[\s"\':])'
                    r'|/Applications/ChatGPT\.app(?=/|$|[\s"\':])')
# LCU's turn lifecycle hooks are MCP tool hooks addressed to its server, `lcu`: JSON in Claude
# Code's settings, an inline table on one line of Codex's `[hooks]` table.
LEGACY_HOOK_SERVER = re.compile(r'(?<![A-Za-z0-9_])server\s*=\s*"lcu"')


class Failure(Exception):
    pass


def write_text(path, text, mode=0o600):
    """Atomically replaces `path` (an existing file keeps its permissions)."""
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        mode = path.stat().st_mode & 0o777
    except OSError:
        pass
    fd, temporary = tempfile.mkstemp(prefix='.silo-', dir=str(path.parent))
    try:
        with os.fdopen(fd, 'w') as output:
            output.write(text)
            output.flush()
            os.fchmod(output.fileno(), mode)
            os.fsync(output.fileno())
        os.replace(temporary, str(path))
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def read_object(path):
    """The JSON object in `path`; `{}` when the file is absent; `None` when it is not an object."""
    try:
        text = path.read_text()
    except FileNotFoundError:
        return {}
    value = json.loads(text) if text.strip() else {}
    return value if isinstance(value, dict) else None


def server_entry():
    return {'type': 'stdio', 'command': DRIVER, 'args': ['mcp'], 'env': dict(sorted(PRIVACY_ENV.items()))}


def legacy_hook(hook):
    """Whether a Claude Code hook is LCU's: an MCP tool hook for the `lcu` server, or a command
    that names LCU's files."""
    if isinstance(hook, dict) and hook.get('type') == 'mcp_tool' and hook.get('server') == 'lcu':
        return True
    return bool(LEGACY.search(json.dumps(hook)))


def strip_legacy_rules(settings):
    """Removes LCU's permission rules and hooks from Claude Code settings."""
    permissions = settings.get('permissions')
    if isinstance(permissions, dict):
        for kind in ('allow', 'deny', 'ask'):
            rules = permissions.get(kind)
            if isinstance(rules, list):
                permissions[kind] = [r for r in rules if not (isinstance(r, str) and LEGACY.search(r))]
    hooks = settings.get('hooks')
    if isinstance(hooks, dict):
        for event in list(hooks):
            entries = hooks[event]
            if not isinstance(entries, list):
                continue
            kept = []
            for entry in entries:
                inner = entry.get('hooks') if isinstance(entry, dict) else None
                if isinstance(inner, list):
                    left = [h for h in inner if not legacy_hook(h)]
                    if len(left) != len(inner):
                        if not left:
                            continue
                        entry = dict(entry, hooks=left)
                elif legacy_hook(entry):
                    continue
                kept.append(entry)
            if kept:
                hooks[event] = kept
            else:
                del hooks[event]
        if not hooks:
            del settings['hooks']


def edit_claude(entry, approval, server=SERVER, remove=False):
    """Claude Code: `mcpServers.cua-driver` in ~/.claude.json (`entry`; None leaves the
    registration alone) and `mcp__cua-driver` in ~/.claude/settings.json `permissions.allow`
    for the auto mode. LCU's entries are removed from both. Another `server` (Silo's own MCP
    server) is only added to or, with `remove`, taken out of ~/.claude.json."""
    state = read_object(CLAUDE_STATE)
    if state is None:
        raise Failure('%s is not a JSON object' % CLAUDE_STATE)
    original = json.dumps(state, sort_keys=True)
    servers = state.get('mcpServers')
    if servers is not None and not isinstance(servers, dict):
        raise Failure('mcpServers is not an object')
    if isinstance(servers, dict):
        servers.pop('lcu', None)
        if remove:
            servers.pop(server, None)
    if entry is not None:
        state.setdefault('mcpServers', {})[server] = entry
    if json.dumps(state, sort_keys=True) != original:
        write_text(CLAUDE_STATE, json.dumps(state, indent=2) + '\n')
    if server != SERVER:
        return
    settings = read_object(CLAUDE_SETTINGS)
    if settings is None:
        raise Failure('%s is not a JSON object' % CLAUDE_SETTINGS)
    original = json.dumps(settings, sort_keys=True)
    strip_legacy_rules(settings)
    permissions = settings.get('permissions')
    if permissions is not None and not isinstance(permissions, dict):
        raise Failure('permissions is not an object')
    allow = (permissions or {}).get('allow')
    if allow is not None and not isinstance(allow, list):
        raise Failure('permissions.allow is not a list')
    if approval == 'auto' and entry is not None:
        permissions = settings.setdefault('permissions', {})
        if APPROVAL_RULE not in (allow or []):
            permissions['allow'] = list(allow or []) + [APPROVAL_RULE]
    elif approval == 'ask' and allow and APPROVAL_RULE in allow:
        permissions['allow'] = [rule for rule in allow if rule != APPROVAL_RULE]
        # Leave no empty containers behind for a rule only Silo added.
        if not permissions['allow']:
            del permissions['allow']
        if not permissions:
            del settings['permissions']
    if json.dumps(settings, sort_keys=True) != original:
        write_text(CLAUDE_SETTINGS, json.dumps(settings, indent=2) + '\n')


TOML_HEADER = re.compile(r'^\s*\[\[?\s*([^\[\]]+?)\s*\]\]?\s*(?:#.*)?$')
LEGACY_SERVER = re.compile(r'^mcp_servers\.("?)lcu\1(\.|$)')


def own_server(server):
    """The header of Codex's table for `server` (or one of its sub-tables)."""
    return re.compile(r'^mcp_servers\.("?)' + re.escape(server) + r'\1(\.|$)')


OWN_SERVER = own_server(SERVER)
# Codex keeps the trust of a hook from config.toml as `[hooks.state."<file>:<event>:<group>:<hook>"]`.
HOOK_TRUST = re.compile(r'^hooks\.state\."(.*):([a-z_]+):\d+:\d+"$')
HOOK_EVENT = re.compile(r'^\s*"?([A-Za-z]+)"?\s*=')


def snake_case(event):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', event).lower()


def legacy_toml_hook(text):
    return bool(LEGACY.search(text) or LEGACY_HOOK_SERVER.search(text))


def toml_items(body):
    """The comma-separated items of an inline TOML array or table body, split at depth 0 so
    strings and nested arrays or tables stay whole."""
    items, depth, quote, start, i = [], 0, None, 0, 0
    while i < len(body):
        char = body[i]
        if quote:
            if quote == '"' and char == '\\':
                i += 1
            elif char == quote:
                quote = None
        elif char in '"\'':
            quote = char
        elif char in '[{':
            depth += 1
        elif char in ']}':
            depth -= 1
        elif char == ',' and depth == 0:
            items.append(body[start:i].strip())
            start = i + 1
        i += 1
    items.append(body[start:].strip())
    return [item for item in items if item]


HOOK_LINE = re.compile(r'^(\s*"?[A-Za-z]+"?\s*=\s*)\[(.*)\](\s*(?:#.*)?)$')
HOOK_GROUP = re.compile(r'^\{(.*)\}$', re.S)
HOOK_LIST = re.compile(r'^(hooks\s*=\s*)\[(.*)\]$', re.S)


def strip_legacy_hook_line(line):
    """One event of Codex's `[hooks]` table without LCU's hooks: the line as it is, the line with
    only the other hooks left, or None when nothing is left."""
    if not legacy_toml_hook(line):
        return line
    match = HOOK_LINE.match(line)
    if not match:
        return None
    groups = []
    for group in toml_items(match.group(2)):
        if not legacy_toml_hook(group):
            groups.append(group)
            continue
        table = HOOK_GROUP.match(group)
        kept = []
        for field in toml_items(table.group(1)) if table else []:
            hooks = HOOK_LIST.match(field)
            if hooks:
                left = [h for h in toml_items(hooks.group(2)) if not legacy_toml_hook(h)]
                if not left:
                    kept = []
                    break
                field = '%s[%s]' % (hooks.group(1), ', '.join(left))
            elif legacy_toml_hook(field):
                kept = []
                break
            kept.append(field)
        if kept:
            groups.append('{ %s }' % ', '.join(kept))
    if not groups:
        return None
    return '%s[%s]%s' % (match.group(1), ', '.join(groups), match.group(3))


def stale_trust(name, events):
    """Whether section `name` is Codex's trust for a hook of this config file under an event that
    has no hook left in it."""
    match = HOOK_TRUST.match(name or '')
    return bool(match) and match.group(1) == str(CODEX_CONFIG) and match.group(2) not in events


def toml_sections(text):
    """The file as `(header name or None, lines)` chunks; the first has no header."""
    sections = [(None, [])]
    for line in text.splitlines():
        match = TOML_HEADER.match(line)
        if match:
            sections.append((match.group(1), [line]))
        else:
            sections[-1][1].append(line)
    return sections


def toml_string(value):
    return json.dumps(value)


def codex_block(entry, approval, server=SERVER):
    lines = ['[mcp_servers.%s]' % server, 'command = %s' % toml_string(entry['command']),
             'args = [' + ', '.join(toml_string(a) for a in entry['args']) + ']']
    if approval == 'auto':
        lines.append('default_tools_approval_mode = "approve"')
    if entry['env']:
        lines += ['', '[mcp_servers.%s.env]' % server]
        lines += ['%s = %s' % (key, toml_string(value)) for key, value in entry['env'].items()]
    return lines


def check_toml(text):
    """Fails when `text` does not parse. The Command Line Tools' Python predates `tomllib`;
    without it the structure Silo writes is trusted."""
    try:
        import tomllib
    except ImportError:
        return
    try:
        tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise Failure('the Codex configuration would not parse: %s' % error)


def edit_codex(entry, approval, server=SERVER, remove=False):
    """Codex: `[mcp_servers.cua-driver]` in ~/.codex/config.toml with its env table, and
    `default_tools_approval_mode = "approve"` for the auto mode. Silo owns that section and
    rewrites it; LCU's server table and hooks are removed. Another `server` has its own table
    written (or, with `remove`, taken out) the same way."""
    own = own_server(server)
    try:
        original = CODEX_CONFIG.read_text()
    except FileNotFoundError:
        original = ''
    kept = []
    for name, lines in toml_sections(original):
        if name is not None:
            if LEGACY_SERVER.match(name) or ((entry is not None or remove) and own.match(name)):
                continue
            if (name.split('.')[0] == 'hooks' and name != 'hooks' and not name.startswith('hooks.state.')
                    and legacy_toml_hook('\n'.join([name] + lines))):
                continue
            if name == 'hooks':
                lines = [line for line in map(strip_legacy_hook_line, lines) if line is not None]
                if not any(line.strip() for line in lines[1:]):
                    continue
        kept.append((name, lines))
    # The trust Codex recorded for a hook of this file whose event has no hook left here is stale:
    # it belonged to the LCU hooks removed above (trust for other files' hooks stays).
    events = set()
    for name, lines in kept:
        if name == 'hooks':
            events.update(snake_case(m.group(1)) for m in map(HOOK_EVENT.match, lines[1:]) if m)
        elif name is not None and name.split('.')[0] == 'hooks' and not name.startswith('hooks.state.'):
            events.add(snake_case(name.split('.')[1].strip('"')))
    kept = [(name, lines) for name, lines in kept if not stale_trust(name, events)]
    text = '\n'.join(line for _, lines in kept for line in lines).rstrip()
    if entry is not None:
        text = (text + '\n\n' if text else '') + '\n'.join(codex_block(entry, approval, server))
    text = text + '\n' if text else ''
    check_toml(text)
    if text != (original.rstrip() + '\n' if original.strip() else ''):
        write_text(CODEX_CONFIG, text)


EDITORS = {'claude-code': edit_claude, 'codex': edit_codex}


def installed(name, directories=None):
    for directory in directories or SEARCH:
        path = Path(directory) / name
        if path.is_file() and os.access(str(path), os.X_OK):
            return True
    return False


def configure(names, approval, directories=None):
    """Registers the driver with every agent in `names`, installed or not: the configuration
    files are written either way. Returns `(registered, pending, failed)`: `failed` holds an
    installed agent whose files could not be updated, `pending` an absent one whose files could
    not be written (the next login tries again)."""
    entry = server_entry()
    registered, pending, failed = [], [], []
    for name in names:
        try:
            EDITORS[name](entry, approval)
            registered.append(name)
        except (Failure, OSError, ValueError) as error:
            sys.stderr.write('could not register %s with %s: %s: %s\n' % (SERVER, name, type(error).__name__, error))
            (failed if installed(AGENTS[name], directories) else pending).append(name)
    return registered, pending, failed


SERVER_NAME = re.compile(r'[a-z][a-z0-9-]{0,31}')


def valid_server(name, command=None, args=()):
    if name == SERVER or not SERVER_NAME.fullmatch(name):
        raise Failure('%r is not a server name Silo may register' % name)
    if command is not None and (not command.startswith('/') or '\0' in command or any('\0' in a for a in args)):
        raise Failure('the command must be an absolute path')


def configure_server(name, command, args, directories=None):
    """Registers the plain stdio MCP server `name` with both agents, installed or not (the
    files are written either way), like `configure`. Returns `(registered, pending, failed)`."""
    valid_server(name, command, args)
    entry = {'type': 'stdio', 'command': command, 'args': list(args), 'env': {}}
    registered, pending, failed = [], [], []
    for agent in sorted(AGENTS):
        try:
            EDITORS[agent](entry, None, server=name)
            registered.append(agent)
        except (Failure, OSError, ValueError) as error:
            sys.stderr.write('could not register %s with %s: %s: %s\n' % (name, agent, type(error).__name__, error))
            (failed if installed(AGENTS[agent], directories) else pending).append(agent)
    return registered, pending, failed


def remove_server(name):
    """Takes the server `name` out of both agents' configuration (a file that does not exist is
    not created). Returns `(removed, failed)`."""
    valid_server(name)
    removed, failed = [], []
    for agent in sorted(AGENTS):
        try:
            EDITORS[agent](None, None, server=name, remove=True)
            removed.append(agent)
        except (Failure, OSError, ValueError) as error:
            sys.stderr.write('could not remove %s from %s: %s: %s\n' % (name, agent, type(error).__name__, error))
            failed.append(agent)
    return removed, failed


def remove_legacy():
    """Removes the entries the earlier LCU setup left in the agents' configuration. A file that
    does not exist is not created; Silo's own registration and approval stay as they are."""
    for name, editor in EDITORS.items():
        try:
            editor(None, None)
        except (Failure, OSError, ValueError) as error:
            sys.stderr.write('could not remove the LCU entries for %s: %s\n' % (name, error))


def privacy():
    """Persists the telemetry and update-check opt-outs in Cua Driver's own config, which the
    daemon reads although LaunchServices starts it without this session's environment."""
    path = CUA_HOME / 'config.json'
    config = read_object(path)
    if config is None:
        raise Failure('%s is not a JSON object' % path)
    wanted = {'telemetry_enabled': False, 'update_check_enabled': False}
    if any(config.get(key) is not wanted[key] for key in wanted):
        config.update(wanted)
        write_text(path, json.dumps(config, indent=2) + '\n')


def now():
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


def save_state(approval, agents, pending):
    write_text(AGENT_STATE, json.dumps({'approval': approval, 'agents': sorted(agents), 'pending': sorted(pending)}) + '\n')


def apply(approval):
    """Registers every agent. The result's `failed` names installed agents whose files could
    not be updated; the caller fails the setup for those. An absent agent whose files could not
    be written stays pending, so the next login tries again."""
    remove_legacy()
    registered, pending, failed = configure(sorted(AGENTS), approval)
    save_state(approval, registered, pending + failed)
    return {'approval': approval, 'agents': sorted(registered), 'pending': sorted(pending + failed), 'failed': sorted(failed)}


def reconcile():
    state = read_object(AGENT_STATE) or {}
    approval = state.get('approval') if state.get('approval') in APPROVALS else None
    pending = [n for n in state.get('pending') or [] if n in AGENTS]
    if approval is None or not pending:
        return {'reconcile': 'idle'}
    registered, still, failed = configure(sorted(pending), approval)
    agents = sorted(set(state.get('agents') or []) | set(registered))
    left = sorted((set(pending) - set(registered)) | set(still) | set(failed))
    save_state(approval, agents, left)
    receipt = read_object(RECEIPT)
    if receipt:
        receipt.update({'agents': agents, 'pending': left, 'updatedAt': now()})
        write_text(RECEIPT, json.dumps(receipt) + '\n')
    return {'reconcile': 'done', 'agents': agents, 'pending': left}


def main(argv=None):
    parser = argparse.ArgumentParser(prog='silo-cua-register')
    sub = parser.add_subparsers(dest='command', required=True)
    first = sub.add_parser('apply')
    first.add_argument('--approval', choices=APPROVALS, required=True)
    sub.add_parser('reconcile')
    sub.add_parser('privacy')
    add = sub.add_parser('server-add')
    add.add_argument('--name', required=True)
    add.add_argument('--command', dest='command_path', required=True)
    add.add_argument('--arg', action='append', default=[])
    remove = sub.add_parser('server-remove')
    remove.add_argument('--name', required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == 'server-add':
            registered, pending, failed = configure_server(args.name, args.command_path, args.arg)
            print(json.dumps({'server': args.name, 'registered': registered, 'pending': pending, 'failed': failed}))
            return 1 if failed else 0
        if args.command == 'server-remove':
            removed, failed = remove_server(args.name)
            print(json.dumps({'server': args.name, 'removed': removed, 'failed': failed}))
            return 1 if failed else 0
        if args.command == 'apply':
            privacy()
            result = apply(args.approval)
        elif args.command == 'privacy':
            privacy()
            result = {'privacy': 'done'}
        else:
            result = reconcile()
    except (Failure, OSError, ValueError) as error:
        sys.stderr.write('error: %s\n' % error)
        return 1
    print(json.dumps(result))
    if result.get('failed'):
        # The agents are installed, so a stale approval would stay in force: fail the setup.
        sys.stderr.write('error: could not update the configuration of %s\n' % ', '.join(result['failed']))
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
