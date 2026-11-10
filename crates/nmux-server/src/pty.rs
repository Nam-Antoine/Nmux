use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::time::Duration;

use anyhow::Context;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::launch::Launch;

pub struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    input: std::sync::mpsc::Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pub pid: Option<u32>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

const DRAIN: Duration = Duration::from_millis(150);

impl PtyProcess {
    pub fn spawn(
        launch: &Launch,
        cwd: &str,
        env: &BTreeMap<String, String>,
        cols: u16,
        rows: u16,
        mut on_data: impl FnMut(&[u8]) + Send + 'static,
        on_exit: impl FnOnce(Option<i32>) + Send + 'static,
    ) -> anyhow::Result<PtyProcess> {
        let pair = native_pty_system().openpty(size(cols, rows)).context("could not open a pseudo-terminal")?;

        let mut cmd = CommandBuilder::new(&launch.file);
        cmd.args(&launch.args);
        cmd.cwd(cwd);
        cmd.env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child =
            pair.slave.spawn_command(cmd).with_context(|| format!("could not start {}", launch.file))?;
        drop(pair.slave);

        let pid = child.process_id();
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader().context("could not read from the pseudo-terminal")?;
        let mut writer = pair.master.take_writer().context("could not write to the pseudo-terminal")?;
        let (input, pending) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::Builder::new().name("nmux-pty-write".into()).spawn(move || {
            for data in pending {
                if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        })?;

        std::thread::Builder::new().name("nmux-pty-read".into()).spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => on_data(&buf[..n]),
                }
            }
        })?;

        std::thread::Builder::new().name("nmux-pty-wait".into()).spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code() as i32);
            std::thread::sleep(DRAIN);
            on_exit(code);
        })?;

        Ok(PtyProcess { master: pair.master, input, killer, pid })
    }

    pub fn write(&self, data: &[u8]) {
        let _ = self.input.send(data.to_vec());
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(size(cols, rows));
    }

    pub fn kill(&mut self) {
        let _ = self.killer.kill();
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        self.kill();
    }
}
