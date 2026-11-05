set -u
token=$1; host=$2; shift 2
case $token in ''|*[!0-9a-f-]*) exit 1;; esac
quiet=/var/lib/silo-quiet
mkdir -p "$quiet/disabled" || exit 1

if [ -s "$quiet/$token.machine-id" ]; then
  new=$(cat "$quiet/$token.machine-id") || exit 1
else
  new=$(tr -d - < /proc/sys/kernel/random/uuid) || exit 1
  printf '%s\n' "$new" > "$quiet/$token.machine-id" || exit 1
fi
printf '%s\n' "$new" > /etc/machine-id || exit 1
if [ -e /var/lib/dbus/machine-id ] || [ -L /var/lib/dbus/machine-id ]; then
  rm -f /var/lib/dbus/machine-id
  printf '%s\n' "$new" > /var/lib/dbus/machine-id || exit 1
fi

if [ ! -e "$quiet/$token.old-hostname" ]; then
  { cat /etc/hostname 2>/dev/null || hostname; } > "$quiet/$token.old-hostname" || exit 1
fi
old=$(head -n 1 "$quiet/$token.old-hostname")
hostname "$host" || exit 1
printf '%s\n' "$host" > /etc/hostname || exit 1
if [ -f /etc/hosts ]; then
  grep -v '^127\.0\.1\.1[[:space:]]' /etc/hosts > "$quiet/hosts.new" || true
  case $old in
    ''|"$host"|*[!A-Za-z0-9.-]*) ;;
    *)
      esc=$(printf '%s' "$old" | sed 's/\./\\./g')
      for _ in 1 2; do
        sed -E "/^[[:space:]]*#/!s/(^|[[:space:]])$esc([[:space:]]|\$)/\1$host\2/g" "$quiet/hosts.new" > "$quiet/hosts.sed" || exit 1
        cat "$quiet/hosts.sed" > "$quiet/hosts.new"
      done
      rm -f "$quiet/hosts.sed"
      ;;
  esac
  printf '127.0.1.1\t%s\n' "$host" >> "$quiet/hosts.new"
  cat "$quiet/hosts.new" > /etc/hosts || exit 1
  rm -f "$quiet/hosts.new"
fi

# Moves a file or link under the saved originals. An earlier copy is never replaced.
park() {
  [ -e "$1" ] || [ -L "$1" ] || return 0
  d="$quiet/disabled$(dirname "$1")"
  mkdir -p "$d" || return 1
  dest="$d/$(basename "$1")"
  n=0
  while [ -e "$dest" ] || [ -L "$dest" ]; do n=$((n + 1)); dest="$d/$(basename "$1").$n"; done
  mv -f "$1" "$dest"
}

# Succeeds when the argv on stdin, one entry per line, belongs to the program or script $1: the
# first entry, or the first one that is not an option, has that base name.
argv_matches() {
  count=0
  while IFS= read -r arg; do
    count=$((count + 1))
    if [ "$count" -gt 1 ]; then
      case $arg in -*) continue;; esac
    fi
    [ "${arg##*/}" = "$1" ] && return 0
    [ "$count" -gt 1 ] && return 1
  done
  return 1
}

# Masks a unit by name, keeping a real file of that name under the saved originals.
mask() {
  if [ -L "$1" ] && [ "$(readlink "$1")" = /dev/null ]; then return 0; fi
  park "$1" && ln -sf /dev/null "$1"
}

