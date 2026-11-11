use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use nmux_core::validate;
use nmux_proto::SshHost;

const MAX_DEPTH: usize = 16;
const MAX_HOSTS: usize = 500;

fn home(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let find = |name: &str| env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| PathBuf::from(v));
    find("USERPROFILE").or_else(|| find("HOME"))
}

fn wildcard(pattern: &[char], name: &[char]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| wildcard(rest, &name[skip..])),
        Some((p, rest)) => name.split_first().is_some_and(|(c, tail)| (*p == '?' || p == c) && wildcard(rest, tail)),
    }
}

fn included(value: &str, home: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for raw in words(value) {
        let path = match raw.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None if Path::new(&raw).is_absolute() || raw.starts_with('/') => PathBuf::from(&raw),
            None => home.join(".ssh").join(&raw),
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.contains(['*', '?']) {
            files.push(path);
            continue;
        }
        let pattern: Vec<char> = name.chars().collect();
        let Some(entries) = path.parent().and_then(|dir| std::fs::read_dir(dir).ok()) else { continue };
        let mut matched: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| wildcard(&pattern, &e.file_name().to_string_lossy().chars().collect::<Vec<_>>()))
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        matched.sort();
        files.extend(matched);
    }
    files
}

fn words(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = value.trim();
    while !rest.is_empty() {
        let (word, tail) = match rest.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
            None => rest.split_once(char::is_whitespace).unwrap_or((rest, "")),
        };
        if !word.is_empty() {
            out.push(word.to_string());
        }
        rest = tail.trim_start();
    }
    out
}

fn read(file: &Path, home: &Path, hosts: &mut Vec<SshHost>, seen: &mut HashSet<String>, depth: usize) {
    let Ok(text) = std::fs::read_to_string(file) else { return };
    let mut current: Option<usize> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let split = line.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(line.len());
        let key = line[..split].to_ascii_lowercase();
        let value = line[split..].trim_start_matches(|c: char| c.is_whitespace() || c == '=').trim();
        match key.as_str() {
            "host" => {
                let names: Vec<String> = words(value)
                    .into_iter()
                    .filter(|n| !n.contains(['*', '?', '!']) && validate::ssh_target(n).is_ok() && !seen.contains(n))
                    .collect();
                seen.extend(names.iter().cloned());
                current = None;
                if let Some(alias) = names.into_iter().next() {
                    if hosts.len() < MAX_HOSTS {
                        hosts.push(SshHost { alias, hostname: None, user: None });
                        current = Some(hosts.len() - 1);
                    }
                }
            }
            "match" => current = None,
            "include" if depth < MAX_DEPTH => {
                for file in included(value, home) {
                    read(&file, home, hosts, seen, depth + 1);
                }
            }
            "hostname" | "user" => {
                let Some(host) = current.map(|i| &mut hosts[i]) else { continue };
                let slot = if key == "user" { &mut host.user } else { &mut host.hostname };
                if slot.is_none() {
                    *slot = words(value).into_iter().next();
                }
            }
            _ => {}
        }
    }
}

pub fn hosts(env: &BTreeMap<String, String>) -> Vec<SshHost> {
    let mut hosts = Vec::new();
    if let Some(home) = home(env) {
        read(&home.join(".ssh").join("config"), &home, &mut hosts, &mut HashSet::new(), 0);
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_hosts_are_listed_with_included_files() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        std::fs::create_dir_all(ssh.join("conf.d")).unwrap();
        std::fs::write(
            ssh.join("config"),
            "# mine\nInclude conf.d/*.conf \"~/extra config\"\n\
             Host vps box\n  HostName 10.0.0.7\n  User=can\n  User ignored\n\
             Host *.internal !bad\n  User nobody\n\
             Host vps\n  HostName twice\n\
             Host -oProxyCommand=x\n\
             Match host x\n  User nobody\n\
             host  bare\n",
        )
        .unwrap();
        std::fs::write(ssh.join("conf.d").join("a.conf"), "Host inc\n Hostname inc.example\n").unwrap();
        std::fs::write(ssh.join("conf.d").join("skip.txt"), "Host skipped\n").unwrap();
        std::fs::write(home.path().join("extra config"), "Host extra\n").unwrap();

        let env: BTreeMap<String, String> = [("UserProfile".to_string(), home.path().to_string_lossy().into_owned())].into();
        let host = |alias: &str, hostname: Option<&str>, user: Option<&str>| SshHost {
            alias: alias.into(),
            hostname: hostname.map(Into::into),
            user: user.map(Into::into),
        };
        assert_eq!(
            hosts(&env),
            vec![
                host("inc", Some("inc.example"), None),
                host("extra", None, None),
                host("vps", Some("10.0.0.7"), Some("can")),
                host("bare", None, None),
            ]
        );
        assert!(hosts(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn wildcards_match_file_names() {
        let m = |p: &str, n: &str| wildcard(&p.chars().collect::<Vec<_>>(), &n.chars().collect::<Vec<_>>());
        assert!(m("*.conf", "a.conf") && m("*", "x") && m("h?st*", "host-1"));
        assert!(!m("*.conf", "a.txt") && !m("h?st", "hst"));
    }
}
