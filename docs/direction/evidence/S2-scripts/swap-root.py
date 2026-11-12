# usage: swap-root.py <name> <fresh-config.json> <out.json>  — copy the sandbox's config and put the image fields of a fresh computer in
import json,sys,sqlite3,os
name,fresh,out=sys.argv[1:4]
db=sqlite3.connect(os.path.expanduser('~/.s2/home/db/msb.db'))
cur=json.loads(db.execute('select config from sandbox where name=?',(name,)).fetchone()[0])
b=json.load(open(fresh))
cur['image']=b['image']; cur['manifest_digest']=b['manifest_digest']
for k,v in b['labels'].items():
    if k.startswith('org.opencontainers.'): cur['labels'][k]=v
json.dump(cur,open(out,'w'),separators=(',',':'))
