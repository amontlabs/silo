-- What a read credential can still change. Run as the read role: psql "$READ_URL" -f read_role_check.sql
-- 1. Functions that run with their owner's rights and that this role may call.
SELECT 'security definer' AS finding, p.oid::regprocedure::text AS what, r.rolname AS runs_as
FROM pg_proc p JOIN pg_roles r ON r.oid = p.proowner
WHERE p.prosecdef AND has_function_privilege(current_user, p.oid, 'EXECUTE')
  AND p.pronamespace NOT IN ('pg_catalog'::regnamespace, 'information_schema'::regnamespace)
UNION ALL
-- 2. Tables this role may write to.
SELECT 'table write', c.oid::regclass::text, string_agg(priv, ',')
FROM pg_class c CROSS JOIN unnest(ARRAY['INSERT','UPDATE','DELETE','TRUNCATE']) priv
WHERE c.relkind IN ('r','p') AND c.relnamespace NOT IN ('pg_catalog'::regnamespace, 'information_schema'::regnamespace)
  AND has_table_privilege(current_user, c.oid, priv)
GROUP BY c.oid
UNION ALL
-- 3. Schemas it may create objects in, and the database-level TEMP right.
SELECT 'create in schema', n.nspname, '' FROM pg_namespace n
WHERE has_schema_privilege(current_user, n.oid, 'CREATE') AND n.nspname NOT LIKE 'pg_temp%' AND n.nspname NOT LIKE 'pg_toast%'
UNION ALL
SELECT 'temp objects', current_database(), '' WHERE has_database_privilege(current_database(), 'TEMP')
UNION ALL
-- 4. Role attributes and memberships that lift limits.
SELECT 'role attribute', a, '' FROM pg_roles, unnest(ARRAY[
  CASE WHEN rolsuper THEN 'SUPERUSER' END, CASE WHEN rolcreaterole THEN 'CREATEROLE' END,
  CASE WHEN rolcreatedb THEN 'CREATEDB' END, CASE WHEN rolbypassrls THEN 'BYPASSRLS' END]) a
WHERE rolname = current_user AND a IS NOT NULL
UNION ALL
SELECT 'member of', g.rolname, '' FROM pg_auth_members m JOIN pg_roles g ON g.oid = m.roleid
WHERE m.member = (SELECT oid FROM pg_roles WHERE rolname = current_user);
