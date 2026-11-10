//! Send one request to a running nmux-server and print the reply.
//!
//!   nmux-ctl '{"op":"getState"}'
//!   nmux-ctl --data-dir D '{"op":"createTab","workspace":3,"pane":{"kind":"shell"}}'

use std::path::PathBuf;

use nmux_proto::Op;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let mut data_dir: Option<PathBuf> = None;
    let mut request: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            _ => request = Some(arg),
        }
    }
    let data_dir = data_dir
        .or_else(nmux_server::default_data_dir)
        .ok_or_else(|| anyhow::anyhow!("cannot find a data directory; pass --data-dir"))?;
    let request = request.ok_or_else(|| anyhow::anyhow!("usage: nmux-ctl [--data-dir DIR] '<json request>'"))?;
    let op: Op = serde_json::from_str(&request)?;
    if matches!(op, Op::Hello { .. }) {
        anyhow::bail!("hello is sent for you");
    }

    let (client, _events) = nmux_server::client::connect(&data_dir).await?;
    match client.call(op).await {
        Ok(reply) => {
            println!("{}", serde_json::to_string_pretty(&reply)?);
            Ok(())
        }
        Err(why) => anyhow::bail!(why),
    }
}
