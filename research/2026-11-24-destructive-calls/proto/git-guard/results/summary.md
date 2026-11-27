## Classification (observe mode, all reps)

| scenario | ref | expected | decision (c) | pack alone (a) | server alone (b) | server asks (c) | probe POST | pack objects / commits / deltified commits / unresolved |
|---|---|---|---|---|---|---|---|---|
| normal | `refs/heads/main` | ff | ff  | ff | n/a: new not on server | 0 | no | 6/2/0/0 |
| new-branch | `refs/heads/topic` | create | create  | - | - | - | no | no pack |
| delete-branch | `refs/heads/feature` | delete | delete  | - | - | - | no | no pack |
| force-main | `refs/heads/main` | force | force  | unknown | n/a: new not on server | 1 | no | 3/1/0/0 |
| rewind-main | `refs/heads/main` | force | force  | unknown | force | 1 | no | 0/0/0/0 |
| force-rebase | `refs/heads/feature` | force | force  | unknown | n/a: new not on server | 1 | no | 3/1/0/1 |
| merge-ff | `refs/heads/main` | ff | ff  | ff | n/a: new not on server | 0 | no | 5/2/0/2 |
| multi | `refs/heads/main` | force | force  | unknown | n/a: new not on server | 1 | no | 6/2/0/1 |
| multi | `refs/heads/feature` | ff | ff  | ff | n/a: new not on server | 0 | no | 6/2/0/1 |
| atomic | `refs/heads/main` | force | force  | unknown | n/a: new not on server | 1 | no | 6/2/0/1 |
| atomic | `refs/heads/feature` | ff | ff  | ff | n/a: new not on server | 0 | no | 6/2/0/1 |
| mirror | `refs/heads/main` | ff | ff  | ff | n/a: new not on server | 0 | no | 3/1/0/1 |
| mirror | `refs/heads/feature` | delete | delete  | - | - | - | no | 3/1/0/1 |
| mirror | `refs/heads/old-feature` | delete | delete  | - | - | - | no | 3/1/0/1 |
| mirror | `refs/tags/v1` | delete | delete  | - | - | - | no | 3/1/0/1 |
| mirror-hide | `refs/heads/main` | ff | ff  | ff | - | 0 | no | 3/1/0/1 |
| mirror-hide | `refs/heads/feature` | delete | delete  | - | - | - | no | 3/1/0/1 |
| mirror-hide | `refs/heads/old-feature` | delete | delete  | - | - | - | no | 3/1/0/1 |
| mirror-hide | `refs/tags/v1` | delete | delete  | - | - | - | no | 3/1/0/1 |
| tag-delete | `refs/tags/v1` | delete | delete  | - | - | - | no | no pack |
| push-options | `refs/heads/main` | ff | ff  | ff | n/a: new not on server | 0 | no | 3/1/0/1 |
| large | `refs/heads/main` | ff | ff  | ff | n/a: new not on server | 0 | yes | 3/1/0/0 |
| large-force | `refs/heads/main` | force | force  | unknown | n/a: new not on server | 1 | yes | 3/1/0/0 |
| signed-force | `refs/heads/main` | force | force  | unknown | n/a: new not on server | 1 | no | 3/1/0/0 |
| touch-silo | `refs/silo/backup/heads/main/forged` | create | create  | - | - | - | no | no pack |

## Time (median over reps, ms)

