//! ぱち簿's records rescued by hand: `rescue <backup folder> <out .pcb>`,
//! with the password in `BACKUP_PASSWORD` where the backup is encrypted.

use std::path::Path;

use ios_backup::{discovery, manifest::Backup};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (Some(folder), Some(out)) = (arguments.first(), arguments.get(1)) else {
        eprintln!("usage: rescue <backup folder> <out .pcb>");
        return Ok(());
    };
    let password = std::env::var("BACKUP_PASSWORD").ok();
    for root in discovery::backups_in(Path::new(folder))? {
        let rescued = pachibo::rescue(&Backup::open(&root, password.as_deref())?)?;
        std::fs::write(out, &rescued.pcb)?;
        println!("{} tallies written to {out}", rescued.tallies);
    }
    Ok(())
}
