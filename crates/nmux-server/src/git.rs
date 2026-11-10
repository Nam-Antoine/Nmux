use std::collections::BTreeMap;
use std::process::Command;

use nmux_proto::{ChangedFile, GitChanges};

use crate::launch::find_on_path;
use crate::ports::quiet;
use crate::remote;

type Env = BTreeMap<String, String>;

const NUMSTAT_MARK: &str = "nmux-numstat";
const MAX_DIFF_BYTES: usize = 400 * 1024;

pub struct Repo<'a> {
    pub cwd: &'a str,
    pub target: Option<&'a str>,
    pub env: &'a Env,
}

impl Repo<'_> {
    fn local(&self, args: &[&str]) -> Result<std::process::Output, String> {
        let git = find_on_path("git", self.env).ok_or("git is not installed")?;
        let mut command = Command::new(git);
        command.arg("-C").arg(self.cwd).args(["-c", "core.quotepath=off"]).args(args).env_clear().envs(self.env);
        quiet(&mut command).output().map_err(|e| format!("could not run git: {e}"))
    }

    fn run(&self, commands: &[&[&str]]) -> Result<Vec<String>, String> {
        let outputs = match self.target {
            None => commands.iter().map(|args| self.local(args)).collect::<Result<Vec<_>, _>>()?,
            Some(target) => {
                let script = commands
                    .iter()
                    .map(|args| {
                        let words: Vec<String> = args.iter().map(|a| remote::sh_quote(a)).collect();
                        format!("git -c core.quotepath=off {}; printf '\\036'", words.join(" "))
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                let out = remote::output(target, &format!("{} || exit 1; {script}", remote::cd(self.cwd)), self.env)?;
                let text = String::from_utf8_lossy(&out.stdout).into_owned();
                let parts: Vec<String> = text.split('\u{1e}').map(str::to_string).collect();
                if parts.len() <= commands.len() {
                    return Err(remote::last_line(&out.stderr, "could not reach the folder on that machine"));
                }
                return Ok(parts.into_iter().take(commands.len()).collect());
            }
        };
        if let Some(first) = outputs.first() {
            if !first.status.success() && first.stdout.is_empty() {
                return Err(remote::last_line(&first.stderr, "git failed"));
            }
        }
        Ok(outputs.into_iter().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).collect())
    }

    pub fn changes(&self) -> Result<GitChanges, String> {
        let out = self.run(&[&["status", "--porcelain=v1", "--branch", "--untracked-files=all"], &["diff", "--numstat", "HEAD"]])?;
        if !out[0].starts_with("## ") {
            return Err("this folder is not a git repository".into());
        }
        Ok(read_changes(&format!("{}{NUMSTAT_MARK}\n{}", out[0], out[1])))
    }

    pub fn diff(&self, path: &str, untracked: bool) -> Result<(String, bool), String> {
        if path.is_empty() || path.len() > 1024 || path.starts_with(['-', '/', '\\']) || path.contains(':') || path.split(['/', '\\']).any(|p| p == "..") {
            return Err("not a path inside the repository".into());
        }
        let args: Vec<&str> = if untracked {
            vec!["diff", "--no-color", "--no-index", "--", "/dev/null", path]
        } else {
            vec!["diff", "--no-color", "HEAD", "--", path]
        };
        let text = self.run(&[&args])?.remove(0);
        if text.len() <= MAX_DIFF_BYTES {
            return Ok((text, false));
        }
        let mut end = MAX_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Ok((text[..end].to_string(), true))
    }
}

pub fn read_changes(text: &str) -> GitChanges {
    let (status, numstat) = text.split_once(&format!("{NUMSTAT_MARK}\n")).unwrap_or((text, ""));
    let mut changes = GitChanges { branch: None, ahead: 0, behind: 0, files: Vec::new() };
    for line in status.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            let (names, counts) = head.split_once(" [").unwrap_or((head, ""));
            let name = names.strip_prefix("No commits yet on ").unwrap_or(names);
            changes.branch = Some(name.split("...").next().unwrap_or(name).to_string());
            let count = |word: &str| counts.split(word).nth(1).and_then(|r| r.trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok());
            changes.ahead = count("ahead").unwrap_or(0);
            changes.behind = count("behind").unwrap_or(0);
        } else if line.len() > 3 {
            let code = &line[..2];
            let path = line[3..].rsplit(" -> ").next().unwrap_or(&line[3..]).trim_matches('"');
            let status = if code == "??" { "untracked" } else if code.contains('D') { "deleted" } else if code.contains('A') { "added" } else if code.contains('R') { "renamed" } else { "modified" };
            changes.files.push(ChangedFile { path: path.to_string(), status: status.to_string(), added: None, removed: None });
        }
    }
    for line in numstat.lines() {
        let mut cols = line.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (cols.next(), cols.next(), cols.next()) else { continue };
        if let Some(file) = changes.files.iter_mut().find(|f| f.path == path) {
            file.added = added.parse().ok();
            file.removed = removed.parse().ok();
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_and_line_counts_are_read_together() {
        let text = "## main...origin/main [ahead 2, behind 1]\n M src/a.rs\nA  new.txt\n?? notes/todo.md\nR  old.rs -> moved.rs\n D gone.rs\n\
                    nmux-numstat\n12\t3\tsrc/a.rs\n5\t0\tnew.txt\n-\t-\timage.png\n";
        let c = read_changes(text);
        assert_eq!((c.branch.as_deref(), c.ahead, c.behind), (Some("main"), 2, 1));
        let seen: Vec<(&str, &str, Option<u32>)> = c.files.iter().map(|f| (f.path.as_str(), f.status.as_str(), f.added)).collect();
        assert_eq!(
            seen,
            vec![
                ("src/a.rs", "modified", Some(12)),
                ("new.txt", "added", Some(5)),
                ("notes/todo.md", "untracked", None),
                ("moved.rs", "renamed", None),
                ("gone.rs", "deleted", None),
            ]
        );
    }

    #[test]
    fn a_new_repository_still_has_a_branch() {
        let c = read_changes("## No commits yet on trunk\n?? a\n");
        assert_eq!(c.branch.as_deref(), Some("trunk"));
        assert_eq!(c.files.len(), 1);
    }

    #[test]
    fn a_diff_path_cannot_leave_the_repository_or_become_an_option() {
        let env = Env::new();
        let repo = Repo { cwd: ".", target: None, env: &env };
        for bad in ["", "--output=x", "../x", "a/../../x", "/etc/passwd", "C:\\x"] {
            assert!(repo.diff(bad, false).is_err(), "{bad}");
        }
    }
}