| scenario | wall pass | wall observe | wall backup | guard work in backup (parse+classify+backup) | classify | backup refs | pack MB |
|---|---|---|---|---|---|---|---|
| normal | 138.1 | 146.1 | 141.7 | 0.2 | 0.1 | None | 0.0 |
| new-branch | 145.7 | 141.4 | 146.4 | 0.1 | 0.0 | None | 0.0 |
| delete-branch | 128.6 | 130.8 | 135.0 | 2.2 | 0.0 | 2.1 | 0.0 |
| force-main | 139.1 | 139.6 | 139.8 | 4.1 | 1.9 | 2.1 | 0.0 |
| rewind-main | 143.3 | 141.4 | 140.0 | 4.7 | 2.4 | 2.2 | 0.0 |
| force-rebase | 143.1 | 148.1 | 146.6 | 4.8 | 2.1 | 2.3 | 0.0 |
| merge-ff | 141.9 | 138.0 | 140.8 | 0.2 | 0.1 | None | 0.0 |
| multi | 147.8 | 151.2 | 145.4 | 4.5 | 1.9 | 2.2 | 0.0 |
| atomic | 138.5 | 150.3 | 147.8 | 4.5 | 2.0 | 2.4 | 0.0 |
| mirror | 142.8 | 120.6 | 122.2 | None | 0.1 | None | 0.0 |
| mirror-hide | None | None | 150.0 | 6.5 | 0.1 | 6.3 | None |
| tag-delete | 127.8 | 127.9 | 134.3 | 2.2 | 0.0 | 2.1 | 0.0 |
| push-options | 139.4 | 136.0 | 147.3 | 0.1 | 0.1 | None | 0.0 |
| large | 489.2 | 401.1 | 402.5 | 64.0 | 5.3 | None | 1.6 |
| large-force | 397.9 | 390.5 | 386.7 | 61.0 | 7.2 | 1.9 | 1.6 |
| signed-force | 145.5 | 147.6 | 152.6 | 4.7 | 2.2 | 2.1 | 0.0 |
| touch-silo | 137.4 | 118.1 | 125.6 | None | 0.0 | None | 0.0 |

## Per-method time inside observe (median ms per force/ff ref)

| scenario | ref | (a) pack | (b) server | (c) hybrid |
|---|---|---|---|---|
| normal | `refs/heads/main` | 0.1 | 1.7 | 0.1 |
| force-main | `refs/heads/main` | 0.1 | 1.7 | 1.9 |
| rewind-main | `refs/heads/main` | 0.0 | 3.7 | 1.8 |
| force-rebase | `refs/heads/feature` | 0.1 | 1.9 | 1.9 |
| merge-ff | `refs/heads/main` | 0.1 | 1.7 | 0.1 |
| multi | `refs/heads/feature` | 0.1 | 2.0 | 0.1 |
| multi | `refs/heads/main` | 0.1 | 1.8 | 1.9 |
| atomic | `refs/heads/feature` | 0.1 | 1.9 | 0.1 |
| atomic | `refs/heads/main` | 0.1 | 1.8 | 1.9 |
| mirror | `refs/heads/main` | 0.1 | 2.0 | 0.1 |
| push-options | `refs/heads/main` | 0.1 | 1.5 | 0.1 |
| large | `refs/heads/main` | 5.8 | 2.2 | 5.8 |
| large-force | `refs/heads/main` | 5.4 | 2.0 | 7.5 |
| signed-force | `refs/heads/main` | 0.1 | 1.9 | 1.8 |

## What the agent saw (refuse, split, hold) and what the server got

### normal / refuse: exit 0, 141.1 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8446/o/normal.git
   17c93a8..20d95e0  main -> main
```
### normal / split: exit 0, 149.5 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8448/o/normal.git
   dc6d0cc..3898ea5  main -> main
```
### normal / hold: exit 0, 148.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/normal.git
   80d544f..7334838  main -> main
```
### normal / backup: exit 0, 146.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/normal.git
   ff92264..ae80f76  main -> main
```
### new-branch / refuse: exit 0, 153.9 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/topic']
```
To https://127.0.0.1:8446/o/new-branch.git
 * [new branch]      topic -> topic
```
### new-branch / split: exit 0, 140.0 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/topic']
```
To https://127.0.0.1:8448/o/new-branch.git
 * [new branch]      topic -> topic
```
### new-branch / hold: exit 0, 135.3 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/topic']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/new-branch.git
 * [new branch]      topic -> topic
