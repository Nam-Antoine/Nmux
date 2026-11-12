use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use nmux_core::{MachineStatus, PaneId};
use nmux_proto::{DirEntry, FilePreview};

use crate::files;
use crate::launch::{find_on_path, Launch};

type Env = BTreeMap<String, String>;

const SKIP_PERMISSIONS_FLAG: &str = "--dangerously-skip-permissions";
const BACKGROUND_TIMEOUT_SECS: u32 = 12;

pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub enum Program<'a> {
    Shell,
    Claude {
        session_id: &'a str,
        resume: bool,
        extra_args: &'a [String],
        skip_permissions: bool,
    },
}

pub fn owner(env: &Env) -> String {
    let name = env.iter().find(|(k, _)| k.eq_ignore_ascii_case("USERNAME") || k.as_str() == "USER").map(|(_, v)| v.as_str()).unwrap_or("user");
    let clean: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect::<String>().to_lowercase();
    if clean.is_empty() { "user".into() } else { clean }
}

pub fn tmux_session(owner: &str, pane: PaneId, created_at_ms: u64) -> String {
    format!("nmux-{owner}-{}-{}", pane.0, created_at_ms / 1000 % 1_000_000)
}

fn place(path: &str) -> String {
    if path == "~" {
        "~".to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("~/{}", sh_quote(rest))
    } else {
        sh_quote(path)
    }
}

pub(crate) fn cd(cwd: &str) -> String {
    if cwd == "~" {
        "cd".to_string()
    } else {
        format!("cd {}", place(cwd))
    }
}

pub fn script(cwd: &str, session: &str, program: &Program) -> String {
    let run = match program {
        Program::Shell => None,
        Program::Claude { session_id, resume, extra_args, skip_permissions } => {
            let mut flags = String::new();
            if *skip_permissions {
                flags.push(' ');
                flags.push_str(SKIP_PERMISSIONS_FLAG);
            }
            for arg in extra_args.iter().filter(|a| !(*skip_permissions && *a == SKIP_PERMISSIONS_FLAG)) {
                flags.push(' ');
                flags.push_str(&sh_quote(arg));
            }
            let id = sh_quote(session_id);
            let claude = if *resume {
                format!("claude --resume {id}{flags} || claude{flags}")
            } else {
                format!("claude --session-id {id}{flags}")
            };
            Some(format!("${{SHELL:-sh}} -lic {}", sh_quote(&claude)))
        }
    };
    let in_tmux = run.as_deref().map(|r| format!(" {}", sh_quote(r))).unwrap_or_default();
    let plain = run.unwrap_or_else(|| "exec ${SHELL:-sh} -l".to_string());
    format!(
        "{cd} || exit 1; \
         if command -v tmux >/dev/null 2>&1; then \
         exec tmux -u new-session -A -s {session}{in_tmux} \
         \\; set-option status off \
         \\; set-option set-titles on \
         \\; set-option set-titles-string '#{{pane_title}}'; \
         else {plain}; fi",
        cd = cd(cwd),
        session = sh_quote(session),
    )
}

fn ssh_bin(env: &Env) -> String {
    find_on_path("ssh", env).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "ssh".into())
}

pub fn launch(target: &str, script: &str, env: &Env) -> Launch {
    Launch {
        file: ssh_bin(env),
        args: vec![
            "-t".into(),
            "-C".into(),
            "-o".into(),
            "ServerAliveInterval=30".into(),
            "-o".into(),
            "ServerAliveCountMax=4".into(),
            "--".into(),
            target.into(),
            format!("sh -c {}", sh_quote(script)),
        ],
    }
}

fn unattended(target: &str, remote_command: &str, env: &Env) -> Command {
    let mut command = Command::new(ssh_bin(env));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .args(["-o", "BatchMode=yes"])
        .args(["-o", &format!("ConnectTimeout={BACKGROUND_TIMEOUT_SECS}")])
        .arg("--")
        .arg(target)
        .arg(remote_command)
        .env_clear()
        .envs(env);
    command
}

pub(crate) fn last_line(stderr: &[u8], fallback: &str) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or(fallback).chars().take(300).collect()
}

