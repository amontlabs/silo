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
| touch-silo | `refs/silo/backup/heads/main/forged` | create | create  | - | - | - | no | no pack |

## Time (median over reps, ms)

| scenario | wall pass | wall observe | wall backup | guard work in backup (parse+classify+backup) | classify | backup refs | pack MB |
|---|---|---|---|---|---|---|---|
| normal | 147.9 | 149.1 | 142.6 | 0.2 | 0.1 | None | 0.0 |
| new-branch | 145.3 | 146.0 | 143.9 | 0.1 | 0.0 | None | 0.0 |
| delete-branch | 135.5 | 130.9 | 135.1 | 2.5 | 0.0 | 2.5 | 0.0 |
| force-main | 143.0 | 148.2 | 142.7 | 4.2 | 1.9 | 2.4 | 0.0 |
| rewind-main | 144.8 | 152.8 | 148.3 | 4.6 | 2.1 | 2.2 | 0.0 |
| force-rebase | 147.5 | 147.9 | 145.3 | 4.2 | 2.0 | 1.9 | 0.0 |
| merge-ff | 151.3 | 145.7 | 135.1 | 0.2 | 0.1 | None | 0.0 |
| multi | 139.5 | 149.9 | 148.6 | 4.4 | 2.0 | 2.1 | 0.0 |
| atomic | 141.9 | 145.5 | 147.3 | 4.1 | 2.0 | 2.1 | 0.0 |
| mirror | 142.0 | 125.6 | 120.4 | None | 0.1 | None | 0.0 |
| mirror-hide | None | None | 152.4 | 8.2 | 0.1 | 8.0 | None |
| tag-delete | 126.8 | 132.1 | 136.7 | 2.2 | 0.0 | 2.1 | 0.0 |
| push-options | 138.9 | 147.1 | 138.9 | 0.1 | 0.1 | None | 0.0 |
| large | 401.0 | 408.2 | 406.1 | 59.8 | 5.0 | None | 1.6 |
| large-force | 392.6 | 394.3 | 411.3 | 71.3 | 8.0 | 2.4 | 1.6 |
| touch-silo | 139.1 | 125.5 | 122.3 | None | 0.0 | None | 0.0 |

## Per-method time inside observe (median ms per force/ff ref)

| scenario | ref | (a) pack | (b) server | (c) hybrid |
|---|---|---|---|---|
| normal | `refs/heads/main` | 0.1 | 2.0 | 0.1 |
| force-main | `refs/heads/main` | 0.1 | 2.2 | 2.1 |
| rewind-main | `refs/heads/main` | 0.0 | 3.9 | 2.0 |
| force-rebase | `refs/heads/feature` | 0.1 | 1.9 | 2.6 |
| merge-ff | `refs/heads/main` | 0.1 | 1.9 | 0.1 |
| multi | `refs/heads/feature` | 0.1 | 1.7 | 0.1 |
| multi | `refs/heads/main` | 0.1 | 1.6 | 1.7 |
| atomic | `refs/heads/feature` | 0.1 | 1.7 | 0.1 |
| atomic | `refs/heads/main` | 0.1 | 1.8 | 2.3 |
| mirror | `refs/heads/main` | 0.1 | 1.6 | 0.1 |
| push-options | `refs/heads/main` | 0.1 | 2.1 | 0.1 |
| large | `refs/heads/main` | 5.8 | 2.1 | 5.8 |
| large-force | `refs/heads/main` | 5.6 | 1.9 | 7.6 |

## What the agent saw (refuse, split, hold) and what the server got

### normal / refuse: exit 0, 143.0 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8446/o/normal.git
   5f0b987..90b920b  main -> main
```
### normal / split: exit 0, 145.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8448/o/normal.git
   aa2e669..1d01d0d  main -> main
```
### normal / hold: exit 0, 148.5 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/normal.git
   5f0b987..58ced64  main -> main
```
### normal / backup: exit 0, 139.5 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/normal.git
   69e9f2a..d2a3cf0  main -> main
```
### new-branch / refuse: exit 0, 144.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/topic']
```
To https://127.0.0.1:8446/o/new-branch.git
 * [new branch]      topic -> topic
```
### new-branch / split: exit 0, 144.3 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/topic']
```
To https://127.0.0.1:8448/o/new-branch.git
 * [new branch]      topic -> topic
```
### new-branch / hold: exit 0, 138.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/topic']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/new-branch.git
 * [new branch]      topic -> topic