```
### new-branch / backup: exit 0, 136.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/topic']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/new-branch.git
 * [new branch]      topic -> topic
```
### delete-branch / refuse: exit 1, 121.3 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/delete-branch.git
 ! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/delete-branch.git'
```
### delete-branch / split: exit 1, 114.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/delete-branch.git
 ! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/delete-branch.git'
```
### delete-branch / hold: exit 0, 2131.5 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/silo/backup/heads/feature/20261124T161716273168']
backups survive gc: {'refs/silo/backup/heads/feature/20261124T161716273168': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/delete-branch.git
 - [deleted]         feature
```
### delete-branch / backup: exit 0, 127.6 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/silo/backup/heads/feature/20261124T161708869118']
backups survive gc: {'refs/silo/backup/heads/feature/20261124T161708869118': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/delete-branch.git
 - [deleted]         feature
```
### force-main / refuse: exit 1, 124.4 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/force-main.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/force-main.git'
```
### force-main / split: exit 1, 134.9 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/force-main.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/force-main.git'
```
### force-main / hold: exit 0, 2156.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T161731765663']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161731765663': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/force-main.git
 + 8c8c2dd...af2cba5 main -> main (forced update)
```
### force-main / backup: exit 0, 144.3 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T161724309081']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161724309081': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/force-main.git
 + 992fe67...85d639f main -> main (forced update)
```
### rewind-main / refuse: exit 1, 124.4 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/rewind-main.git
 ! [remote rejected] HEAD~2 -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/rewind-main.git'
```
### rewind-main / split: exit 1, 121.2 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/rewind-main.git
 ! [remote rejected] HEAD~2 -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/rewind-main.git'
```
### rewind-main / hold: exit 0, 2136.6 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T161747463955']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161747463955': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/rewind-main.git
 + f72fb1e...d05a06b HEAD~2 -> main (forced update)
```
### rewind-main / backup: exit 0, 155.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T161739896096']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161739896096': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/rewind-main.git
 + 2c92411...ba2263b HEAD~2 -> main (forced update)
```
### force-rebase / refuse: exit 1, 125.3 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/feature (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/force-rebase.git
 ! [remote rejected] feature -> feature (Held by Silo: rewrites refs/heads/feature (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/force-rebase.git'
```
### force-rebase / split: exit 1, 140.8 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/feature (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/force-rebase.git
 ! [remote rejected] feature -> feature (Held by Silo: rewrites refs/heads/feature (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/force-rebase.git'
```
### force-rebase / hold: exit 0, 2147.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/silo/backup/heads/feature/20261124T161803409780']
backups survive gc: {'refs/silo/backup/heads/feature/20261124T161803409780': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/force-rebase.git
 + bbcfe92...806fac4 feature -> feature (forced update)
```
### force-rebase / backup: exit 0, 146.6 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/silo/backup/heads/feature/20261124T161755721193']
backups survive gc: {'refs/silo/backup/heads/feature/20261124T161755721193': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/force-rebase.git
 + 1ac91b5...1d276d4 feature -> feature (forced update)
```
### merge-ff / refuse: exit 0, 143.3 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8446/o/merge-ff.git
   738e654..c319c55  main -> main
```
### merge-ff / split: exit 0, 148.2 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8448/o/merge-ff.git
   860b895..839b4d4  main -> main
```
### merge-ff / hold: exit 0, 139.8 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/merge-ff.git
   f4cbb6d..bdb2bf7  main -> main
```
### merge-ff / backup: exit 0, 130.1 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/merge-ff.git
   9acc511..cf069d5  main -> main
```
### multi / refuse: exit 1, 124.9 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/multi.git
 ! [remote rejected] feature -> feature (refused with the held refs of this push)
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/multi.git'
```
### multi / split: exit 1, 138.9 ms, answer ['split'], byte-for-byte None, server refs changed ['refs/heads/feature']
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/multi.git
   c287797..9e4debb  feature -> feature
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/multi.git'
```
### multi / hold: exit 0, 2161.6 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/silo/backup/heads/main/20261124T161833797443']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161833797443': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/multi.git
   58fe7db..e337e93  feature -> feature
 + 3d4495e...3bdca1d main -> main (forced update)
```
### multi / backup: exit 0, 160.0 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/silo/backup/heads/main/20261124T161826059156']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161826059156': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/multi.git
   b8e6294..33604d2  feature -> feature
 + 2fce321...14df0ca main -> main (forced update)
```
### atomic / refuse: exit 1, 125.3 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/atomic.git
 ! [remote rejected] feature -> feature (atomic push: refused with the held refs)
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/atomic.git'
```
### atomic / split: exit 1, 123.9 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/atomic.git
 ! [remote rejected] feature -> feature (atomic push: refused with the held refs)
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/atomic.git'
```
### atomic / hold: exit 0, 2148.3 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/silo/backup/heads/main/20261124T161850177842']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161850177842': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/atomic.git
   d08c9e8..71c5b6f  feature -> feature
 + afe2401...46678e7 main -> main (forced update)
```
### atomic / backup: exit 0, 143.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/silo/backup/heads/main/20261124T161842341527']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161842341527': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/atomic.git
   0446c7c..2ecdff4  feature -> feature
 + f77474b...d5463b9 main -> main (forced update)
```
### mirror / refuse: exit 1, 120.6 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/mirror.git
 ! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.)
 ! [remote rejected] main -> main (refused with the held refs of this push)
 ! [remote rejected] old-feature (Held by Silo: deletes refs/heads/old-feature. Ask the user in the Silo app.)
 ! [remote rejected] refs/silo/backup/heads/main/earlier (refs/silo/ is kept by Silo and cannot be changed from the computer)
 ! [remote rejected] v1 (Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.)
 ! [remote rejected] origin/HEAD -> origin/HEAD (refused with the held refs of this push)
 ! [remote rejected] origin/feature -> origin/feature (refused with the held refs of this push)
 ! [remote rejected] origin/main -> origin/main (refused with the held refs of this push)
 ! [remote rejected] origin/old-feature -> origin/old-feature (refused with the held refs of this push)
error: failed to push some refs to 'https://127.0.0.1:8446/o/mirror.git'
```
### mirror / split: exit 1, 125.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8448/o/mirror.git
 ! [remote rejected] feature (refused with the held refs of this push)
 ! [remote rejected] main -> main (refused with the held refs of this push)
 ! [remote rejected] old-feature (refused with the held refs of this push)
 ! [remote rejected] refs/silo/backup/heads/main/earlier (refs/silo/ is kept by Silo and cannot be changed from the computer)
 ! [remote rejected] v1 (refused with the held refs of this push)
 ! [remote rejected] origin/HEAD -> origin/HEAD (refused with the held refs of this push)
 ! [remote rejected] origin/feature -> origin/feature (refused with the held refs of this push)
 ! [remote rejected] origin/main -> origin/main (refused with the held refs of this push)
 ! [remote rejected] origin/old-feature -> origin/old-feature (refused with the held refs of this push)
error: failed to push some refs to 'https://127.0.0.1:8448/o/mirror.git'
```
### mirror / hold: exit 1, 118.7 ms, answer ['refused'], byte-for-byte None, server refs changed []
backups survive gc: {}; mirror clone sees refs/silo: True; ls-remote shows: True
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8447/o/mirror.git
 ! [remote rejected] feature (refused with the held refs of this push)
 ! [remote rejected] main -> main (refused with the held refs of this push)
 ! [remote rejected] old-feature (refused with the held refs of this push)
 ! [remote rejected] refs/silo/backup/heads/main/earlier (refs/silo/ is kept by Silo and cannot be changed from the computer)
 ! [remote rejected] v1 (refused with the held refs of this push)
 ! [remote rejected] origin/HEAD -> origin/HEAD (refused with the held refs of this push)
 ! [remote rejected] origin/feature -> origin/feature (refused with the held refs of this push)
 ! [remote rejected] origin/main -> origin/main (refused with the held refs of this push)
 ! [remote rejected] origin/old-feature -> origin/old-feature (refused with the held refs of this push)
error: failed to push some refs to 'https://127.0.0.1:8447/o/mirror.git'
```
### mirror / backup: exit 1, 127.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
backups survive gc: {}; mirror clone sees refs/silo: True; ls-remote shows: True
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8445/o/mirror.git
 ! [remote rejected] feature (refused with the held refs of this push)
 ! [remote rejected] main -> main (refused with the held refs of this push)
 ! [remote rejected] old-feature (refused with the held refs of this push)
 ! [remote rejected] refs/silo/backup/heads/main/earlier (refs/silo/ is kept by Silo and cannot be changed from the computer)
 ! [remote rejected] v1 (refused with the held refs of this push)
 ! [remote rejected] origin/HEAD -> origin/HEAD (refused with the held refs of this push)
 ! [remote rejected] origin/feature -> origin/feature (refused with the held refs of this push)
 ! [remote rejected] origin/main -> origin/main (refused with the held refs of this push)
 ! [remote rejected] origin/old-feature -> origin/old-feature (refused with the held refs of this push)
error: failed to push some refs to 'https://127.0.0.1:8445/o/mirror.git'
```
### mirror-hide / split: exit 1, 136.3 ms, answer ['split'], byte-for-byte None, server refs changed ['refs/heads/main', 'refs/remotes/origin/HEAD', 'refs/remotes/origin/feature', 'refs/remotes/origin/main', 'refs/remotes/origin/old-feature']
```
remote: Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/mirror-hide.git
   f9ac43c..500f47c  main -> main
 * [new reference]   origin/HEAD -> origin/HEAD
 * [new reference]   origin/feature -> origin/feature
 * [new reference]   origin/main -> origin/main
 * [new reference]   origin/old-feature -> origin/old-feature
 ! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.)
 ! [remote rejected] old-feature (Held by Silo: deletes refs/heads/old-feature. Ask the user in the Silo app.)
 ! [remote rejected] v1 (Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/mirror-hide.git'
```
### mirror-hide / backup: exit 0, 150.0 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/heads/old-feature', 'refs/remotes/origin/HEAD', 'refs/remotes/origin/feature', 'refs/remotes/origin/main', 'refs/remotes/origin/old-feature', 'refs/silo/backup/heads/feature/20261124T161905379520', 'refs/silo/backup/heads/old-feature/20261124T161905379520', 'refs/silo/backup/tags/v1/20261124T161905379520', 'refs/tags/v1']
backups survive gc: {'refs/silo/backup/tags/v1/20261124T161905379520': True, 'refs/silo/backup/heads/old-feature/20261124T161905379520': True, 'refs/silo/backup/heads/feature/20261124T161905379520': True}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/mirror-hide.git
 - [deleted]         feature
   c48f141..34e7fe8  main -> main
 - [deleted]         old-feature
 - [deleted]         v1
 * [new reference]   origin/HEAD -> origin/HEAD
 * [new reference]   origin/feature -> origin/feature
 * [new reference]   origin/main -> origin/main
 * [new reference]   origin/old-feature -> origin/old-feature
```
### tag-delete / refuse: exit 1, 113.9 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/tag-delete.git
 ! [remote rejected] v1 (Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/tag-delete.git'
```
### tag-delete / split: exit 1, 112.8 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/tag-delete.git
 ! [remote rejected] v1 (Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/tag-delete.git'
```
### tag-delete / hold: exit 0, 2128.5 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/silo/backup/tags/v1/20261124T161920740895', 'refs/tags/v1']
backups survive gc: {'refs/silo/backup/tags/v1/20261124T161920740895': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/tag-delete.git
 - [deleted]         v1
```
### tag-delete / backup: exit 0, 134.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/silo/backup/tags/v1/20261124T161913144622', 'refs/tags/v1']
backups survive gc: {'refs/silo/backup/tags/v1/20261124T161913144622': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/tag-delete.git
 - [deleted]         v1
```
### push-options / refuse: exit 0, 155.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8446/o/push-options.git
   d7eace6..d505467  main -> main
```
### push-options / split: exit 0, 149.1 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8448/o/push-options.git
   9bb6767..408c974  main -> main
```
### push-options / hold: exit 0, 156.0 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/push-options.git
   5b90e4f..762522e  main -> main
```
### push-options / backup: exit 0, 138.0 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/push-options.git
   29f62d3..3eaa6b8  main -> main
```
### large / refuse: exit 0, 608.3 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8446/o/large.git
   3b28e9f..7be8ec0  main -> main
```
### large / split: exit 0, 547.5 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8448/o/large.git
   0d21626..3caa6ec  main -> main
```
### large / hold: exit 0, 571.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/large.git
   3962171..0eb2fb1  main -> main
```
### large / backup: exit 0, 502.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/large.git
   4e5e201..f1155fe  main -> main
```
### large-force / refuse: exit 1, 268.8 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/large-force.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/large-force.git'
```
### large-force / split: exit 1, 281.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/large-force.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/large-force.git'
```
### large-force / hold: exit 0, 2395.3 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T162017874314']
backups survive gc: {'refs/silo/backup/heads/main/20261124T162017874314': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/large-force.git
 + ce783c7...7d81b25 main -> main (forced update)
```
### large-force / backup: exit 0, 386.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T162007892076']
backups survive gc: {'refs/silo/backup/heads/main/20261124T162007892076': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/large-force.git
 + d21697d...0153cef main -> main (forced update)
```
### signed-force / refuse: exit 1, 129.5 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/signed-force.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/signed-force.git'
```
### signed-force / split: exit 1, 130.1 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/signed-force.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/signed-force.git'
```
### signed-force / hold: exit 0, 2153.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T162034420022']
backups survive gc: {'refs/silo/backup/heads/main/20261124T162034420022': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/signed-force.git
 + 5efad1b...cf6d087 main -> main (forced update)
```
### signed-force / backup: exit 0, 152.6 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T162026616521']
backups survive gc: {'refs/silo/backup/heads/main/20261124T162026616521': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/signed-force.git
 + 40b34b1...b761efc main -> main (forced update)
```
### touch-silo / refuse: exit 1, 132.7 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8446/o/touch-silo.git
 ! [remote rejected] main -> refs/silo/backup/heads/main/forged (refs/silo/ is kept by Silo and cannot be changed from the computer)
error: failed to push some refs to 'https://127.0.0.1:8446/o/touch-silo.git'
```
### touch-silo / split: exit 1, 128.5 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8448/o/touch-silo.git
 ! [remote rejected] main -> refs/silo/backup/heads/main/forged (refs/silo/ is kept by Silo and cannot be changed from the computer)
error: failed to push some refs to 'https://127.0.0.1:8448/o/touch-silo.git'
```
### touch-silo / hold: exit 1, 127.2 ms, answer ['refused'], byte-for-byte None, server refs changed []
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8447/o/touch-silo.git
 ! [remote rejected] main -> refs/silo/backup/heads/main/forged (refs/silo/ is kept by Silo and cannot be changed from the computer)
error: failed to push some refs to 'https://127.0.0.1:8447/o/touch-silo.git'
```
### touch-silo / backup: exit 1, 121.1 ms, answer ['refused'], byte-for-byte None, server refs changed []
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8445/o/touch-silo.git
 ! [remote rejected] main -> refs/silo/backup/heads/main/forged (refs/silo/ is kept by Silo and cannot be changed from the computer)
error: failed to push some refs to 'https://127.0.0.1:8445/o/touch-silo.git'
```
