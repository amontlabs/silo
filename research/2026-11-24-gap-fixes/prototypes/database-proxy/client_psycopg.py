"""psycopg 3 through the boundary: server-side parameters (extended protocol) and pipeline mode,
where a held statement sits between two others that were sent with it before any answer came.

Run: python client_psycopg.py "<DATABASE_URL with the placeholder>"
"""
import sys
import time

import psycopg

url = sys.argv[1]
with psycopg.connect(url, autocommit=False) as conn:
    print("server-side params:", conn.execute(
        "select current_user, count(*) from orders where total > %s", (100,)).fetchone())
    conn.rollback()

    # Pipeline: INSERT, DROP TABLE (held), INSERT, all sent before the first reply.
    conn.execute("create table if not exists audit(note text)")
    conn.commit()
    t0 = time.monotonic()
    try:
        with conn.pipeline():
            conn.execute("insert into audit values (%s)", ("before",))
            conn.execute("drop table orders")
            conn.execute("insert into audit values (%s)", ("after",))
        conn.commit()
        print("pipeline committed?!")
    except psycopg.Error as e:
        print(f"pipeline refused after {time.monotonic() - t0:.1f} s: "
              f"sqlstate={e.sqlstate} {e.diag.message_primary} ({e.diag.message_detail})")
        print("transaction status:", conn.info.transaction_status.name)
        conn.rollback()
    print("audit rows (0 expected: the whole transaction failed as a real error would):",
          conn.execute("select count(*) from audit").fetchone()[0])
    print("orders:", conn.execute("select count(*) from orders").fetchone()[0])
    conn.rollback()
