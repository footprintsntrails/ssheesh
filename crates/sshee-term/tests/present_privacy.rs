//! 0.10's `src/lib/privacy.test.ts` (`maskText`), one test for one test,
//! same inputs and expected results. The servers are made up.

use sshee_term::privacy::{HostNames, Names};

/// The saved servers of 0.10's test: "prod-api" at 203.0.113.10 as deploy,
/// and "homelab" at nas.home.example.net as root.
fn names(learned: &[&str]) -> Names {
    let hosts = [
        HostNames { name: "prod-api", hostname: "203.0.113.10", user: Some("deploy") },
        HostNames { name: "homelab", hostname: "nas.home.example.net", user: Some("root") },
    ];
    Names::new(hosts, learned.iter().copied())
}

fn mask(text: &str) -> String {
    names(&[]).mask_text(text)
}

#[test]
fn hides_server_names_users_and_addresses_without_moving_columns() {
    let line = "deploy@prod-api:~$ ping 203.0.113.10";
    let out = mask(line);
    assert_eq!(out, "••••••@••••••••:~$ ping ••••••••••••");
    assert_eq!(out.chars().count(), line.chars().count());
}

#[test]
fn hides_a_full_hostname_and_its_short_form() {
    assert_eq!(mask("ssh nas.home.example.net"), "ssh ••••••••••••••••••••");
    assert_eq!(mask("nas login:"), "••• login:");
}

#[test]
fn leaves_names_everybody_has_alone() {
    assert!(mask("root@box:/root#").contains("root@"));
}

#[test]
fn hides_ipv6_addresses_but_not_clock_times() {
    assert_eq!(mask("from 2001:db8::1 at 12:34:56"), "from ••••:•••::• at 12:34:56");
    assert!(!mask("fe80::1c2d:3e4f:5a6b:7c8d").chars().any(|c| c.is_ascii_hexdigit()));
}

#[test]
fn leaves_terminal_color_codes_intact() {
    let sgr = "\x1b[38:2::255:0:0mred\x1b[0m";
    assert_eq!(mask(sgr), sgr);
}

#[test]
fn hides_names_learned_at_run_time() {
    assert_eq!(names(&["web-7731.example.com"]).mask_text("user@web-7731:~$"), "user@••••••••:~$");
}

// Beyond 0.10's tests: the hand-written address rules.

#[test]
fn an_address_inside_a_longer_number_or_word_stays() {
    // Version numbers and parts of longer dotted runs aren't addresses.
    assert_eq!(mask("v1.2.3.4567 and 1.2.3.4.5"), "v1.2.3.4567 and 1.2.3.4.5");
    assert_eq!(mask("at 198.51.100.7:22, ok"), "at •••.••.•••.•:22, ok");
}

#[test]
fn every_character_stays_or_becomes_a_dot() {
    let n = names(&["db-main"]);
    for line in ["db-main 203.0.113.10 fe80::1 Ünïcødé", "", "•••", "prod-api服务器"] {
        let out = n.mask_text(line);
        assert_eq!(out.chars().count(), line.chars().count(), "{line}");
        assert!(line.chars().zip(out.chars()).all(|(a, b)| a == b || b == '•'), "{line} -> {out}");
    }
}

#[test]
fn a_long_server_list_is_hidden_whole_and_doesnt_stop_the_app() {
    // Thousands of made-up hosts, each a name, an address, its short form and a user:
    // more than one regex may hold (0.10's JavaScript had no such limit).
    let names: Vec<(String, String, String)> = (0..4000)
        .map(|i| (format!("web-node-{i:05}-prod"), format!("web-node-{i:05}.internal.example.net"), format!("svc{i:05}")))
        .collect();
    let hosts = names.iter().map(|(n, h, u)| HostNames { name: n, hostname: h, user: Some(u) });
    let n = Names::new(hosts, ["сервер-база"]);
    assert_eq!(n.words().len(), 4000 * 4 + 1);
    assert_eq!(n.mask_text("ssh svc03999@WEB-NODE-03999.internal.example.net"), format!("ssh {}@{}", "•".repeat(8), "•".repeat(35)));
    assert_eq!(n.mask_text("on web-node-00000: ok"), "on ••••••••••••••: ok");
    // A name in other letters, in any case.
    assert_eq!(n.mask_text("СЕРВЕР-БАЗА up"), "••••••••••• up");
}

#[test]
fn the_longest_name_at_a_place_goes_whole() {
    let n = Names::new([HostNames { name: "db-main", hostname: "db-main.example.net", user: Some("deploy") }], []);
    assert_eq!(n.mask_text("DB-MAIN.EXAMPLE.NET and db-main"), "••••••••••••••••••• and •••••••");
}