#[derive(Debug, PartialEq, Eq)]
pub struct Probe {
    pub status: MachineStatus,
    pub detail: Option<String>,
    pub tmux: bool,
    pub claude: bool,
}

const PROBE: &str = "echo nmux-ok; command -v tmux >/dev/null 2>&1 && echo nmux-tmux; \
                     ${SHELL:-sh} -lic 'command -v claude' >/dev/null 2>&1 && echo nmux-claude; true";

pub fn read_probe(stdout: &str, stderr: &str) -> Probe {
    let has = |mark: &str| stdout.lines().any(|l| l.trim() == mark);
    if has("nmux-ok") {
        return Probe { status: MachineStatus::Online, detail: None, tmux: has("nmux-tmux"), claude: has("nmux-claude") };
    }
    let lower = stderr.to_lowercase();
    let auth = lower.contains("permission denied") || lower.contains("authentication") || lower.contains("passphrase");
    Probe {
        status: if auth { MachineStatus::Auth } else { MachineStatus::Error },
        detail: Some(last_line(stderr.as_bytes(), "ssh gave no reason")),
        tmux: false,
        claude: false,
    }
}

pub fn probe(target: &str, env: &Env) -> Probe {
    let command = format!("sh -c {}", sh_quote(PROBE));
    match unattended(target, &command, env).stdin(Stdio::null()).output() {
        Ok(out) => read_probe(&String::from_utf8_lossy(&out.stdout), &String::from_utf8_lossy(&out.stderr)),
        Err(err) => Probe {
            status: MachineStatus::Error,
            detail: Some(format!("could not run ssh: {err}")),
            tmux: false,
            claude: false,
        },
    }
}

fn process_name(line: &str) -> String {
    line.split("((\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap_or_default().to_string()
}

pub fn read_listeners(text: &str) -> Vec<(String, u16, String)> {
    let mut found: Vec<(String, u16, String)> = Vec::new();
    for line in text.lines() {
        let Some((address, port, _)) = crate::ports::parse_ss(line).into_iter().next() else { continue };
        if !found.iter().any(|f| f.1 == port) {
            found.push((address, port, process_name(line)));
        }
    }
    found.sort_by_key(|f| f.1);
    found
}

pub fn output(target: &str, script: &str, env: &Env) -> Result<std::process::Output, String> {
    let command = format!("sh -c {}", sh_quote(script));
    unattended(target, &command, env).stdin(Stdio::null()).output().map_err(|e| format!("could not run ssh: {e}"))
}

pub fn listening(target: &str, env: &Env) -> Result<Vec<(String, u16, String)>, String> {
    let out = output(target, "ss -Hltnp 2>/dev/null || netstat -ltn 2>/dev/null; echo nmux-ok", env)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !stdout.lines().any(|l| l.trim() == "nmux-ok") {
        return Err(last_line(&out.stderr, "ssh gave no reason"));
    }
    Ok(read_listeners(&stdout))
}

fn checked(target: &str, script: &str, env: &Env) -> Result<Vec<u8>, String> {
    let out = output(target, script, env)?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(last_line(&out.stderr, "ssh gave no reason"))
    }
}

pub fn read_listing(text: &str) -> (String, Vec<DirEntry>) {
    let mut lines = text.lines();
    let path = lines.next().unwrap_or("/").to_string();
    let entries = lines
        .filter(|line| !line.is_empty())
        .map(|line| DirEntry { name: line.trim_end_matches('/').to_string(), dir: line.ends_with('/'), size: 0 })
        .collect();
    (path, files::sorted(entries))
}

pub fn list_dir(target: &str, path: &str, env: &Env) -> Result<(String, Vec<DirEntry>), String> {
    let out = checked(target, &format!("{} || exit 1; pwd; ls -1ApL 2>/dev/null; true", cd(path)), env)?;
    Ok(read_listing(&String::from_utf8_lossy(&out)))
}

