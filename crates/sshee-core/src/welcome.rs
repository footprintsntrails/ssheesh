//! What a new terminal on a server shows first: the server's own welcome
//! (motd), SSHeesh's (the logo beside the server's facts), or nothing. The
//! server's notices (updates waiting, a restart needed) always stay.

use crate::ssh::shell_quote;
use serde::Deserialize;

/// The choice from Settings, as the app sends it for each new session.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Welcome {
    /// "auto": the server's own if it has one, else SSHeesh's. "ssheesh",
    /// "server" or "off". (The app turns "first time" into one of these.)
    #[serde(default)]
    pub mode: String,
    /// What SSHeesh's shows: "logo", "system", "usage", "status".
    #[serde(default)]
    pub parts: Vec<String>,
    /// Your own line under it.
    #[serde(default)]
    pub note: String,
}

/// The script, run on the server before the login shell starts. It reads the
/// motd files the way a login would (each once, none with ~/.hushlogin),
/// tells a server's own message from the distribution's boilerplate, and
/// draws SSHeesh's welcome with the server's facts: a few quick reads of
/// /proc, df and docker, nothing that waits on the network.
const SCRIPT: &str = r#"E=$(printf '\033'); R="$E[0m"; w_mode=@MODE@; w_parts=@PARTS@; w_note=@NOTE@
m=; if [ ! -e "$HOME/.hushlogin" ]; then m=$(seen=; for f in /run/motd.dynamic /etc/motd /run/motd.d/* /etc/motd.d/* /usr/lib/motd.d/*; do [ -f "$f" ] && [ -r "$f" ] || continue; r=$(readlink -f "$f" 2>/dev/null || echo "$f"); case " $seen " in *" $r "*) continue ;; esac; seen="$seen $r"; cat "$f"; done 2>/dev/null); elif [ "$w_mode" != ssheesh ]; then w_mode=off; fi
w_notice='updates? can be applied|security updates?|apt list --upgradable|ESM|restart required|Learn more about enabling'
w_boiler='^[[:space:]]*\* (Documentation|Management|Support):|^Welcome to (Ubuntu|Debian|Alpine|Fedora|CentOS|Rocky|AlmaLinux|Arch|Void)|System information as of|System load:|Usage of /|Memory usage:|Swap usage:|Processes:|Users logged in:|IPv[46] address for|Expanded Security Maintenance for Applications is not enabled|Strictly confined Kubernetes|ubuntu\.com/engage|just raised the bar|programs included with the Debian|exact distribution terms|/usr/share/doc/\*/copyright|ABSOLUTELY NO WARRANTY|permitted by applicable law|This system has been minimized|unminimize|Alpine Wiki|setup-alpine|You may change this message|Web console:|Activate the web console|cockpit\.socket|^[[:space:]]*Last login:'
w_has() { case " $w_parts " in *" $1 "*) return 0 ;; esac; return 1; }
w_bar() { n=$(( $1 * $2 / 100 )); c=$grn; [ "$1" -ge 80 ] && c=$amb; [ "$1" -ge 90 ] && c=$red; s="$c"; i=0; while [ $i -lt "$2" ]; do [ $i -eq $n ] && s="$s$trk"; s="$s━"; i=$((i+1)); done; printf '%s%s' "$s" "$R"; }
w_pct() { c=$txt; [ "$1" -ge 80 ] && c=$amb; [ "$1" -ge 90 ] && c=$red; printf '%s%3d%%%s' "$c" "$1" "$R"; }
w_logo() { o=; p=$1; i=0; while [ -n "$p" ]; do c=${p%"${p#?}"}; p=${p#?}; case $c in .) o="$o$R  " ;; T) if [ "$2" = 2 ] && [ $i -ge 1 ] && [ $i -le 3 ]; then o="$o$E[48;2;47;49;54m$E[38;2;92;95;103m• "; else o="$o$E[48;2;47;49;54m  "; fi ;; B) o="$o$E[48;2;35;36;40m  " ;; E) o="$o$E[48;2;242;240;234m  " ;; G) o="$o$E[48;2;61;214;140m  " ;; esac; i=$((i+1)); done; printf '%s%s' "$o" "$R"; }
w_banner() {
  txt="$E[38;2;216;220;226m"; dim="$E[38;2;125;133;144m"; lab="$E[38;2;95;103;117m"; trk="$E[38;2;43;48;56m"; grn="$E[38;2;61;214;140m"; amb="$E[38;2;229;184;79m"; red="$E[38;2;247;118;142m"
  w_cols=$(stty size 2>/dev/null | cut -d' ' -f2); case "$w_cols" in ''|*[!0-9]*) w_cols=${COLUMNS:-80} ;; esac
  os=$( . /etc/os-release 2>/dev/null; printf '%s' "${PRETTY_NAME:-$(uname -sr)}" ); arch=$(uname -m 2>/dev/null); cores=$(nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null)
  up=$(awk '{d=int($1/86400); h=int($1%86400/3600); n=int($1%3600/60); if (d) printf "%d day%s", d, (d==1 ? "" : "s"); else if (h) printf "%dh %dm", h, n; else printf "%d min", n}' /proc/uptime 2>/dev/null)
  load=$(cut -d' ' -f1-3 /proc/loadavg 2>/dev/null | sed 's/ /  /g')
  set -- $(awk '/^MemTotal:/ {t=$2} /^MemAvailable:/ {a=$2} END {if (t) printf "%d %.1f %.0f", (t-a)*100/t, (t-a)/1048576, t/1048576}' /proc/meminfo 2>/dev/null); mp=$1 mu=$2 mt=$3
  set -- $(df -Pk / 2>/dev/null | awk 'NR==2 && $2>0 {printf "%d %d %d", $3*100/$2, $3/1048576, $2/1048576}'); dp=$1 du=$2 dt=$3
  ct=; cu=; if w_has status && command -v docker >/dev/null 2>&1; then set -- $( { timeout 2 docker ps --format '{{.Status}}' 2>/dev/null || docker ps --format '{{.Status}}' 2>/dev/null; } | awk '{n++} /unhealthy/ {u++} END {if (n) printf "%d %d", n, u+0}'); ct=$1 cu=$2; fi
  upd=; sec=; f=/var/lib/update-notifier/updates-available; if [ -r "$f" ]; then upd=$(sed -n 's/^\([0-9][0-9]*\) updates* can be applied.*/\1/p' "$f" | head -n1); sec=$(sed -n 's/^\([0-9][0-9]*\) of these updates* .*security.*/\1/p' "$f" | head -n1); fi
  last=$(last -n 2 -i -w "$(id -un)" 2>/dev/null | awk 'NR==2 && NF>6 && $1!="wtmp" {printf "%s %s %s %s from %s", $4, $5, $6, $7, $3}')
  bw=22; [ "$w_cols" -lt 100 ] && bw=12
  i1=; i2=; i3=; i4=; i5=; i6=; i7=; i8=
  if w_has system; then i1="$txt$os$R  $dim·  $arch  ·  $cores cores$R"; i2="${dim}up $up  ·  load $load$R"; fi
  if w_has usage; then [ -n "$mp" ] && i4="${lab}mem $R  $(w_bar "$mp" $bw) $(w_pct "$mp")  $dim$mu / $mt GiB$R"; [ -n "$dp" ] && i5="${lab}disk$R  $(w_bar "$dp" $bw) $(w_pct "$dp")  $dim$du / $dt GB$R"; fi
  if w_has status; then s=; [ -n "$ct" ] && s="$grn▲$R $txt$ct containers$R" && [ "${cu:-0}" -gt 0 ] && s="$s$amb · $cu unhealthy$R"; if [ -n "$upd" ] && [ "$upd" -gt 0 ]; then [ -n "$s" ] && s="$s   "; s="$s$dim↑$R $txt$upd updates$R"; [ -n "$sec" ] && [ "$sec" -gt 0 ] && s="$s$red · $sec security$R"; fi; i7=$s; [ -n "$last" ] && i8="${dim}last login $last$R"; fi
  printf '\n'
  if [ "$w_cols" -lt 76 ]; then
    w_has logo && printf '  %s  %s\n  %s  %s\n' "$E[48;2;47;49;54m$E[38;2;92;95;103m •••  $R" "$i1" "$E[48;2;35;36;40m$E[38;2;242;240;234m$E[1m >_<  $R" "$i2" || { [ -n "$i1" ] && printf '  %s\n  %s\n' "$i1" "$i2"; }
    [ -n "$mp$dp" ] && w_has usage && printf '  %smem%s %s  %sdisk%s %s\n' "$lab" "$R" "$(w_pct "${mp:-0}")" "$lab" "$R" "$(w_pct "${dp:-0}")"
    [ -n "$i7" ] && printf '  %s\n' "$i7"
  elif w_has logo; then
    n=0; for row in .TTTTTTTTT. TTTTTTTTTTT BBBBBBBBBBB BEBBBBBBBEB BBEBBBBBEBB BEBBBBBBBEB BBBBGGGBBBB .BBBBBBBBB.; do n=$((n+1)); eval "info=\${i$n}"; printf '  %s    %s\n' "$(w_logo "$row" $n)" "$info"; done
  else
    for n in 1 2 3 4 5 6 7 8; do eval "info=\${i$n}"; [ -n "$info" ] && printf '  %s\n' "$info"; done
  fi
  w_notes=$(printf '%s\n' "$m" | grep -E "$w_notice"); if [ -n "$w_notes" ]; then printf '\n'; printf '%s\n' "$w_notes" | while IFS= read -r l; do printf '  %s%s%s\n' "$dim" "$l" "$R"; done; fi
  [ -n "$w_note" ] && printf '\n  %s\n' "$w_note"
  printf '\n'
}
w_own=$(printf '%s\n' "$m" | grep -Ev "$w_boiler" | grep -Ev "$w_notice" | grep -v '^[[:space:]]*$')
case "$w_mode" in off) ;; server) [ -n "$m" ] && printf '%s\n' "$m" ;; ssheesh) w_banner ;; *) if [ -n "$w_own" ]; then printf '%s\n' "$m"; else w_banner; fi ;; esac"#;

