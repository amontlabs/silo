"""Sort a Postgres statement by what its text says, using the Postgres parser (libpg_query, via pglast).

What this module returns is exact about the text: it is what the parser read. It is not a
statement about what running the text will do: a SELECT on a view can call a function that
deletes rows. See the doc, section "What a parse knows".

Each statement gets one kind:
  read         SELECT, SHOW, EXPLAIN (without ANALYZE), and calls only to builtin functions
               that do not write or reach outside the database
  write        INSERT, UPDATE or DELETE with a WHERE, CREATE, most ALTER, GRANT...
  destructive  DROP, TRUNCATE, DELETE or UPDATE without WHERE, ALTER TABLE ... DROP ...,
               DROP DATABASE, DROP OWNED, a prisma/drizzle style reset
  opaque       text whose effect the boundary does not read: DO blocks, CALL, calls to functions
               defined in the database, builtins that run a query given as a string, the
               fast-path function call message
  admin        builtins that act on the server or reach its files: set_config, pg_read_file,
               lo_*, pg_terminate_backend, COPY ... PROGRAM, COPY to or from a server file
  session      statements that change how later statements run: SET of a guarded setting,
               SET ROLE, SET SESSION AUTHORIZATION, BEGIN READ WRITE, RESET ALL, DISCARD
  unparsed     the parser refused it
"""

from __future__ import annotations

import functools
from dataclasses import dataclass, field

from pglast import ast, parse_sql
from pglast.enums import AlterTableType, ObjectType
from pglast.visitors import Visitor

ORDER = ["read", "write", "session", "opaque", "admin", "destructive", "unparsed"]

# Volatile builtins (PG16 has 259) that neither write data nor reach outside the database.
# Everything volatile in pg_catalog that is not here is "admin" or "opaque".
HARMLESS_VOLATILE = {
    "random", "random_normal", "setseed", "gen_random_uuid", "clock_timestamp", "timeofday",
    "nextval", "currval", "lastval",
    "pg_sleep", "pg_sleep_for", "pg_sleep_until",
    "pg_advisory_lock", "pg_advisory_lock_shared", "pg_advisory_unlock",
    "pg_advisory_unlock_all", "pg_advisory_unlock_shared", "pg_advisory_xact_lock",
    "pg_advisory_xact_lock_shared", "pg_try_advisory_lock", "pg_try_advisory_lock_shared",
    "pg_try_advisory_xact_lock", "pg_try_advisory_xact_lock_shared",
    "pg_is_in_recovery", "pg_database_size", "pg_indexes_size", "pg_relation_size",
    "pg_table_size", "pg_tablespace_size", "pg_total_relation_size", "pg_lock_status",
    "pg_blocking_pids", "pg_safe_snapshot_blocking_pids", "pg_current_wal_lsn",
    "pg_current_wal_flush_lsn", "pg_current_wal_insert_lsn", "pg_jit_available",
    "pg_notification_queue_usage", "current_query", "pg_partition_tree",
    "pg_partition_ancestors", "pg_sequence_last_value", "txid_status", "pg_xact_status",
    "array_sample", "array_shuffle", "pg_stat_clear_snapshot", "pg_stat_have_stats",
    "pg_xact_commit_timestamp", "pg_last_committed_xact",
}
# Builtins that take SQL as a string and run it: a SELECT that hides any statement.
# query_to_xml, cursor_to_xml, ts_stat also run a string, but measured: Postgres runs it read-only
# ("DELETE is not allowed in a non-volatile function"), so they stay "read". dblink opens a new
# session that is not.
RUNS_A_STRING = {
                 "dblink", "dblink_exec", "dblink_open", "dblink_send_query"}
# Settings whose change would undo what the boundary set up for the session.
GUARDED_SETTINGS = {"default_transaction_read_only", "transaction_read_only", "role",
                    "session_authorization", "search_path", "session_replication_role",
                    "statement_timeout", "lock_timeout", "idle_in_transaction_session_timeout"}

DESTRUCTIVE_DROP_ATS = {AlterTableType.AT_DropColumn, AlterTableType.AT_DropConstraint,
                        AlterTableType.AT_DropNotNull, AlterTableType.AT_DetachPartition,
                        AlterTableType.AT_DropIdentity, AlterTableType.AT_DropExpression}