pub fn read_file(target: &str, path: &str, env: &Env) -> Result<FilePreview, String> {
    let (limit, whole) = files::wanted(Path::new(path));
    let fits = if whole { format!("[ \"$s\" -le {limit} ]") } else { "true".to_string() };
    let script = format!(
        "f={}; [ -f \"$f\" ] || {{ echo 'not a file' >&2; exit 1; }}; s=$(wc -c < \"$f\") || exit 1; echo $s; if {fits}; then head -c {limit} \"$f\"; fi",
        place(path)
    );
    let out = checked(target, &script, env)?;
    let split = out.iter().position(|b| *b == b'\n').ok_or("the machine sent no file size")?;
    let size: u64 = String::from_utf8_lossy(&out[..split]).trim().parse().map_err(|_| "the machine sent no file size")?;
    Ok(FilePreview { path: path.to_string(), size, content: files::content(Path::new(path), size, out[split + 1..].to_vec()) })
}

const EXISTS: &str = "[ -e \"$p\" ] || [ -L \"$p\" ]";
const MISSING: &str = "else echo 'no such file or folder' >&2; exit 1; fi";

fn named(path: &str) -> Result<(&str, &str), String> {
    let whole = path.trim_end_matches('/');
    match whole.rsplit_once('/') {
        Some((_, name)) if !matches!(name, "." | "..") => Ok((whole, name)),
        _ => Err("that folder cannot be changed".into()),
    }
}

fn fresh(path: &str) -> Result<String, String> {
    let (whole, name) = named(path)?;
    Ok(format!("p={}; if {EXISTS}; then printf '%s already exists\\n' {} >&2; exit 1; fi", place(whole), sh_quote(name)))
}

fn movable(path: &str, roots: &[String]) -> Result<String, String> {
    let roots: Vec<String> = roots.iter().map(|root| place(root)).collect();
    Ok(format!(
        "p={}; P=$(cd \"$p\" 2>/dev/null && pwd -P); \
         if [ -n \"$P\" ]; then for r in {}; do R=$(cd \"$r\" 2>/dev/null && pwd -P); \
         case \"$R/\" in \"$P\"/*) echo 'a workspace folder cannot be renamed or removed' >&2; exit 1;; esac; done; fi",
        place(named(path)?.0),
        roots.join(" "),
    ))
}

pub fn make_dir(target: &str, path: &str, env: &Env) -> Result<(), String> {
    checked(target, &format!("{}; mkdir \"$p\"", fresh(path)?), env).map(drop)
}

pub fn make_file(target: &str, path: &str, env: &Env) -> Result<(), String> {
    checked(target, &format!("{}; set -C; : > \"$p\"", fresh(path)?), env).map(drop)
}

pub fn rename(target: &str, from: &str, to: &str, roots: &[String], env: &Env) -> Result<(), String> {
    let script = format!("{}; t=\"$p\"; {}; if {EXISTS}; then mv \"$p\" \"$t\"; {MISSING}", fresh(to)?, movable(from, roots)?);
    checked(target, &script, env).map(drop)
}

const TRASH: &str = "\"$HOME/.nmux-trash\"";

pub fn remove(target: &str, path: &str, roots: &[String], env: &Env) -> Result<(), String> {
    let script = format!(
        "{}; if {EXISTS}; then find {TRASH} -mindepth 1 -maxdepth 1 -mtime +7 -exec rm -rf {{}} + 2>/dev/null; \
         t={TRASH}/$(date +%s)-$$; mkdir -p \"$t\" && printf '%s' \"$p\" > \"$t/from\" && mv \"$p\" \"$t/item\"; {MISSING}",
        movable(path, roots)?
    );
    checked(target, &script, env).map(drop)
}

pub fn restore(target: &str, path: &str, env: &Env) -> Result<(), String> {
    let script = format!(
        "{}; b=; for t in {TRASH}/*; do if [ \"$(cat \"$t/from\" 2>/dev/null)\" = \"$p\" ] && {{ [ -e \"$t/item\" ] || [ -L \"$t/item\" ]; }}; then b=$t; fi; done; \
         [ -n \"$b\" ] || {{ echo 'nothing to undo' >&2; exit 1; }}; mv \"$b/item\" \"$p\" && rm -rf \"$b\"",
        fresh(path)?
    );
    checked(target, &script, env).map(drop)
}

struct Transfer {
    child: Child,
    errors: std::thread::JoinHandle<Vec<u8>>,
}