```
### new-branch / backup: exit 0, 137.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/topic']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/new-branch.git
 * [new branch]      topic -> topic
```
### delete-branch / refuse: exit 1, 117.6 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/delete-branch.git
 ! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/delete-branch.git'
```
### delete-branch / split: exit 1, 123.9 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/delete-branch.git
 ! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/delete-branch.git'
```
### delete-branch / hold: exit 0, 2127.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/silo/backup/heads/feature/20261124T160842865050']
backups survive gc: {'refs/silo/backup/heads/feature/20261124T160842865050': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/delete-branch.git
 - [deleted]         feature
```
### delete-branch / backup: exit 0, 135.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/silo/backup/heads/feature/20261124T160835252786']
backups survive gc: {'refs/silo/backup/heads/feature/20261124T160835252786': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/delete-branch.git
 - [deleted]         feature
```
### force-main / refuse: exit 1, 120.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/force-main.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/force-main.git'
```
### force-main / split: exit 1, 122.4 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/force-main.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/force-main.git'
```
### force-main / hold: exit 0, 2152.3 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T160858955564']
backups survive gc: {'refs/silo/backup/heads/main/20261124T160858955564': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/force-main.git
 + 9c6add8...2f7b6e0 main -> main (forced update)
```
### force-main / backup: exit 0, 136.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T160851257099']
backups survive gc: {'refs/silo/backup/heads/main/20261124T160851257099': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/force-main.git
 + 14967bf...c9fc01d main -> main (forced update)
```
### rewind-main / refuse: exit 1, 127.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/rewind-main.git
 ! [remote rejected] HEAD~2 -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/rewind-main.git'
```
### rewind-main / split: exit 1, 123.7 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/rewind-main.git
 ! [remote rejected] HEAD~2 -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/rewind-main.git'
```
### rewind-main / hold: exit 0, 2162.9 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T160915253254']
backups survive gc: {'refs/silo/backup/heads/main/20261124T160915253254': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/rewind-main.git
 + 0b27264...e476b8a HEAD~2 -> main (forced update)
```
### rewind-main / backup: exit 0, 143.1 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T160907428989']
backups survive gc: {'refs/silo/backup/heads/main/20261124T160907428989': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/rewind-main.git
 + e6038c6...9416987 HEAD~2 -> main (forced update)
```
### force-rebase / refuse: exit 1, 118.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/feature (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/force-rebase.git
 ! [remote rejected] feature -> feature (Held by Silo: rewrites refs/heads/feature (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/force-rebase.git'
```
### force-rebase / split: exit 1, 121.5 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/feature (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/force-rebase.git
 ! [remote rejected] feature -> feature (Held by Silo: rewrites refs/heads/feature (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/force-rebase.git'
```
### force-rebase / hold: exit 0, 2150.6 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/silo/backup/heads/feature/20261124T160931286769']
backups survive gc: {'refs/silo/backup/heads/feature/20261124T160931286769': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/force-rebase.git
 + 2cb058e...448b070 feature -> feature (forced update)
```
### force-rebase / backup: exit 0, 160.8 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/silo/backup/heads/feature/20261124T160923678145']
backups survive gc: {'refs/silo/backup/heads/feature/20261124T160923678145': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/force-rebase.git
 + 2214034...db6f687 feature -> feature (forced update)
```
### merge-ff / refuse: exit 0, 145.9 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8446/o/merge-ff.git
   924c72d..a22f718  main -> main
```
### merge-ff / split: exit 0, 144.2 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8448/o/merge-ff.git
   9fb0e15..7406b05  main -> main
```
### merge-ff / hold: exit 0, 134.6 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/merge-ff.git
   fceb7d8..811f1be  main -> main
```
### merge-ff / backup: exit 0, 135.1 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/merge-ff.git
   8b2843c..a3d50d6  main -> main
```
### multi / refuse: exit 1, 122.8 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/multi.git
 ! [remote rejected] feature -> feature (refused with the held refs of this push)
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/multi.git'
```
### multi / split: exit 1, 143.1 ms, answer ['split'], byte-for-byte None, server refs changed ['refs/heads/feature']
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/multi.git
   07cb202..4b54de7  feature -> feature
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/multi.git'
```
### multi / hold: exit 0, 2163.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/silo/backup/heads/main/20261124T161001357323']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161001357323': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/multi.git
   0a48a94..3cc3660  feature -> feature
 + aa7ae25...07f1a1d main -> main (forced update)
```
### multi / backup: exit 0, 145.1 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/silo/backup/heads/main/20261124T160953725812']
backups survive gc: {'refs/silo/backup/heads/main/20261124T160953725812': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/multi.git
   13cfdb6..dc659bf  feature -> feature
 + c8299e6...0fac2c7 main -> main (forced update)
```
### atomic / refuse: exit 1, 124.1 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/atomic.git
 ! [remote rejected] feature -> feature (atomic push: refused with the held refs)
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/atomic.git'
```
### atomic / split: exit 1, 124.5 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/atomic.git
 ! [remote rejected] feature -> feature (atomic push: refused with the held refs)
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/atomic.git'
```
### atomic / hold: exit 0, 2139.0 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/silo/backup/heads/main/20261124T161017452963']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161017452963': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/atomic.git
   2684f43..618c62c  feature -> feature
 + 845894c...f2ec5c1 main -> main (forced update)
```
### atomic / backup: exit 0, 138.8 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/silo/backup/heads/main/20261124T161009675415']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161009675415': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/atomic.git
   de5866e..0c66a25  feature -> feature
 + 903b9ef...2b5219e main -> main (forced update)
```
### mirror / refuse: exit 1, 117.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/heads/old-feature. Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/mirror.git
 ! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.)
 ! [remote rejected] main -> main (refused with the held refs of this push)
 ! [remote rejected] old-feature (Held by Silo: deletes refs/heads/old-feature. Ask the user in the Silo app.)
 ! [remote rejected] refs/silo/backup/heads/main/earlier (Held by Silo: deletes refs/silo/backup/heads/main/earlier. Ask the user in the Silo app.)
 ! [remote rejected] v1 (Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.)
 ! [remote rejected] origin/HEAD -> origin/HEAD (refused with the held refs of this push)
 ! [remote rejected] origin/feature -> origin/feature (refused with the held refs of this push)
 ! [remote rejected] origin/main -> origin/main (refused with the held refs of this push)
 ! [remote rejected] origin/old-feature -> origin/old-feature (refused with the held refs of this push)
error: failed to push some refs to 'https://127.0.0.1:8446/o/mirror.git'
```
### mirror / split: exit 1, 124.4 ms, answer ['refused'], byte-for-byte None, server refs changed []
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
### mirror / hold: exit 1, 120.2 ms, answer ['refused'], byte-for-byte None, server refs changed []
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
### mirror / backup: exit 1, 117.9 ms, answer ['refused'], byte-for-byte None, server refs changed []
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
### mirror-hide / split: exit 1, 151.4 ms, answer ['split'], byte-for-byte None, server refs changed ['refs/heads/main', 'refs/remotes/origin/HEAD', 'refs/remotes/origin/feature', 'refs/remotes/origin/main', 'refs/remotes/origin/old-feature']
```
remote: Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/mirror-hide.git
   eab4955..61b1598  main -> main
 * [new reference]   origin/HEAD -> origin/HEAD
 * [new reference]   origin/feature -> origin/feature
 * [new reference]   origin/main -> origin/main
 * [new reference]   origin/old-feature -> origin/old-feature
 ! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. Ask the user in the Silo app.)
 ! [remote rejected] old-feature (Held by Silo: deletes refs/heads/old-feature. Ask the user in the Silo app.)
 ! [remote rejected] v1 (Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/mirror-hide.git'
```
### mirror-hide / backup: exit 0, 152.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/feature', 'refs/heads/main', 'refs/heads/old-feature', 'refs/remotes/origin/HEAD', 'refs/remotes/origin/feature', 'refs/remotes/origin/main', 'refs/remotes/origin/old-feature', 'refs/silo/backup/heads/feature/20261124T161032493257', 'refs/silo/backup/heads/old-feature/20261124T161032493257', 'refs/silo/backup/tags/v1/20261124T161032493257', 'refs/tags/v1']
backups survive gc: {'refs/silo/backup/tags/v1/20261124T161032493257': True, 'refs/silo/backup/heads/feature/20261124T161032493257': True, 'refs/silo/backup/heads/old-feature/20261124T161032493257': True}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/mirror-hide.git
 - [deleted]         feature
   8e0d566..1a4bd63  main -> main
 - [deleted]         old-feature
 - [deleted]         v1
 * [new reference]   origin/HEAD -> origin/HEAD
 * [new reference]   origin/feature -> origin/feature
 * [new reference]   origin/main -> origin/main
 * [new reference]   origin/old-feature -> origin/old-feature
```
### tag-delete / refuse: exit 1, 114.1 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/tag-delete.git
 ! [remote rejected] v1 (Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/tag-delete.git'
```
### tag-delete / split: exit 1, 121.2 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/tag-delete.git
 ! [remote rejected] v1 (Held by Silo: deletes refs/tags/v1. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/tag-delete.git'
```
### tag-delete / hold: exit 0, 2146.4 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/silo/backup/tags/v1/20261124T161047676868', 'refs/tags/v1']
backups survive gc: {'refs/silo/backup/tags/v1/20261124T161047676868': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/tag-delete.git
 - [deleted]         v1
```
### tag-delete / backup: exit 0, 141.1 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/silo/backup/tags/v1/20261124T161040249073', 'refs/tags/v1']
backups survive gc: {'refs/silo/backup/tags/v1/20261124T161040249073': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/tag-delete.git
 - [deleted]         v1
```
### push-options / refuse: exit 0, 141.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8446/o/push-options.git
   59ef474..a10f409  main -> main
```
### push-options / split: exit 0, 143.5 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8448/o/push-options.git
   2553fa6..2362bc5  main -> main
```
### push-options / hold: exit 0, 136.0 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/push-options.git
   59ef474..52dbf10  main -> main
```
### push-options / backup: exit 0, 137.9 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/push-options.git
   4395b40..206c043  main -> main
```
### large / refuse: exit 0, 569.5 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8446/o/large.git
   c2f1f80..d64c39f  main -> main
```
### large / split: exit 0, 522.2 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
```
To https://127.0.0.1:8448/o/large.git
   197edf7..cfac7ee  main -> main
```
### large / hold: exit 0, 540.6 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8447/o/large.git
   ecc493b..eb842a3  main -> main
```
### large / backup: exit 0, 513.8 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main']
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
To https://127.0.0.1:8445/o/large.git
   6f5f547..56230fd  main -> main
```
### large-force / refuse: exit 1, 265.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/large-force.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/large-force.git'
```
### large-force / split: exit 1, 271.8 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.        
To https://127.0.0.1:8448/o/large-force.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8448/o/large-force.git'
```
### large-force / hold: exit 0, 2406.7 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T161145018389']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161145018389': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8447/o/large-force.git
 + cb301b0...2a92e27 main -> main (forced update)
```
### large-force / backup: exit 0, 414.2 ms, answer ['passed'], byte-for-byte True, server refs changed ['refs/heads/main', 'refs/silo/backup/heads/main/20261124T161134629511']
backups survive gc: {'refs/silo/backup/heads/main/20261124T161134629511': True}; mirror clone sees refs/silo: True; ls-remote shows: True
```
To https://127.0.0.1:8445/o/large-force.git
 + f32c30a...8ec4ce4 main -> main (forced update)
```
### touch-silo / refuse: exit 1, 122.2 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: Held by Silo: changes refs/silo/backup/heads/main/forged. Ask the user in the Silo app.        
To https://127.0.0.1:8446/o/touch-silo.git
 ! [remote rejected] main -> refs/silo/backup/heads/main/forged (Held by Silo: changes refs/silo/backup/heads/main/forged. Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/touch-silo.git'
```
### touch-silo / split: exit 1, 130.6 ms, answer ['refused'], byte-for-byte None, server refs changed []
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8448/o/touch-silo.git
 ! [remote rejected] main -> refs/silo/backup/heads/main/forged (refs/silo/ is kept by Silo and cannot be changed from the computer)
error: failed to push some refs to 'https://127.0.0.1:8448/o/touch-silo.git'
```
### touch-silo / hold: exit 1, 118.0 ms, answer ['refused'], byte-for-byte None, server refs changed []
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8447/o/touch-silo.git
 ! [remote rejected] main -> refs/silo/backup/heads/main/forged (refs/silo/ is kept by Silo and cannot be changed from the computer)
error: failed to push some refs to 'https://127.0.0.1:8447/o/touch-silo.git'
```
### touch-silo / backup: exit 1, 120.9 ms, answer ['refused'], byte-for-byte None, server refs changed []
backups survive gc: {}; mirror clone sees refs/silo: False; ls-remote shows: False
```
remote: refs/silo/ is kept by Silo and cannot be changed from the computer        
To https://127.0.0.1:8445/o/touch-silo.git
 ! [remote rejected] main -> refs/silo/backup/heads/main/forged (refs/silo/ is kept by Silo and cannot be changed from the computer)
error: failed to push some refs to 'https://127.0.0.1:8445/o/touch-silo.git'
```