# Succeeds when the Exec line of the autostart entry $1 has a word whose base name is $2, so an
# entry still matches when it starts the program through a wrapper.
desktop_runs() {
  line=$(grep -m 1 '^Exec=' "$1" 2>/dev/null) || return 1
  printf '%s\n' "${line#Exec=}" | tr -s ' \t' '\n\n' | (
    while IFS= read -r w; do
      w=${w#\"}; w=${w%\"}; w=${w#\'}; w=${w%\'}
      [ "${w##*/}" = "$2" ] && exit 0
    done
    exit 1
  )
}

pids_of() {
  for d in /proc/[0-9]*; do
    p=${d#/proc/}
    [ "$p" = 1 ] && continue
    [ "$p" = "$$" ] && continue
    [ "$p" = "$PPID" ] && continue
    [ -r "$d/cmdline" ] || continue
    if tr '\0' '\n' < "$d/cmdline" 2>/dev/null | argv_matches "$1"; then printf '%s\n' "$p"; fi
  done
}

stop_processes() {
  pids=$(pids_of "$1")
  [ -z "$pids" ] && return 0
  for p in $pids; do kill -TERM "$p" 2>/dev/null; done
  i=0
  while [ "$i" -lt 25 ]; do
    [ -z "$(pids_of "$1")" ] && return 0
    sleep 0.2
    i=$((i + 1))
  done
  for p in $(pids_of "$1"); do kill -KILL "$p" 2>/dev/null; done
  sleep 0.5
  [ -z "$(pids_of "$1")" ]
}

for daemon in cron crond anacron atd; do
  stop_processes "$daemon" || exit 1
done
for f in /var/spool/cron/crontabs/* /var/spool/cron/* /var/spool/cron/atjobs/* /var/spool/cron/atspool/* /etc/cron.d/* /etc/cron.hourly/* /etc/cron.daily/* /etc/cron.weekly/* /etc/cron.monthly/* /etc/anacrontab; do
  case "$f" in */crontabs|*/atjobs|*/atspool|*.placeholder) continue;; esac
  park "$f" || exit 1
done
if [ -f /etc/crontab ]; then
  mkdir -p "$quiet/disabled/etc" || exit 1
  [ -e "$quiet/disabled/etc/crontab" ] || cp /etc/crontab "$quiet/disabled/etc/crontab" || exit 1
  grep '^[[:space:]]*#' /etc/crontab > "$quiet/crontab.new"
  cat "$quiet/crontab.new" > /etc/crontab; rm -f "$quiet/crontab.new"
fi

mkdir -p /etc/systemd/system || exit 1
# A timer the computer defined itself is saved before a mask takes its name. Each mask made here
# is listed first, so a promote removes those and no mask that was there before or came later.
masked_list=$quiet/$token.masked-timers.list
for dir in /etc/systemd/system /usr/lib/systemd/system /lib/systemd/system; do
  for f in "$dir"/*.timer; do
    [ -e "$f" ] || [ -L "$f" ] || continue
    unit=/etc/systemd/system/$(basename "$f")
    if ! { [ -L "$unit" ] && [ "$(readlink "$unit")" = /dev/null ]; }; then
      grep -qxF "$unit" "$masked_list" 2>/dev/null || printf '%s\n' "$unit" >> "$masked_list" || exit 1
    fi
    mask "$unit" || exit 1
  done
done
for f in /root/.config/systemd/user/*.timer /home/*/.config/systemd/user/*.timer; do park "$f" || exit 1; done

IFS='
'
for a; do
  case "$a" in
    stop=*)
      n=${a#stop=}
      case $n in ''|.|..|-*|*[!A-Za-z0-9._@:+-]*) exit 1;; esac
      unit=${n%.service}
      stop_processes "$n" || exit 1
      [ "$unit" != "$n" ] && { stop_processes "$unit" || exit 1; }
      mask "/etc/systemd/system/$unit.service" || exit 1
      case "$n" in *.*) [ "$unit" = "$n" ] && { mask "/etc/systemd/system/$n" || exit 1; };; esac
      if [ -f "/etc/init.d/$n" ]; then chmod a-x "/etc/init.d/$n" || exit 1; fi
      for f in /etc/rc[0-9S].d/[SK][0-9][0-9]"$n"; do
        [ -e "$f" ] || [ -L "$f" ] || continue
        park "$f" || exit 1
      done
      for f in "/etc/xdg/autostart/$n.desktop" "/root/.config/autostart/$n.desktop" /home/*/.config/autostart/"$n".desktop "/etc/supervisor/conf.d/$n.conf"; do park "$f" || exit 1; done
      for f in /etc/xdg/autostart/*.desktop /root/.config/autostart/*.desktop /home/*/.config/autostart/*.desktop; do
        [ -f "$f" ] || continue
        if desktop_runs "$f" "$n"; then park "$f" || exit 1; fi
      done
      for f in "/root/.config/systemd/user/$unit.service" /home/*/.config/systemd/user/"$unit".service; do park "$f" || exit 1; done
      mkdir -p /etc/systemd/user || exit 1
      mask "/etc/systemd/user/$unit.service" || exit 1
      ;;
  esac
done

# Removes what a scrub pattern matches. Each match is resolved through its parent directory
# first; one whose real parent is not inside the pattern's literal directory, or that lands
# at the top of the file system, stops the script. Links are removed, never followed.
scrub_one() {
  pattern=$1
  prefix=${pattern%%[*?[]*}
  prefix=${prefix%/*}
  [ -n "$prefix" ] || return 1
  [ -d "$prefix" ] || return 0
  for f in $pattern; do
    [ -e "$f" ] || [ -L "$f" ] || continue
    name=${f##*/}
    case $name in .|..) continue;; esac
    parent=$(cd -P -- "${f%/*}" 2>/dev/null && pwd -P) || continue
    case "$parent/" in "$prefix"/*) ;; *) return 1;; esac
    target=$parent/$name
    case ${target#/} in */*) ;; *) return 1;; esac
    rm -rf -- "$target" || return 1
  done
  return 0
}
for a; do
  case "$a" in
    scrub=*) scrub_one "${a#scrub=}" || exit 1;;
  esac
done

: > "$quiet/applied-$token" || exit 1
sync
exit 0