impl Transfer {
    fn start(target: &str, script: &str, env: &Env, stdin: Stdio, stdout: Stdio) -> Result<Transfer, String> {
        let mut child = unattended(target, &format!("sh -c {}", sh_quote(script)), env)
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run ssh: {e}"))?;
        let mut stderr = child.stderr.take();
        let errors = std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(stderr) = &mut stderr {
                let _ = stderr.read_to_end(&mut text);
            }
            text
        });
        Ok(Transfer { child, errors })
    }

    fn finish(mut self, local: std::io::Result<()>) -> Result<(), String> {
        let status = self.child.wait().map_err(|e| e.to_string())?;
        let far = (!status.success()).then(|| last_line(&self.errors.join().unwrap_or_default(), "ssh gave no reason"));
        match (local, far) {
            (Err(e), far) if far.is_none() || !matches!(e.kind(), ErrorKind::BrokenPipe | ErrorKind::UnexpectedEof) => Err(e.to_string()),
            (_, Some(far)) => Err(far),
            (_, None) => Ok(()),
        }
    }
}

fn pack(stream: impl Write, found: &[(PathBuf, String)]) -> std::io::Result<()> {
    let mut archive = tar::Builder::new(stream);
    for (path, name) in found {
        if path.is_dir() {
            archive.append_dir_all(name, path)?;
        } else {
            archive.append_path_with_name(path, name)?;
        }
    }
    archive.into_inner().map(drop)
}

fn unpack(stream: impl Read, dir: &Path) -> std::io::Result<()> {
    for entry in tar::Archive::new(stream).entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if kind.is_file() || kind.is_dir() || kind.is_hard_link() {
            entry.unpack_in(dir)?;
        }
    }
    Ok(())
}

const STAGED: &str = "nmux-staged";
const SETTLE: &str = "find . -type d -exec sh -c 'for f; do mkdir -p \"../$f\" || exit 1; done' sh {} + && \
                      find . ! -type d -exec sh -c 'for f; do \
                      if [ -d \"../$f\" ]; then echo \"${f#./} is a folder there\" >&2; exit 1; fi; \
                      mv -f \"$f\" \"../$f\" || exit 1; done' sh {} +";

