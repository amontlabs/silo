set -u
host=$1; shift
quiet=/var/lib/silo-quiet
mkdir -p "$quiet/disabled" || exit 1
new=$(tr -d - < /proc/sys/kernel/random/uuid) || exit 1
printf '%s\n' "$new" > /etc/machine-id || exit 1
if [ -e /var/lib/dbus/machine-id ] || [ -L /var/lib/dbus/machine-id ]; then
  rm -f /var/lib/dbus/machine-id
  printf '%s\n' "$new" > /var/lib/dbus/machine-id || exit 1
fi
hostname "$host" 2>/dev/null
printf '%s\n' "$host" > /etc/hostname || exit 1
if [ -f /etc/hosts ]; then
  grep -v '^127\.0\.1\.1[[:space:]]' /etc/hosts > "$quiet/hosts.new" || true
  printf '127.0.1.1\t%s\n' "$host" >> "$quiet/hosts.new"
  cat "$quiet/hosts.new" > /etc/hosts; rm -f "$quiet/hosts.new"
fi
park() {
  [ -e "$1" ] || [ -L "$1" ] || return 0
  d="$quiet/disabled$(dirname "$1")"
  mkdir -p "$d" && mv -f "$1" "$d/"
}
for daemon in cron crond anacron atd; do pkill -x "$daemon" 2>/dev/null; done
for f in /var/spool/cron/crontabs/* /var/spool/cron/* /var/spool/cron/atjobs/* /var/spool/cron/atspool/* /etc/cron.d/* /etc/cron.hourly/* /etc/cron.daily/* /etc/cron.weekly/* /etc/cron.monthly/* /etc/anacrontab; do
  case "$f" in */crontabs|*/atjobs|*/atspool|*.placeholder) continue;; esac
  park "$f" || exit 1
done
if [ -f /etc/crontab ]; then
  mkdir -p "$quiet/disabled/etc" && cp /etc/crontab "$quiet/disabled/etc/crontab" || exit 1
  grep '^[[:space:]]*#' /etc/crontab > "$quiet/crontab.new"
  cat "$quiet/crontab.new" > /etc/crontab; rm -f "$quiet/crontab.new"
fi
for dir in /etc/systemd/system /usr/lib/systemd/system /lib/systemd/system; do
  for f in "$dir"/*.timer; do
    [ -e "$f" ] || [ -L "$f" ] || continue
    mkdir -p /etc/systemd/system
    ln -sf /dev/null "/etc/systemd/system/$(basename "$f")" || exit 1
  done
done
for f in /root/.config/systemd/user/*.timer /home/*/.config/systemd/user/*.timer; do park "$f" || exit 1; done
IFS='
'
for a; do
  case "$a" in
    stop=*)
      n=${a#stop=}
      pkill -x -- "$n" 2>/dev/null
      mkdir -p /etc/systemd/system
      ln -sf /dev/null "/etc/systemd/system/${n%.service}.service"
      case "$n" in *.*) ln -sf /dev/null "/etc/systemd/system/$n";; esac
      [ -f "/etc/init.d/$n" ] && chmod a-x "/etc/init.d/$n"
      rm -f /etc/rc[0-9S].d/[SK][0-9][0-9]"$n"
      for f in "/etc/xdg/autostart/$n.desktop" "/root/.config/autostart/$n.desktop" /home/*/.config/autostart/"$n".desktop "/etc/supervisor/conf.d/$n.conf"; do park "$f" || exit 1; done
      ;;
  esac
done
for a; do
  case "$a" in
    scrub=*)
      for f in ${a#scrub=}; do rm -rf -- "$f" || exit 1; done
      ;;
  esac
done
exit 0
