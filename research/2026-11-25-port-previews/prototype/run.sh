#!/bin/bash
# Every preview test. Needs Python 3, Node with Playwright and its Chromium, and strace for the
# name lookups. Output: <out>/results.json, and dns.txt for the lookups seen leaving the browser.
# usage: ./run.sh <out> [<vite app> <next app>]
set -e; H=$(cd $(dirname $0); pwd); O=$(mkdir -p ${1:-$H/out}; cd ${1:-$H/out}; pwd); rm -f $O/results.json $O/dns.txt
node $H/run.js $O cross toss login persist ui background > /dev/null
for m in "#363" "no resolver"; do
  ONLY="$m" strace -f -qq -e trace=sendto,sendmmsg,sendmsg -s 300 -o $O/strace.out node $H/run.js $O dns > /dev/null
  echo "$m: lookups carrying the secret: $(grep -a 'exfil' $O/strace.out | grep -v CONNECT | grep -o -a -E '[a-z]+-akiac1secret777' | sort | uniq -c | tr -s ' \n' ' ')" >> $O/dns.txt
  rm $O/strace.out
done
[ -n "$2" ] && node $H/devservers.js "$2" "$3" $O > /dev/null
cat $O/dns.txt
