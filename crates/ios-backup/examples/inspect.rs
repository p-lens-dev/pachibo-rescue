//! Looking into a backup by hand: `inspect <backup folder> [domain] [out folder]`,
//! with the password in `BACKUP_PASSWORD` where it is encrypted.
//! With a folder alone it says what the backup is and lists its apps' domains;
//! with a domain it lists that domain's paths; with an out folder too, it writes them there.

use std::path::Path;

use ios_backup::{discovery, extract, manifest::Backup, summary};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(folder) = arguments.first() else {
        eprintln!("usage: inspect <backup folder> [domain] [out folder]");
        return Ok(());
    };
    let password = std::env::var("BACKUP_PASSWORD").ok();
    for root in discovery::backups_in(Path::new(folder))? {
        println!("{:#?}", summary::summarize(&root)?);
        let backup = Backup::open(&root, password.as_deref())?;
        match (arguments.get(1), arguments.get(2)) {
            (None, _) => backup.app_domains()?.iter().for_each(|domain| println!("{domain}")),
            (Some(domain), None) => backup.entries(domain)?.iter().for_each(|entry| println!("{:?}\t{:?}\t{}", entry.kind, entry.size, entry.relative_path)),
            (Some(domain), Some(out)) => println!("{:?}", extract::extract_domain(&backup, domain, Path::new(out))?),
        }
    }
    Ok(())
}