@dataclass
class Catalog:
    """Function names, read from the server's pg_proc when the session starts."""
    builtin_volatility: dict[str, str] = field(default_factory=dict)  # name -> i/s/v
    user_defined: set[str] = field(default_factory=set)  # names defined outside pg_catalog

    SQL = ("select proname, provolatile, pronamespace = 'pg_catalog'::regnamespace "
           "from pg_proc where prokind in ('f','p','a','w')")

    def load(self, rows):
        for name, vol, builtin in rows:
            if builtin in (True, "t"):
                if self.builtin_volatility.get(name) != "v":
                    self.builtin_volatility[name] = vol
            else:
                self.user_defined.add(name)


@dataclass
class Finding:
    kind: str
    what: str  # short, true sentence for the record and the refusal

    def __str__(self):
        return f"{self.kind}: {self.what}"


@dataclass
class Verdict:
    kind: str
    findings: list[Finding]
    statements: int

    def reason(self):
        worst = [f for f in self.findings if f.kind == self.kind]
        return "; ".join(f.what for f in worst[:3]) or self.kind


def _sets_read_write(defelems):
    for d in defelems or ():
        if getattr(d, "defname", "") == "transaction_read_only":
            if getattr(getattr(d.arg, "val", None), "ival", 1) == 0:
                return True
    return False


def _worst(kinds):
    return max(kinds, key=ORDER.index) if kinds else "read"


class _Calls(Visitor):
    def __init__(self):
        self.calls = []

    def visit_FuncCall(self, ancestors, node):
        self.calls.append(tuple(n.sval for n in node.funcname))


def _function_finding(name: tuple, cat: Catalog) -> Finding | None:
    schema, fn = (name[0], name[-1]) if len(name) > 1 else (None, name[0])
    if schema not in (None, "pg_catalog"):
        return Finding("opaque", f"calls {'.'.join(name)}(), a function defined in the database")
    if fn in cat.user_defined:
        # Also catches a function that shadows a builtin through search_path or an overload.
        return Finding("opaque", f"calls {fn}(), a function defined in the database")
    if fn in RUNS_A_STRING:
        return Finding("opaque", f"calls {fn}(), which runs SQL given to it as a string")
    vol = cat.builtin_volatility.get(fn)
    if vol is None:
        return Finding("opaque", f"calls {fn}(), a function the boundary does not know")
    if vol == "v" and fn not in HARMLESS_VOLATILE:
        return Finding("admin", f"calls {fn}(), a builtin that acts on the server")
    return None


