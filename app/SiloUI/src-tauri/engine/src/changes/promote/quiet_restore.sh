set -u
# Puts a promoted fork's parked scheduled jobs back (`restore`), or parks them again (`repark`).
# The jobs were moved under /var/lib/silo-quiet/disabled when the fork was made quiet. An item
# the fork has replaced since stays where it is. Repeating either mode finds nothing more to do.
# SILO_ROOT is a prefix for every path, so the script can be tried on a directory.
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

remember() { printf '%s %s\n' "$1" "$2" >> "$marker" || exit 1; }

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
        rm -f "$dest" || exit 1
      else
        echo "left $rel"
        continue
      fi
    fi
    mkdir -p "$(dirname "$dest")" || exit 1
    mv "$from" "$dest" || exit 1
    remember "$kind" "$rel"
    echo "restored $rel"
  done || exit 1
  if [ -f "$disabled/etc/crontab" ]; then
    if crontab_is_quiet; then
      cat "$disabled/etc/crontab" > "$root/etc/crontab" || exit 1
      rm -f "$disabled/etc/crontab"
      remember crontab /etc/crontab
      echo "restored /etc/crontab"
    else
      echo "left /etc/crontab"
    fi
  fi
  # A timer the quiet step masked had no file to park: its vendor unit is what comes back.
  for f in "$root"/etc/systemd/system/*.timer; do
    [ -L "$f" ] && [ "$(readlink "$f")" = /dev/null ] || continue
    unit=${f##*/}
    if [ -e "$root/usr/lib/systemd/system/$unit" ] || [ -e "$root/lib/systemd/system/$unit" ]; then
      rm -f "$f" || exit 1
      remember unmask "/etc/systemd/system/$unit"
      echo "restored /etc/systemd/system/$unit"
    fi
  done
  [ -n "$root" ] || systemctl daemon-reload >/dev/null 2>&1 || true
  sync
}

repark() {
  [ -f "$marker" ] || exit 0
  while IFS=' ' read -r kind rel; do
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
        if [ -f "$dest" ] && [ ! -e "$disabled/etc/crontab" ]; then
          mkdir -p "$disabled/etc" || exit 1
          cp "$dest" "$disabled/etc/crontab" || exit 1
          grep '^[[:space:]]*#' "$dest" > "$quiet/crontab.new"
          cat "$quiet/crontab.new" > "$dest"; rm -f "$quiet/crontab.new"
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
