import json,sys
for i,l in enumerate(open(sys.argv[1])):
    r=json.loads(l); b=r['body']
    if 'messages' not in b: print(i, r['path']); continue
    tools=[t['name'] for t in b.get('tools',[])]
    print(f"--- req {i} model={b.get('model')} tools={len(tools)} msgs={len(b['messages'])} meta={b.get('metadata')}")
    for m in b['messages'][-6:]:
        c=m['content']
        if isinstance(c,str): print('  ',m['role'],'TEXT',c[:100].replace('\n',' ')); continue
        for x in c:
            t=x['type']
            if t=='text': print('  ',m['role'],'text',x['text'][:100].replace('\n',' '))
            elif t=='tool_use': print('  ',m['role'],'tool_use',x['id'],x['name'],json.dumps(x['input'])[:120])
            elif t=='tool_result': print('  ',m['role'],'tool_result',x['tool_use_id'],'err=',x.get('is_error'),json.dumps(x.get('content'))[:300])
            else: print('  ',m['role'],t)