/// The script for this choice. Unknown or empty modes mean "auto"; no
/// parts means all of them.
pub fn script(w: &Welcome) -> String {
    let mode = match w.mode.as_str() {
        "ssheesh" | "server" | "off" => w.mode.as_str(),
        _ => "auto",
    };
    let parts = if w.parts.is_empty() { "logo system usage status".to_string() } else { w.parts.join(" ") };
    SCRIPT.replace("@MODE@", &shell_quote(mode)).replace("@PARTS@", &shell_quote(&parts)).replace("@NOTE@", &shell_quote(w.note.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn run(w: &Welcome) -> String {
        let home = std::env::temp_dir().join(format!("sshee-welcome-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let out = std::process::Command::new("sh").arg("-c").arg(script(w)).env("HOME", &home).env("COLUMNS", "120").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[cfg(unix)]
    #[test]
    fn ssheesh_welcome_shows_the_logo_and_facts() {
        let out = run(&Welcome { mode: "ssheesh".into(), parts: vec![], note: "be careful: production".into() });
        assert!(out.contains("cores"), "{out}");
        assert!(out.contains("\x1b[48;2;61;214;140m"), "the green mouth of the logo");
        assert!(out.contains("be careful: production"));
    }

    #[cfg(unix)]
    #[test]
    fn parts_can_be_left_out() {
        let out = run(&Welcome { mode: "ssheesh".into(), parts: vec!["system".into()], note: String::new() });
        assert!(out.contains("cores"));
        assert!(!out.contains("\x1b[48;2;61;214;140m"), "no logo");
        assert!(!out.contains("mem "), "no usage bars");
    }

    #[cfg(unix)]
    #[test]
    fn off_shows_nothing() {
        assert_eq!(run(&Welcome { mode: "off".into(), ..Default::default() }).trim(), "");
    }

    #[test]
    fn notes_are_quoted() {
        let s = script(&Welcome { mode: "ssheesh".into(), parts: vec![], note: "it's $(reboot)".into() });
        assert!(s.contains(r"w_note='it'\''s $(reboot)'"));
    }
}
