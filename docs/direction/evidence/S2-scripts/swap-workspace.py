# usage: swap-workspace.py <from-sandbox-dir> <to-sandbox-dir>   (both stopped)
# Replaces the target's workspace disk (its whole owned-volumes/<id> folder and the layer list in
# runtime/owned-disks/<id>/disk.json) with an APFS clone of the source's; keeps the target's volume_id.
import json,sys,os,subprocess,glob,shutil
src,dst=sys.argv[1:3]
vol=os.path.basename(glob.glob(src+'/owned-volumes/workspace_*')[0])
sj=f'{src}/runtime/owned-disks/{vol}/disk.json'; dj=f'{dst}/runtime/owned-disks/{vol}/disk.json'
s=json.load(open(sj)); d=json.load(open(dj))
staged=f'{dst}/owned-volumes/.swap-{vol}'
subprocess.run(['cp','-c','-R',f'{src}/owned-volumes/{vol}',staged],check=True)
old=f'{dst}/owned-volumes/{vol}'
os.rename(old,f'{dst}/.workspace-before-swap')       # kept outside owned-volumes for undo
os.rename(staged,old)
s['volume_id']=d['volume_id']
tmp=dj+'.new'; json.dump(s,open(tmp,'w'),separators=(',',':')); os.replace(tmp,dj)
print('swapped',vol,[l['path'] for l in s['layers']])
