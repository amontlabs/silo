set -u
# Puts a promoted fork's parked scheduled jobs back (`restore`), or parks them again (`repark`).
# The jobs were moved under /var/lib/silo-quiet/disabled when the fork was made quiet. An item
# the fork has replaced since stays where it is. Repeating either mode finds nothing more to do.
# SILO_ROOT is a prefix for every path, so the script can be tried on a directory.
#
# Every change is written to restored.list before it is made ("pending <kind> <path>") and
# marked afterwards ("done <kind> <path>"); each change is one rename or removal, so a run that
# stops anywhere leaves an item either untouched or complete. `repark` reads the list and looks
# at what each item is now. `restore` ends by printing the whole list as "recorded ..." lines,
# so a run that finds nothing left to do still reports what an earlier run restored.
mode=${1:-}
root=${SILO_ROOT:-}
quiet=$root/var/lib/silo-quiet
disabled=$quiet/disabled
marker=$quiet/restored.list
# The parked paths that are scheduled jobs; everything else under disabled/ belongs to the
# services the fork was asked to stop and stays parked.
scheduled() {
  case $1 in
    /var/spool/cron/*|/etc/cron.d/*|/etc/cron.hourly/*|/etc/cron.daily/*|/etc/cron.weekly/*|/etc/cron.monthly/*) return 0;;
    /etc/anacrontab|/etc/systemd/system/*.timer) return 0;;
    /root/.config/systemd/user/*.timer|/home/*/.config/systemd/user/*.timer) return 0;;
  esac
  return 1
}

# Written and flushed before the change it names.
intend() { printf 'pending %s %s\n' "$1" "$2" >> "$marker" || exit 1; sync; }
finish() { printf 'done %s %s\n' "$1" "$2" >> "$marker" || exit 1; }

parked_list() {
  [ -d "$disabled" ] || return 0
  find "$disabled" \( -type f -o -type l \) -print | while IFS= read -r p; do
    rel=${p#"$disabled"}
    scheduled "$rel" && printf '%s\n' "$rel"
  done
}

# Whether /etc/crontab holds nothing but comments, as the quiet step leaves it.
crontab_is_quiet() {
  [ -f "$root/etc/crontab" ] || return 1
  ! grep -q '^[[:space:]]*[^[:space:]#]' "$root/etc/crontab"
}

# A collision copy ("name.1") is an earlier item the quiet step kept; it is never restored.
collision_copy() {
  case $1 in
    *.[0-9]|*.[0-9][0-9]) [ -e "$disabled${1%.*}" ] || [ -L "$disabled${1%.*}" ];;
    *) return 1;;
  esac
}

# Whether the quiet step listed the mask at $1 as its own (<token>.masked-timers.list).
masked_by_quiet() {
  for list in "$quiet"/*.masked-timers.list; do
    [ -f "$list" ] && grep -qxF "$1" "$list" && return 0
  done
  return 1
}

restore() {
  mkdir -p "$quiet" || exit 1
  parked_list | while IFS= read -r rel; do
    collision_copy "$rel" && continue
    dest=$root$rel
    from=$disabled$rel
    kind=file
    if [ -e "$dest" ] || [ -L "$dest" ]; then
      if [ -L "$dest" ] && [ "$(readlink "$dest")" = /dev/null ]; then
        kind=mask
      else
        echo "left $rel"
        continue
      fi
    fi
    mkdir -p "$(dirname "$dest")" || exit 1
    intend "$kind" "$rel"
    # One rename; over a mask it replaces the link, so the timer is never left unmasked and
    # unrestored in between.
    mv -f "$from" "$dest" || exit 1
    finish "$kind" "$rel"
    echo "restored $rel"
  done || exit 1
  if [ -f "$disabled/etc/crontab" ]; then
    if [ -f "$root/etc/crontab" ] && cmp -s "$disabled/etc/crontab" "$root/etc/crontab"; then
      # An earlier run wrote it and stopped before removing the saved copy.
      rm -f "$disabled/etc/crontab" || exit 1
      finish crontab /etc/crontab
      echo "restored /etc/crontab"
    elif crontab_is_quiet; then
      intend crontab /etc/crontab
      cat "$disabled/etc/crontab" > "$root/etc/crontab.silo-new" || exit 1
      mv -f "$root/etc/crontab.silo-new" "$root/etc/crontab" || exit 1
      rm -f "$disabled/etc/crontab" || exit 1
      finish crontab /etc/crontab
      echo "restored /etc/crontab"
    else
      echo "left /etc/crontab"
    fi
  fi
  # A timer the quiet step masked had no file to park: its vendor unit is what comes back. Only
  # the masks the quiet step recorded are removed.
  for f in "$root"/etc/systemd/system/*.timer; do
    [ -L "$f" ] && [ "$(readlink "$f")" = /dev/null ] || continue
    unit=${f##*/}
    masked_by_quiet "/etc/systemd/system/$unit" || continue
    if [ -e "$root/usr/lib/systemd/system/$unit" ] || [ -e "$root/lib/systemd/system/$unit" ]; then
      intend unmask "/etc/systemd/system/$unit"
      rm -f "$f" || exit 1
      finish unmask "/etc/systemd/system/$unit"
      echo "restored /etc/systemd/system/$unit"
    fi
  done
  [ -n "$root" ] || systemctl daemon-reload >/dev/null 2>&1 || true
  sync
  [ -f "$marker" ] && sed 's/^/recorded /' "$marker"
  return 0
}

repark() {
  [ -f "$marker" ] || exit 0
  while IFS=' ' read -r state kind rel; do
    # A list written before the states existed has only "<kind> <path>".
    case $state in pending|done) ;; *) rel=$kind; kind=$state;; esac
    [ -n "$rel" ] || continue
    dest=$root$rel
    case $kind in
      file|mask)
        from=$disabled$rel
        if [ -e "$dest" ] && [ ! -e "$from" ] && [ ! -L "$from" ]; then
          mkdir -p "$(dirname "$from")" || exit 1
          mv "$dest" "$from" || exit 1
          [ "$kind" = mask ] && { ln -sf /dev/null "$dest" || exit 1; }
        fi
        ;;
      unmask)
        if [ ! -e "$dest" ] && [ ! -L "$dest" ]; then ln -sf /dev/null "$dest" || exit 1; fi
        ;;
      crontab)
        if [ -f "$dest" ]; then
          if [ ! -e "$disabled/etc/crontab" ]; then
            mkdir -p "$disabled/etc" || exit 1
            cp "$dest" "$disabled/etc/crontab" || exit 1
          fi
          # Quiet again unless it already is (the saved copy then still holds the jobs).
          if ! crontab_is_quiet; then
            grep '^[[:space:]]*#' "$dest" > "$quiet/crontab.new"
            cat "$quiet/crontab.new" > "$dest"; rm -f "$quiet/crontab.new"
          fi
        fi
        ;;
    esac
  done < "$marker"
  rm -f "$marker" || exit 1
  [ -n "$root" ] || systemctl daemon-reload >/dev/null 2>&1 || true
  sync
}

case $mode in
  restore) restore;;
  repark) repark;;
  *) exit 2;;
esac
exit 0
