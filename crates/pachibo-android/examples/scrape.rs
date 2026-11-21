//! ぱち簿's records read off a phone over adb: `scrape <out.csv> [--shallow]`.
//!
//! The phone needs USB debugging on and this computer allowed. `adb` is found
//! on the PATH, or at `$ADB` where it is set; `$ANDROID_SERIAL` picks one phone
//! of several. `--shallow` reads only the day, parlor, machine and result (no
//! opening of each record for 投資/回収/メモ/タグ), which is quicker.

use std::io::Write as _;

use pachibo_android::adb::Adb;
use pachibo_android::{End, Walk, csv_of, rescue, survey};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(out) = arguments.first() else {
        eprintln!("usage: scrape <out.csv> [--shallow]");
        return Ok(());
    };
    let deep = !arguments.iter().any(|argument| argument == "--shallow");

    let program = std::env::var("ADB").unwrap_or_else(|_| "adb".to_owned());
    // Every adb run and every note of the walk goes to standard error.
    let mut phone = Adb::new(program, std::env::var("ANDROID_SERIAL").ok()).log_to(std::io::stderr()).on_wait(|waiting| eprintln!("waiting: {waiting:?}"));

    let expected = survey(&mut phone, || false)?.records;
    match expected {
        Some(count) => println!("{count} records on the phone"),
        None => println!("the number of records could not be read"),
    }

    let walk = Walk { deep, expected, start: Some(this_month()), ..Walk::default() };
    let outcome = rescue(
        &mut phone,
        walk,
        |progress| {
            print!("\r{:04}-{:02}  {} records so far   ", progress.year, progress.month, progress.records);
            let _ = std::io::stdout().flush();
        },
        || false,
    );
    println!();

    std::fs::write(out, csv_of(&outcome.records))?;
    println!("{} records written to {out}", outcome.records.len());
    for (year, month) in &outcome.unsure_months {
        println!("{year:04}-{month:02}: the results read do not add up to the month's total");
    }
    if let End::Failed(error) = outcome.end {
        return Err(format!("stopped partway: {error}").into());
    }
    Ok(())
}

/// This month by the clock, in UTC (near enough to begin a walk from: a walk
/// reads back from it, and a month ahead holds no record).
fn this_month() -> (i32, u32) {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (i32::try_from(year).unwrap_or(1970), u32::try_from(month).unwrap_or(1))
}