pub fn upload_paths(target: &str, sources: &[String], to: &str, overwrite: bool, env: &Env) -> Result<(), String> {
    let found = files::upload_names(sources)?;
    let check = if overwrite {
        String::new()
    } else {
        let names: Vec<String> = found.iter().map(|(_, name)| sh_quote(&format!("./{name}"))).collect();
        format!(
            "t=; for p in {}; do if {EXISTS}; then t=\"$t${{t:+, }}${{p#./}}\"; fi; done; \
             if [ -n \"$t\" ]; then printf 'already exists: %s\\n' \"$t\" >&2; exit 1; fi; ",
            names.join(" ")
        )
    };
    let stage = format!("./.nmux-upload-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let receive = format!("{} || exit 1; {check}mkdir {stage} || exit 1; echo {STAGED}; tar -xof - -C {stage}", cd(to));
    let mut transfer = Transfer::start(target, &receive, env, Stdio::piped(), Stdio::piped())?;
    let sent = pack(transfer.child.stdin.take().ok_or("ssh has no input")?, &found);
    let mut said = String::new();
    if let Some(stdout) = &mut transfer.child.stdout {
        let _ = stdout.read_to_string(&mut said);
    }
    let sent = transfer.finish(sent);
    if !said.contains(STAGED) {
        return sent;
    }
    let place = if sent.is_ok() { SETTLE } else { "false" };
    let settled = checked(target, &format!("{} || exit 1; cd {stage} || exit 1; {place}; s=$?; cd .. && rm -rf {stage}; exit $s", cd(to)), env);
    sent.and(settled.map(drop))
}

pub fn download(target: &str, path: &str, downloads: &Path, env: &Env) -> Result<PathBuf, String> {
    let (whole, name) = named(path)?;
    files::plain_name(name)?;
    let parent = &whole[..whole.len() - name.len() - 1];
    let script = format!(
        "{} || exit 1; p={}; if {EXISTS}; then h=; if [ -L \"$p\" ] && [ -f \"$p\" ]; then h=h; fi; tar -c${{h}}f - \"$p\"; {MISSING}",
        cd(if parent.is_empty() { "/" } else { parent }),
        sh_quote(&format!("./{name}")),
    );
    let staging = downloads.join(format!(".nmux-download-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let landed = (|| {
        let mut transfer = Transfer::start(target, &script, env, Stdio::null(), Stdio::piped())?;
        let received = unpack(transfer.child.stdout.take().ok_or("ssh has no output")?, &staging);
        transfer.finish(received)?;
        if staging.join(name).symlink_metadata().is_err() {
            return Err("only files and folders can be downloaded".into());
        }
        let landed = files::free_name(downloads, name);
        std::fs::rename(staging.join(name), &landed).map_err(|e| e.to_string())?;
        Ok(landed)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    landed
}

pub fn end_sessions(target: String, sessions: Vec<String>, env: Env) {
    if sessions.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let kills: Vec<String> =
            sessions.iter().map(|s| format!("tmux kill-session -t {} 2>/dev/null", sh_quote(&format!("={s}")))).collect();
        let command = format!("sh -c {}", sh_quote(&format!("{}; true", kills.join("; "))));
        let _ = unattended(&target, &command, &env).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

pub fn upload(target: &str, name: &str, bytes: &[u8], env: &Env) -> Result<String, String> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return Err("bad file name".into());
    }
    let path = format!("/tmp/nmux-{name}");
    let command = format!("sh -c {}", sh_quote(&format!("umask 077; cat > {}", sh_quote(&path))));
    let mut child = unattended(target, &command, env)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run ssh: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(bytes).map_err(|e| format!("could not send the image: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("could not copy the image to the machine: {}", last_line(&out.stderr, "ssh failed")));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_carry_the_name_of_who_started_them() {
        let env: Env = [("USERNAME".to_string(), "Nam Tran!".to_string())].into();
        assert_eq!(owner(&env), "namtran");
        assert_eq!(owner(&Env::new()), "user");
        assert_eq!(tmux_session("namtran", PaneId(7), 1_794_307_748_733), "nmux-namtran-7-307748");
    }

    #[test]
    fn quoting_survives_quotes_and_spaces() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("it's a dir"), "'it'\\''s a dir'");
    }

    #[test]
    fn home_relative_folders_keep_the_tilde_unquoted() {
        assert_eq!(cd("~"), "cd");
        assert_eq!(cd("~/my proj"), "cd ~/'my proj'");
        assert_eq!(cd("/srv/app"), "cd '/srv/app'");
        let (path, entries) = read_listing("/home/can\nnotes.txt\nsrc/\n.ssh/\nApp\n");
        assert_eq!(path, "/home/can");
        let seen: Vec<(&str, bool)> = entries.iter().map(|e| (e.name.as_str(), e.dir)).collect();
        assert_eq!(seen, [(".ssh", true), ("src", true), ("App", false), ("notes.txt", false)]);
    }

    #[test]
    fn roots_and_dot_folders_are_never_changed() {
        let env = Env::new();
        for path in ["/", "~", "", "~/", "//", "/srv/.", "/srv/..", "~/..", "/srv/../", "relative"] {
            assert!(named(path).is_err(), "{path}");
            assert!(remove("-oProxyCommand=evil", path, &[], &env).is_err(), "{path}");
            assert!(rename("-oProxyCommand=evil", path, "/tmp/x", &[], &env).is_err(), "{path}");
            assert!(download("-oProxyCommand=evil", path, Path::new("unused"), &env).is_err(), "{path}");
        }
        assert_eq!(named("/srv/my app/").unwrap(), ("/srv/my app", "my app"));
        assert_eq!(named("~/a").unwrap(), ("~/a", "a"));
    }

    #[test]
    fn file_scripts_quote_every_path() {
        let s = fresh("~/it's here/new dir").unwrap();
        assert!(s.starts_with("p=~/'it'\\''s here/new dir'; if [ -e \"$p\" ]"), "{s}");
        assert!(s.contains("printf '%s already exists\\n' 'new dir' >&2"), "{s}");
        let s = movable("/srv/a b", &["~/my proj".to_string(), "/srv".to_string()]).unwrap();
        assert!(s.starts_with("p='/srv/a b'; "), "{s}");
        assert!(s.contains("for r in ~/'my proj' '/srv'; do"), "{s}");
    }

    #[test]
    #[cfg(windows)]
    fn a_download_only_lands_under_one_plain_name() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(downloads.join("notes.txt"), "mine").unwrap();
        for path in ["/srv/..\\notes.txt", "/srv/..\\..\\notes.txt", "/srv/a\\b", "/srv/C:notes.txt", "/srv/report "] {
            let err = download("-oProxyCommand=evil", path, &downloads, &Env::new()).unwrap_err();
            assert_eq!(err, "that name will not work on this computer", "{path}");
        }
        assert_eq!(std::fs::read_to_string(downloads.join("notes.txt")).unwrap(), "mine");
        assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 1);
    }

    #[test]
    #[ignore = "needs NMUX_TEST_SSH set to an ssh target; it works under /tmp/nmux-filetest-*"]
    fn file_ops_on_a_real_machine() {
        let target = std::env::var("NMUX_TEST_SSH").unwrap();
        let env: Env = std::env::vars().collect();
        let local = tempfile::tempdir().unwrap();
        let base = format!("/tmp/nmux-filetest-{}", std::process::id());
        let at = |rest: &str| format!("{base}/{rest}");
        let names = |path: &str| list_dir(&target, path, &env).unwrap().1.into_iter().map(|e| e.name).collect::<Vec<_>>();
        let refused = |what: &str, result: Result<(), String>, why: &str| {
            let err = result.expect_err(what);
            assert!(err.contains(why), "{what}: {err}");
            println!("{what}: refused, {err}");
        };

        let folder = local.path().join("my folder");
        std::fs::create_dir_all(folder.join("it's deep/empty")).unwrap();
        std::fs::write(folder.join("it's deep/\u{e9}t\u{e9} 'q'.txt"), "nested").unwrap();
        let big: Vec<u8> = (0..3_000_000u32).map(|n| (n % 251) as u8).collect();
        std::fs::write(local.path().join("big file.bin"), &big).unwrap();
        let sources = vec![folder.to_string_lossy().into_owned(), local.path().join("big file.bin").to_string_lossy().into_owned()];
        let clash = local.path().join("clash/it's a dir");
        std::fs::create_dir_all(clash.parent().unwrap()).unwrap();
        std::fs::write(&clash, "a file named like a folder there").unwrap();

        for path in ["/", "~", "", "~/", "/tmp/.", "/tmp/.."] {
            refused(&format!("remove {path:?}"), remove(&target, path, &[], &env), "that folder cannot be changed");
            refused(&format!("rename {path:?}"), rename(&target, path, &at("x"), &[], &env), "that folder cannot be changed");
        }

        make_dir(&target, &base, &env).unwrap();
        make_dir(&target, &at("it's a dir"), &env).unwrap();
        refused("make dir again", make_dir(&target, &at("it's a dir"), &env), "it's a dir already exists");
        make_file(&target, &at("it's a dir/new \"file\".txt"), &env).unwrap();
        refused("make file again", make_file(&target, &at("it's a dir/new \"file\".txt"), &env), "new \"file\".txt already exists");

        upload_paths(&target, &sources, &base, false, &env).unwrap();
        assert_eq!(upload_paths(&target, &sources, &base, false, &env).unwrap_err(), "already exists: my folder, big file.bin");
        std::fs::write(folder.join("added.txt"), "added").unwrap();
        upload_paths(&target, &sources, &base, true, &env).unwrap();
        refused("upload a file over a folder", upload_paths(&target, &[clash.to_string_lossy().into_owned()], &base, true, &env), "it's a dir is a folder there");
        assert_eq!(names(&base), ["it's a dir", "my folder", "big file.bin"], "nothing is left behind by an upload");
        assert_eq!(names(&at("my folder")), ["it's deep", "added.txt"]);
        assert_eq!(names(&at("my folder/it's deep")), ["empty", "\u{e9}t\u{e9} 'q'.txt"]);
        println!("after upload: {:?}", names(&base));

        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::write(folder.join("added.txt"), "changed").unwrap();
            std::fs::write(folder.join("zz locked.txt"), "locked").unwrap();
            let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(folder.join("zz locked.txt")).unwrap();
            let err = upload_paths(&target, &sources, &base, true, &env).unwrap_err();
            println!("upload cut short: refused, {err}");
            drop(held);
            let kept = read_file(&target, &at("my folder/added.txt"), &env).unwrap().content;
            assert_eq!(kept, nmux_proto::FileContent::Text { text: "added".into(), truncated: false }, "a cut upload changes nothing");
            assert_eq!(names(&base), ["it's a dir", "my folder", "big file.bin"]);
            assert_eq!(names(&at("my folder")), ["it's deep", "added.txt"]);
        }

        rename(&target, &at("my folder"), &at("it's a dir/moved folder"), &[], &env).unwrap();
        refused("rename onto a taken name", rename(&target, &at("big file.bin"), &at("it's a dir"), &[], &env), "it's a dir already exists");
        refused("rename a missing one", rename(&target, &at("nope"), &at("nope2"), &[], &env), "no such file or folder");
        refused("rename a workspace", rename(&target, &at("it's a dir"), &at("other"), &[at("it's a dir/moved folder")], &env), "workspace folder");
        refused("remove a workspace", remove(&target, &at("it's a dir/"), &[at("it's a dir")], &env), "workspace folder");
        assert_eq!(names(&base), ["it's a dir", "big file.bin"]);
        assert_eq!(names(&at("it's a dir")), ["moved folder", "new \"file\".txt"]);

        make_file(&target, &at("report"), &env).unwrap();
        make_file(&target, &at("report "), &env).unwrap();
        remove(&target, &at("report "), &[], &env).unwrap();
        assert_eq!(names(&base), ["it's a dir", "big file.bin", "report"], "only the name with the space at its end is gone");
        remove(&target, &at("report"), &[], &env).unwrap();

        checked(&target, &format!("{} && ln -s 'big file.bin' link.bin && ln -s \"it's a dir\" linkdir", cd(&base)), &env).unwrap();
        let downloads = local.path().join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        for landed in [["big file.bin", "moved folder"], ["big file (1).bin", "moved folder (1)"]] {
            let file = download(&target, &at("big file.bin"), &downloads, &env).unwrap();
            assert_eq!(file, downloads.join(landed[0]));
            assert!(std::fs::read(&file).unwrap() == big, "the bytes that came back are the bytes that went");
            let dir = download(&target, &at("it's a dir/moved folder"), &downloads, &env).unwrap();
            assert_eq!(dir, downloads.join(landed[1]));
            assert_eq!(std::fs::read_to_string(dir.join("it's deep/\u{e9}t\u{e9} 'q'.txt")).unwrap(), "nested");
            assert!(dir.join("it's deep/empty").is_dir() && dir.join("added.txt").is_file());
            println!("downloaded: {landed:?}");
        }
        let linked = download(&target, &at("link.bin"), &downloads, &env).unwrap();
        assert!(std::fs::read(&linked).unwrap() == big, "a link to a file comes back as the file");
        refused("download a link to a folder", download(&target, &at("linkdir"), &downloads, &env).map(drop), "only files and folders can be downloaded");
        refused("download a missing one", download(&target, &at("nope"), &downloads, &env).map(drop), "no such file or folder");
        assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 5, "no staging folder is left in Downloads");

        remove(&target, &at("big file.bin"), &[], &env).unwrap();
        remove(&target, &at("it's a dir"), &[], &env).unwrap();
        assert_eq!(names(&base), ["link.bin", "linkdir"]);
        restore(&target, &at("it's a dir"), &env).unwrap();
        restore(&target, &at("big file.bin"), &env).unwrap();
        assert!(["it's a dir", "big file.bin"].iter().all(|name| names(&base).contains(&name.to_string())), "a delete can be undone");
        assert!(names(&at("it's a dir")).contains(&"moved folder".to_string()), "with what was inside");
        refused("undo onto a taken name", restore(&target, &at("big file.bin"), &env), "already exists");
        refused("undo what was never deleted", restore(&target, &at("nope"), &env), "nothing to undo");
        remove(&target, &at("big file.bin"), &[], &env).unwrap();
        remove(&target, &at("it's a dir"), &[], &env).unwrap();
        refused("remove a missing one", remove(&target, &at("nope"), &[], &env), "no such file or folder");
        assert_eq!(names(&base), ["link.bin", "linkdir"]);
        remove(&target, &base, &[], &env).unwrap();
        assert!(list_dir(&target, &base, &env).is_err());
        println!("cleaned up {base}");
    }

    #[test]
    fn a_shell_pane_uses_tmux_when_it_is_there() {
        let s = script("/srv/app", "nmux-7-123", &Program::Shell);
        assert!(s.starts_with("cd '/srv/app' || exit 1; "), "{s}");
        assert!(s.contains("exec tmux -u new-session -A -s 'nmux-7-123' \\; set-option status off"), "{s}");
        assert!(s.ends_with("else exec ${SHELL:-sh} -l; fi"), "{s}");
    }

    #[test]
    fn a_claude_pane_starts_then_resumes_its_own_conversation() {
        let extra = vec!["--model".to_string(), "it's".to_string(), SKIP_PERMISSIONS_FLAG.to_string()];
        let start = Program::Claude { session_id: "uuid-1", resume: false, extra_args: &extra, skip_permissions: true };
        let s = script("~/proj", "nmux-9-1", &start);
        assert!(s.contains("else ${SHELL:-sh} -lic 'claude --session-id '\\''uuid-1'\\'' --dangerously-skip-permissions "), "{s}");
        assert_eq!(s.matches(SKIP_PERMISSIONS_FLAG).count(), 2, "once per branch, not repeated from extra args: {s}");

        let resume = Program::Claude { session_id: "uuid-1", resume: true, extra_args: &[], skip_permissions: false };
        let s = script("~/proj", "nmux-9-1", &resume);
        assert!(s.contains("claude --resume '\\''uuid-1'\\'' || claude'"), "{s}");
    }

    #[test]
    fn the_target_cannot_become_an_option() {
        let l = launch("-oProxyCommand=evil", "true", &Env::new());
        let dashes = l.args.iter().position(|a| a == "--").unwrap();
        assert_eq!(l.args[dashes + 1], "-oProxyCommand=evil");
        assert_eq!(l.args.last().unwrap(), "sh -c 'true'");
    }

    #[test]
    fn remote_listeners_are_read_from_ss_and_netstat() {
        let ss = "LISTEN 0 511 127.0.0.1:5173 0.0.0.0:* users:((\"node\",pid=1234,fd=20))\n\
                  LISTEN 0 128 0.0.0.0:22 0.0.0.0:*\n\
                  LISTEN 0 128 [::]:22 [::]:*\nnmux-ok\n";
        assert_eq!(
            read_listeners(ss),
            vec![("0.0.0.0".to_string(), 22, String::new()), ("127.0.0.1".to_string(), 5173, "node".to_string())]
        );
        let netstat = "Active Internet connections (only servers)\n\
                       Proto Recv-Q Send-Q Local Address           Foreign Address         State\n\
                       tcp        0      0 0.0.0.0:8080            0.0.0.0:*               LISTEN\n";
        assert_eq!(read_listeners(netstat), vec![("0.0.0.0".to_string(), 8080, String::new())]);
    }

    #[test]
    fn probe_results_are_read_from_markers() {
        let p = read_probe("motd line\nnmux-ok\nnmux-tmux\n", "");
        assert_eq!(p, Probe { status: MachineStatus::Online, detail: None, tmux: true, claude: false });
        let p = read_probe("", "Warning: x\nuser@host: Permission denied (publickey,password).\n");
        assert_eq!(p.status, MachineStatus::Auth);
        assert_eq!(p.detail.as_deref(), Some("user@host: Permission denied (publickey,password)."));
        let p = read_probe("", "ssh: Could not resolve hostname nope: No such host is known.\n");
        assert_eq!(p.status, MachineStatus::Error);
    }
}