def _statement(stmt, cat: Catalog) -> list[Finding]:
    out: list[Finding] = []
    t = type(stmt).__name__
    if isinstance(stmt, ast.DropStmt):
        objtype = ObjectType(stmt.removeType).name.removeprefix("OBJECT_").lower()
        cascade = " CASCADE" if stmt.behavior and int(stmt.behavior) == 1 else ""
        out.append(Finding("destructive", f"DROP {objtype.upper()}{cascade}"))
    elif isinstance(stmt, (ast.DropdbStmt, ast.DropOwnedStmt, ast.DropRoleStmt,
                           ast.DropTableSpaceStmt, ast.DropSubscriptionStmt)):
        out.append(Finding("destructive", t.removesuffix("Stmt")))
    elif isinstance(stmt, ast.TruncateStmt):
        out.append(Finding("destructive", "TRUNCATE"))
    elif isinstance(stmt, ast.DeleteStmt):
        out.append(Finding("destructive", "DELETE without WHERE") if stmt.whereClause is None
                   else Finding("write", "DELETE with WHERE"))
    elif isinstance(stmt, ast.UpdateStmt):
        out.append(Finding("destructive", "UPDATE without WHERE") if stmt.whereClause is None
                   else Finding("write", "UPDATE with WHERE"))
    elif isinstance(stmt, ast.AlterTableStmt):
        for c in stmt.cmds or ():
            if c.subtype in DESTRUCTIVE_DROP_ATS:
                out.append(Finding("destructive",
                                   f"ALTER TABLE ... {AlterTableType(c.subtype).name[3:]}"))
        if not out:
            out.append(Finding("write", "ALTER TABLE"))
    elif isinstance(stmt, (ast.DoStmt, ast.CallStmt)):
        out.append(Finding("opaque", "a DO block, whose body the boundary does not read"
                           if t == "DoStmt" else "CALL of a procedure"))
    elif isinstance(stmt, ast.CopyStmt):
        if stmt.is_program:
            out.append(Finding("admin", "COPY ... PROGRAM runs a command on the database server"))
        elif stmt.filename:
            out.append(Finding("admin", "COPY to or from a file on the database server"))
        else:
            out.append(Finding("write" if stmt.is_from else "read",
                               "COPY FROM STDIN" if stmt.is_from else "COPY TO STDOUT"))
    elif isinstance(stmt, ast.VariableSetStmt):
        name = (stmt.name or "").lower()
        if int(stmt.kind) == 5:  # RESET ALL
            out.append(Finding("session", "RESET ALL"))
        elif name in ("transaction", "session characteristics"):
            if _sets_read_write(stmt.args):
                out.append(Finding("session", f"SET {name.upper()} ... READ WRITE"))
        elif name in GUARDED_SETTINGS:
            out.append(Finding("session", f"changes {name}"))
    elif isinstance(stmt, ast.TransactionStmt):
        if _sets_read_write(stmt.options):
            out.append(Finding("session", "starts a READ WRITE transaction"))
    elif isinstance(stmt, ast.DiscardStmt):
        out.append(Finding("session", "DISCARD"))
    elif isinstance(stmt, (ast.CreateFunctionStmt,)):
        out.append(Finding("write", "CREATE FUNCTION"))
    elif isinstance(stmt, ast.ExplainStmt):
        analyze = any(getattr(o, "defname", "") == "analyze" for o in stmt.options or ())
        inner = _statement(stmt.query, cat)
        out.extend(inner if analyze else [Finding("read", "EXPLAIN")])
        if analyze:
            return out + _calls(stmt.query, cat)
        return out
    elif isinstance(stmt, (ast.SelectStmt, ast.VariableShowStmt)):
        if isinstance(stmt, ast.SelectStmt) and stmt.intoClause is not None:
            out.append(Finding("write", "SELECT INTO"))
        # A data-modifying CTE: WITH d AS (DELETE ...) SELECT ...
        wc = getattr(stmt, "withClause", None)
        for cte in (wc.ctes if wc else ()) or ():
            out.extend(_statement(cte.ctequery, cat))
        if not out:
            out.append(Finding("read", "SELECT"))
    elif isinstance(stmt, ast.InsertStmt):
        out.append(Finding("write", "INSERT"))
    elif isinstance(stmt, ast.ListenStmt):
        out.append(Finding("read", "LISTEN"))
    elif isinstance(stmt, ast.NotifyStmt):
        out.append(Finding("write", "NOTIFY"))
    elif isinstance(stmt, ast.PrepareStmt):
        out.extend(_statement(stmt.query, cat))
    elif isinstance(stmt, ast.ExecuteStmt):
        out.append(Finding("opaque", f"EXECUTE of the prepared statement {stmt.name}"))
    else:
        out.append(Finding("write", t.removesuffix("Stmt")))
    # Data-modifying CTEs on INSERT/UPDATE/DELETE too.
    wc = getattr(stmt, "withClause", None)
    if not isinstance(stmt, ast.SelectStmt) and wc:
        for cte in wc.ctes or ():
            out.extend(_statement(cte.ctequery, cat))
    return out + _calls(stmt, cat)


def _calls(stmt, cat):
    v = _Calls()
    v(stmt)
    return [f for f in (_function_finding(c, cat) for c in v.calls) if f]


@functools.lru_cache(maxsize=4096)
def _parse(sql: str):
    return parse_sql(sql)


def classify(sql: str, cat: Catalog, prepared: dict | None = None) -> Verdict:
    try:
        stmts = _parse(sql)
    except Exception as e:  # pglast.parser.ParseError
        return Verdict("unparsed", [Finding("unparsed", f"the parser refused it: {e}")], 0)
    findings = []
    for raw in stmts:
        if isinstance(raw.stmt, ast.ExecuteStmt) and prepared is not None \
                and raw.stmt.name in prepared:
            findings.extend(prepared[raw.stmt.name].findings)
            continue
        findings.extend(_statement(raw.stmt, cat))
    return Verdict(_worst([f.kind for f in findings]), findings, len(stmts))


def created_function_names(sql: str) -> set[str]:
    """Names of functions a statement creates, so the catalog stays current in the session."""
    try:
        stmts = _parse(sql)
    except Exception:
        return set()
    names = set()
    for raw in stmts:
        s = raw.stmt
        if isinstance(s, ast.CreateFunctionStmt):
            names.add(s.funcname[-1].sval)
    return names


if __name__ == "__main__":
    import sys
    cat = Catalog()
    cat.builtin_volatility.update({"lower": "i", "now": "s", "count": "i", "set_config": "v",
                                   "query_to_xml": "v", "pg_read_file": "v"})
    cat.user_defined.add("purge_old")
    for line in sys.stdin:
        if line.strip():
            v = classify(line.strip(), cat)
            print(f"{v.kind:12} {line.strip()[:70]:70} | {v.reason()}")
