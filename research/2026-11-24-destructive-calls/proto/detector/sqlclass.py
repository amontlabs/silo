"""A small, conservative SQL statement classifier for SQL sent over HTTP.

classify(sql, dialect) returns a dict:
  verdict: "destructive" | "harmless" | "unknown"
  statements: one entry per statement, each {kind, verdict, objects, reason}

Destructive: DROP (any object), TRUNCATE, DELETE or UPDATE without WHERE (or with a WHERE that
holds for every row as written, such as 1=1 or id=5 OR true), ALTER ... DROP ..., anywhere in the
statement tree (so a DELETE inside a CTE counts).
Unknown: anything the parser cannot read, any statement it can only keep as raw text, code that runs
later or elsewhere (DO, CALL, CREATE FUNCTION / PROCEDURE / TRIGGER / RULE, EXPLAIN ANALYZE of
something unknown), and calls to functions that run SQL from a string (dblink_exec, ...).

The Postgres wire proxy designed in another thread needs the same classifier. Both should load this
one (or its port) so a statement gets the same answer whichever way it reaches the database.
"""
import logging
import re

import sqlglot
from sqlglot import exp

logging.getLogger("sqlglot").setLevel(logging.ERROR)

try:  # simplify turns "id = 5 OR 1 = 1" into TRUE
    from sqlglot.optimizer.simplify import simplify as _simplify
except Exception:  # pragma: no cover
    _simplify = None

# Functions that run SQL given as a string, or remove data, from inside a SELECT.
RUNS_SQL_FUNCTIONS = {
    "dblink_exec", "dblink", "dblink_send_query", "query_to_xml", "query_to_xml_and_xmlschema",
    "cursor_to_xml", "lo_unlink", "pg_file_unlink", "pg_drop_replication_slot",
    "pg_catalog.lo_unlink", "pg_terminate_backend",
}
CODE_KINDS = {"FUNCTION", "PROCEDURE", "TRIGGER", "RULE", "EVENT TRIGGER", "AGGREGATE"}
HARMLESS_COMMANDS = {"VACUUM", "ANALYZE", "REINDEX", "SHOW", "RESET", "DISCARD", "LISTEN",
                     "UNLISTEN", "CHECKPOINT", "DEALLOCATE", "PREPARE TRANSACTION"}


def _objects(node):
    out = []
    for t in node.find_all(exp.Table):
        name = ".".join(p for p in (t.args.get("db") and t.db, t.name) if p)
        if name and name not in out:
            out.append(name)
    return out


def _where_holds_for_all(where):
    """True when the WHERE has no column in it (constant) or simplifies to TRUE."""
    if where is None:
        return True
    cond = where.this
    if not list(cond.find_all(exp.Column)):
        return True
    if _simplify is not None:
        try:
            s = _simplify(cond.copy())
            if isinstance(s, exp.Boolean) and s.this:
                return True
        except Exception:
            pass
    return False


def _classify_tree(stmt):
    """Return (verdict, kind, reason) for one parsed statement."""
    if isinstance(stmt, exp.Command):
        head = (stmt.this or "").upper()
        e = stmt.expression
        rest = (e.name if isinstance(e, exp.Literal) else str(e)) if e is not None else ""
        if head == "EXPLAIN":
            if re.match(r"\s*(\(.*ANALY[SZ]E|ANALY[SZ]E)", rest, re.I | re.S):
                inner = re.sub(r"^\s*(\([^)]*\)|ANALY[SZ]E(\s+VERBOSE)?)\s*", "", rest, flags=re.I)
                sub = classify(inner)
                return sub["verdict"], "explain analyze", "EXPLAIN ANALYZE runs the statement"
            return "harmless", "explain", ""
        if head in HARMLESS_COMMANDS:
            return "harmless", head.lower(), ""
        return "unknown", head.lower() or "command", "the classifier cannot read %s statements" % (head or "this")
    if isinstance(stmt, exp.Create):
        kind = str(stmt.args.get("kind") or "").upper()
        if kind in CODE_KINDS:
            return "unknown", "create " + kind.lower(), "creates code that runs later; Silo cannot tell what it does"
    for node in stmt.walk():
        if isinstance(node, exp.Drop):
            kind = str(node.args.get("kind") or "").upper() or "OBJECT"
            return "destructive", "drop " + kind.lower(), ""
        if isinstance(node, exp.TruncateTable):
            return "destructive", "truncate", ""
        if isinstance(node, exp.Delete) and _where_holds_for_all(node.args.get("where")):
            return "destructive", "delete all rows", ""
        if isinstance(node, exp.Update) and _where_holds_for_all(node.args.get("where")):
            return "destructive", "update all rows", ""
        if isinstance(node, (exp.Anonymous, exp.Func)):
            name = (node.name if isinstance(node, exp.Anonymous) else node.sql_name()).lower()
            if name in RUNS_SQL_FUNCTIONS:
                return "unknown", "function " + name, "calls %s, which runs SQL Silo cannot see" % name
    if isinstance(stmt, (exp.Select, exp.Union, exp.Show, exp.Describe, exp.Pragma, exp.Set,
                         exp.Transaction, exp.Commit, exp.Rollback)):
        return "harmless", type(stmt).__name__.lower(), ""
    return "harmless", type(stmt).__name__.lower(), ""


def classify(sql, dialect="postgres"):
    if not isinstance(sql, str):
        return {"verdict": "unknown", "statements": [], "reason": "SQL is not text"}
    try:
        stmts = sqlglot.parse(sql, read=dialect)
    except Exception as e:
        return {"verdict": "unknown", "statements": [],
                "reason": "the SQL does not parse (%s)" % type(e).__name__}
    out = []
    for s in stmts:
        if s is None:
            continue
        verdict, kind, reason = _classify_tree(s)
        out.append({"kind": kind, "verdict": verdict, "objects": _objects(s), "reason": reason})
    if any(s["verdict"] == "destructive" for s in out):
        v = "destructive"
    elif any(s["verdict"] == "unknown" for s in out):
        v = "unknown"
    else:
        v = "harmless"
    return {"verdict": v, "statements": out}
