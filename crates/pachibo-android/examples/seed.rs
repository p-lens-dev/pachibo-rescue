//! Test records written into ぱち簿 on a phone through its own screens, to
//! read back with the rescue and compare: `seed --yes-add-records
//! <expected.csv> [--from N] [--count N] [--plan]`.
//!
//! **This adds records to the ぱち簿 on the phone connected**, which can be
//! taken out again only one at a time in the app: use a phone kept for
//! testing. Nothing is written without `--yes-add-records`; `--plan` only
//! lists what would be written.
//!
//! The plan is ten years of records, newest first, laid out to try the
//! reading: months with none (two in a row, and over a year), a day of nine
//! records (its list scrolls), a leap day, the first and last of months, a
//! year's turn, holidays, the same date in two years, outlay in balls, nothing
//! taken back, six-figure amounts, memos with commas, quotes, `&`, `<`, line
//! breaks and Japanese (set past the keyboard), tags, hours. Parlors and
//! machines are chosen from ぱち簿's own database (Tokyo's parlors, its
//! newest machines) for the first four records — the free version allows
//! its database a few times only — and from its history after, so they are
//! real names, in Japanese, typed by no one.
//!
//! Each record's editor is read just before it is saved, and written to
//! `expected.csv` as the rescue writes records: a read of the phone should
//! give back exactly that file. Where a step does not land the seeder stops
//! and says where; `--from N` begins again from the plan's N-th record, and
//! `--adopt N` takes into the expected file a record of the plan's N-th day
//! that was saved before the seeder stopped (it only reads).

use std::error::Error as StdError;
use std::thread::sleep;
use std::time::Duration;

use pachibo_android::adb::Adb;
use pachibo_android::screen::{Screen, View};
use pachibo_android::{Calendar, Device, Record, csv_of, go_to_month, iso_date, parse_calendar, parse_record};

type Failed = Box<dyn StdError>;

/// A stop, with why.
fn stop<T>(why: impl Into<String>) -> Result<T, Failed> {
    Err(why.into().into())
}

// ---------------------------------------------------------------------------
// The plan.
// ---------------------------------------------------------------------------

/// Where a record's parlor or machine is chosen from.
#[derive(Debug, Clone, Copy)]
enum Pick {
    /// The n-th of ぱち簿's history (recently used).
    History(usize),
    /// The n-th choice from ぱち簿's database.
    Database(usize),
}

/// An amount and what it is counted in.
#[derive(Debug, Clone, Copy)]
enum Amount {
    /// Yen.
    Yen(u32),
    /// Balls (or, for a slot machine, medals), at the first rate ぱち簿
    /// offers other than yen.
    Balls(u32),
}

/// One record to write.
#[derive(Debug, Clone)]
struct Planned {
    date: (i32, u32, u32),
    parlor: Pick,
    machine: Pick,
    outlay: Amount,
    takings: Amount,
    hours: u32,
    tags: Vec<usize>,
    memo: &'static str,
}

/// Memos to cycle through: none, and the characters a CSV or XML must carry.
const MEMOS: [&str; 8] = ["", "good,day", "say \"hi\"", "", "line1\nline2", "x&y<z>", "it's 2 a.m.", "朝イチ並び、据え置き狙い"];

/// A small, fixed sequence of numbers, so the plan is the same every run.
struct Dice(u64);

impl Dice {
    fn roll(&mut self, below: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below.max(1)
    }

    /// One of `choices` (the first where there are none to roll for).
    fn one_of<T: Copy + Default>(&mut self, choices: &[T]) -> T {
        let at = usize::try_from(self.roll(u64::try_from(choices.len()).unwrap_or(1))).unwrap_or(0);
        choices.get(at).copied().unwrap_or_default()
    }

    /// A number below `below`.
    fn below(&mut self, below: usize) -> usize {
        usize::try_from(self.roll(u64::try_from(below).unwrap_or(1))).unwrap_or(0)
    }
}

/// The n-th of `choices`, going round.
fn round<T: Copy + Default>(choices: &[T], n: usize) -> T {
    choices.get(n % choices.len().max(1)).copied().unwrap_or_default()
}

fn days_in(year: i32, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn month_before((year, month): (i32, u32)) -> (i32, u32) {
    if month <= 1 { (year - 1, 12) } else { (year, month - 1) }
}

/// The plan, newest first: 2026-10 back to 2016-11.
fn plan() -> Vec<Planned> {
    let mut dice = Dice(20_261_121);
    // Days chosen in particular, and how many records each holds.
    let special: &[((i32, u32, u32), usize)] = &[
        ((2025, 11, 18), 1), // the same date as a record of 2026
        ((2025, 11, 3), 1),  // 文化の日
        ((2024, 2, 29), 1),  // a leap day
        ((2023, 5, 3), 9),   // 憲法記念日, and nine records: the list scrolls
        ((2022, 1, 1), 1),   // a year's turn
        ((2021, 12, 31), 2),
        ((2018, 1, 31), 1),
        ((2016, 11, 1), 1), // the oldest, on the first
    ];
    // Months left empty: two in a row, and fourteen.
    let empty = |month: (i32, u32)| matches!(month, (2025, 7 | 8)) || ((2019, 3)..=(2020, 4)).contains(&month);

    let mut days: Vec<((i32, u32, u32), usize)> = Vec::new();
    let mut month = (2026, 10);
    while month >= (2016, 11) {
        let chosen: Vec<_> = special.iter().filter(|((year, mon, _), _)| (*year, *mon) == month).copied().collect();
        if !chosen.is_empty() {
            days.extend(chosen);
        } else if !empty(month) {
            let count: usize = dice.one_of(&[0, 1, 1, 2]);
            let mut taken: Vec<u32> = Vec::new();
            for _ in 0..count {
                let day = u32::try_from(dice.roll(u64::from(days_in(month.0, month.1)))).unwrap_or(0) + 1;
                if !taken.contains(&day) {
                    taken.push(day);
                    days.push(((month.0, month.1, day), 1));
                }
            }
        }
        month = month_before(month);
    }
    // Newest first, so the calendar is turned back a month at a time.
    days.sort_by_key(|&(date, _)| std::cmp::Reverse(date));

    let mut plan = Vec::new();
    for (date, count) in days {
        for _ in 0..count {
            let n = plan.len();
            // The free ぱち簿 lets its database be used a few times only
            // (then it asks for the premium version): four from it, the rest
            // from the history those four leave.
            let parlor = if n < 4 { Pick::Database(n) } else { Pick::History(dice.below(5)) };
            let machine = if n < 4 { Pick::Database(n) } else { Pick::History(dice.below(7)) };
            let outlay_yen: u32 = dice.one_of(&[0, 1_000, 5_000, 12_000, 30_000, 46_000, 100_000, 999_999]);
            let outlay = if n % 7 == 3 { Amount::Balls(1_250) } else { Amount::Yen(outlay_yen) };
            let takings = if n % 5 == 1 { Amount::Yen(0) } else { Amount::Yen(dice.one_of(&[2_000, 8_500, 30_000, 127_000])) };
            let tags = match n % 4 {
                1 => vec![0],
                2 => vec![3, 4],
                _ => Vec::new(),
            };
            plan.push(Planned { date, parlor, machine, outlay, takings, hours: round(&[0, 1, 3, 5], n), tags, memo: round(&MEMOS, n) });
        }
    }
    plan
}

// ---------------------------------------------------------------------------
// Driving ぱち簿's screens.
// ---------------------------------------------------------------------------

/// The phone, a step at a time, each step checked.
struct Ui {
    phone: Adb,
}

/// The machines listed on a screen: a machine's row is its name over its
/// maker (an index of kana, as in 50音順, has an id=name with no maker).
fn machine_rows(screen: &Screen) -> Vec<View> {
    let views = screen.views();
    views
        .iter()
        .zip(views.iter().skip(1))
        .filter(|(view, next)| view.id.as_deref() == Some("name") && next.id.as_deref() == Some("maker"))
        .map(|(view, _)| view.clone())
        .collect()
}

/// The screens met, each known by what is on it.
fn is_calendar(screen: &Screen) -> bool {
    screen.has("year") && screen.has("viewpager")
}
fn is_editor(screen: &Screen) -> bool {
    screen.has("save") && screen.has("invest")
}
fn titled(screen: &Screen, title: &str) -> bool {
    screen.text_of("title").as_deref().map(str::trim) == Some(title) || screen.text_of("alertTitle").as_deref().map(str::trim) == Some(title)
}

impl Ui {
    /// The screen now, as XML and as views.
    fn look(&mut self) -> Result<(String, Screen), Failed> {
        let xml = self.phone.dump()?;
        let screen = Screen::parse(&xml);
        Ok((xml, screen))
    }

    /// Look until the screen is as `test` wants, up to a few times.
    fn wait(&mut self, what: &str, test: impl Fn(&Screen) -> bool) -> Result<(String, Screen), Failed> {
        for _ in 0..6 {
            let (xml, screen) = self.look()?;
            if test(&screen) {
                return Ok((xml, screen));
            }
            sleep(Duration::from_millis(400));
        }
        stop(format!("{what} did not come up"))
    }

    fn tap(&mut self, view: &View) -> Result<(), Failed> {
        let Some((x, y)) = view.center else {
            return stop(format!("{view:?} has no place to tap"));
        };
        self.phone.tap(x, y)?;
        Ok(())
    }

    fn tap_id(&mut self, screen: &Screen, id: &str) -> Result<(), Failed> {
        let Some(view) = screen.with_id(id).into_iter().next() else {
            return stop(format!("no {id} on screen"));
        };
        self.tap(&view)
    }

    fn tap_text(&mut self, screen: &Screen, text: &str) -> Result<(), Failed> {
        let Some(view) = screen.with_text(text).into_iter().next() else {
            return stop(format!("no {text} on screen"));
        };
        self.tap(&view)
    }

    /// The calendar, turned to `month` as the rescue turns it (the year and
    /// month picker where it is far); ぱち簿 brought up again first where it
    /// is not on screen (it crashes, out of memory, after hours of use).
    fn calendar_at(&mut self, month: (i32, u32)) -> Result<Calendar, Failed> {
        match go_to_month(&mut self.phone, month) {
            Ok(calendar) => Ok(calendar),
            Err(error) => {
                eprintln!("the calendar could not be turned to {:04}-{:02} ({error}); bringing ぱち簿 up again", month.0, month.1);
                self.phone.launch_calendar()?;
                Ok(go_to_month(&mut self.phone, month)?)
            }
        }
    }

    /// The editor of a new record on `date`.
    fn new_record(&mut self, date: (i32, u32, u32)) -> Result<(), Failed> {
        let (year, month, day) = date;
        let calendar = self.calendar_at((year, month))?;
        let Some(cell) = calendar.cells.iter().find(|cell| cell.day == day).cloned() else {
            return stop(format!("no cell for {}", iso_date(year, month, day)));
        };
        if !calendar.is_selected(day) {
            self.phone.tap(cell.center.0, cell.center.1)?;
            let (xml, _) = self.wait("the calendar", is_calendar)?;
            if !parse_calendar(&xml).is_some_and(|now| now.is_selected(day)) {
                return stop(format!("{} was not selected", iso_date(year, month, day)));
            }
        }
        // A tap on the day selected opens the 作成 menu.
        self.phone.tap(cell.center.0, cell.center.1)?;
        let (_, menu) = self.wait("the 作成 menu", |screen| titled(screen, "作成"))?;
        self.tap_text(&menu, "結果作成")?;
        let (xml, _) = self.wait("a new record's editor", is_editor)?;
        let opened = parse_record(&xml).map(|record| record.date);
        if opened.as_deref() != Some(iso_date(year, month, day).as_str()) {
            return stop(format!("the editor opened on {opened:?}, not {}", iso_date(year, month, day)));
        }
        Ok(())
    }

    fn pick_parlor(&mut self, pick: Pick) -> Result<(), Failed> {
        let (_, editor) = self.wait("the editor", is_editor)?;
        self.tap_id(&editor, "hall")?;
        let (_, local) = self.wait("ホール", |screen| titled(screen, "ホール"))?;
        match pick {
            Pick::History(n) => {
                let halls: Vec<View> = local.with_id("name").into_iter().filter(|view| view.text.as_deref() != Some("ホール名")).collect();
                let Some(hall) = halls.get(n % halls.len().max(1)).cloned() else {
                    return stop("no parlor in the history");
                };
                self.tap(&hall)?;
            }
            Pick::Database(n) => {
                self.tap_id(&local, "server")?;
                let (_, server) = self.wait("ホール・データベース", |screen| titled(screen, "ホール・データベース"))?;
                self.tap_id(&server, "tab2")?;
                let (_, by_address) = self.wait("住所", |screen| screen.has("prefecture"))?;
                self.tap_id(&by_address, "prefecture")?;
                let (_, prefectures) = self.wait("都道府県を選択", |screen| titled(screen, "都道府県を選択"))?;
                self.tap_text(&prefectures, "東京都")?;
                // ぱち簿 opens the choice of city at once where it remembers
                // the prefecture, else goes back to the address tab.
                let (_, next) = self.wait("市区町村", |screen| titled(screen, "市区町村・町名を選択") || (screen.has("city") && !titled(screen, "都道府県を選択")))?;
                if !titled(&next, "市区町村・町名を選択") {
                    self.tap_id(&next, "city")?;
                }
                let (_, cities) = self.wait("市区町村・町名を選択", |screen| titled(screen, "市区町村・町名を選択"))?;
                let names = cities.with_id("name");
                let Some(city) = names.get(n % names.len().clamp(1, 8)).cloned() else {
                    return stop("no city to choose");
                };
                self.tap(&city)?;
                let (_, halls) = self.wait("the parlors", |screen| screen.text_of("info").is_some_and(|info| info.contains('件')))?;
                let names = halls.with_id("name");
                let Some(hall) = names.get((n * 2 + 1) % names.len().clamp(1, 8)).cloned() else {
                    return stop("no parlor to choose");
                };
                self.tap(&hall)?;
            }
        }
        self.wait("the editor", is_editor)?;
        Ok(())
    }

    fn pick_machine(&mut self, pick: Pick) -> Result<(), Failed> {
        let (_, editor) = self.wait("the editor", is_editor)?;
        self.tap_id(&editor, "machine")?;
        let (_, local) = self.wait("機種", |screen| titled(screen, "機種"))?;
        let chooser = match pick {
            Pick::History(n) => (local, n),
            Pick::Database(n) => {
                // The database opens on its newest machines (新着順), a row
                // each; 50音順 is an index of kana to open first.
                self.tap_id(&local, "server")?;
                let (_, newest) = self.wait("機種・データベース", |screen| titled(screen, "機種・データベース") && !machine_rows(screen).is_empty())?;
                (newest, n)
            }
        };
        let (screen, n) = chooser;
        let names = machine_rows(&screen);
        let Some(machine) = names.get(n % names.len().clamp(1, 10)).cloned() else {
            return stop("no machine to choose");
        };
        self.tap(&machine)?;
        self.wait("the editor", is_editor)?;
        Ok(())
    }

    /// Set what went in (`invest`) or came back (`recovery`).
    fn amount(&mut self, side: &str, amount: Amount) -> Result<(), Failed> {
        let (in_yen, value) = match amount {
            Amount::Yen(value) => (true, value),
            Amount::Balls(value) => (false, value),
        };
        let (_, editor) = self.wait("the editor", is_editor)?;
        let now_in_yen = editor.text_of(&format!("{side}_mode")).as_deref().map(str::trim) == Some("円");
        if now_in_yen != in_yen {
            // The rates offered: 円, then balls' for a pachinko machine
            // (4.00, …) or medals' for a slot machine (20.00, …).
            self.tap_id(&editor, &format!("{side}_mode"))?;
            let (_, rates) = self.wait("換金率", |screen| !screen.with_text("換金率").is_empty())?;
            let offered = rates.with_id("name");
            let chosen = if in_yen { offered.iter().find(|view| view.text.as_deref() == Some("円")) } else { offered.iter().find(|view| view.text.as_deref() != Some("円")) };
            let Some(rate) = chosen.cloned() else {
                return stop("no rate to choose");
            };
            self.tap(&rate)?;
            self.wait("the editor", is_editor)?;
        }
        let (_, editor) = self.wait("the editor", is_editor)?;
        self.tap_id(&editor, side)?;
        let (_, keypad) = self.wait("the keypad", |screen| screen.has("key0") && screen.has("ok"))?;
        self.tap_id(&keypad, "ce")?;
        for digit in value.to_string().chars() {
            self.tap_id(&keypad, &format!("key{digit}"))?;
        }
        self.tap_id(&keypad, "ok")?;
        let (_, editor) = self.wait("the editor", is_editor)?;
        let shown: String = editor.text_of(side).unwrap_or_default().chars().filter(char::is_ascii_digit).collect();
        if shown.parse::<u32>().ok() != Some(value) {
            return stop(format!("{side} reads {shown}, not {value}"));
        }
        Ok(())
    }

    fn hours(&mut self, hours: u32) -> Result<(), Failed> {
        if hours == 0 {
            return Ok(());
        }
        let (_, editor) = self.wait("the editor", is_editor)?;
        self.tap_id(&editor, "time")?;
        // The hour wheel turns by a row for each row-high swipe on it (a tap
        // on the hour below does not turn it): turn it until the hour in its
        // middle is the one wanted.
        for _ in 0..30 {
            let (_, wheel) = self.wait("稼働時間", |screen| titled(screen, "稼働時間"))?;
            let mut shown: Vec<(i32, i32, u32)> = wheel
                .views()
                .into_iter()
                .filter_map(|view| {
                    let hour = view.text.as_deref()?.strip_suffix('時')?.parse().ok()?;
                    let (x, y) = view.center?;
                    Some((y, x, hour))
                })
                .collect();
            shown.sort_unstable();
            let (Some(&(top, x, _)), Some(&(middle, _, now))) = (shown.first(), shown.get(1)) else {
                return stop("no hour wheel");
            };
            if now == hours {
                self.tap_id(&wheel, "ok")?;
                let (_, editor) = self.wait("the editor", is_editor)?;
                let shown = editor.text_of("time").unwrap_or_default();
                if !shown.trim().starts_with(&format!("{hours}時間")) {
                    return stop(format!("the hours read {shown}, not {hours}"));
                }
                return Ok(());
            }
            let row = middle - top;
            let below = middle + row;
            if now < hours {
                self.phone.swipe((x, below), (x, middle), 400)?;
            } else {
                self.phone.swipe((x, middle), (x, below), 400)?;
            }
        }
        stop(format!("the hour wheel would not turn to {hours}"))
    }

    fn tags(&mut self, tags: &[usize]) -> Result<(), Failed> {
        if tags.is_empty() {
            return Ok(());
        }
        let (_, editor) = self.wait("the editor", is_editor)?;
        self.tap_id(&editor, "tag")?;
        let (_, list) = self.wait("タグ", |screen| titled(screen, "タグ") && screen.has("ok"))?;
        let names = list.with_id("name");
        for &n in tags {
            let Some(tag) = names.get(n).cloned() else {
                return stop(format!("no tag {n}"));
            };
            self.tap(&tag)?;
        }
        self.tap_id(&list, "ok")?;
        self.wait("the editor", is_editor)?;
        Ok(())
    }

    fn memo(&mut self, memo: &str) -> Result<(), Failed> {
        if memo.is_empty() {
            return Ok(());
        }
        let (_, editor) = self.wait("the editor", is_editor)?;
        self.tap_id(&editor, "memo")?;
        self.wait("編集", |screen| titled(screen, "編集"))?;
        self.phone.set_text(memo)?;
        // Read again: the keyboard coming up moves the dialog.
        let (_, dialog) = self.wait("編集", |screen| titled(screen, "編集"))?;
        self.tap_id(&dialog, "button1")?;
        self.wait("the editor", is_editor)?;
        Ok(())
    }

    /// Write one planned record; what the editor showed as it was saved.
    fn write(&mut self, planned: &Planned) -> Result<Record, Failed> {
        self.new_record(planned.date)?;
        self.pick_parlor(planned.parlor)?;
        self.pick_machine(planned.machine)?;
        self.amount("invest", planned.outlay)?;
        self.amount("recovery", planned.takings)?;
        self.hours(planned.hours)?;
        self.tags(&planned.tags)?;
        self.memo(planned.memo)?;
        let (xml, editor) = self.wait("the editor", is_editor)?;
        let Some(record) = parse_record(&xml) else {
            return stop("the filled editor did not read");
        };
        self.tap_id(&editor, "save")?;
        // An ad may come up over the save, and leave the calendar on this
        // month when it goes: turn back to the record's month to check it.
        sleep(Duration::from_secs(1));
        let (year, month, day) = planned.date;
        let calendar = self.calendar_at((year, month))?;
        if !calendar.cells.iter().any(|cell| cell.day == day && cell.has_record()) {
            return stop(format!("{} does not show a record after saving", iso_date(year, month, day)));
        }
        Ok(record)
    }

    /// The records on `date` as their editors show them, opened one by one
    /// from the day's list: to take into the expected file a record saved
    /// when the seeder stopped before writing it down.
    fn records_on(&mut self, date: (i32, u32, u32)) -> Result<Vec<Record>, Failed> {
        let (year, month, day) = date;
        let calendar = self.calendar_at((year, month))?;
        let Some(cell) = calendar.cells.iter().find(|cell| cell.day == day).cloned() else {
            return stop(format!("no cell for {}", iso_date(year, month, day)));
        };
        if !calendar.is_selected(day) {
            self.phone.tap(cell.center.0, cell.center.1)?;
        }
        let (xml, _) = self.wait("the day's list", is_calendar)?;
        if !parse_calendar(&xml).is_some_and(|now| now.is_selected(day)) {
            return stop(format!("{} was not selected", iso_date(year, month, day)));
        }
        // A row is a record where it shows a result; a holiday (元日) is a
        // row too, without one.
        let rows = pachibo_android::parse_day_list(&xml);
        let mut records = Vec::new();
        for row in rows {
            self.phone.tap(row.center.0, row.center.1)?;
            let (xml, _) = self.wait("the record's editor", is_editor)?;
            let Some(record) = parse_record(&xml) else {
                return stop("the editor did not read");
            };
            records.push(record);
            self.phone.back()?;
            self.wait("the calendar", is_calendar)?;
        }
        Ok(records)
    }
}

fn main() -> Result<(), Failed> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| arguments.iter().any(|argument| argument == name);
    let number = |name: &str| -> Option<usize> { arguments.iter().position(|argument| argument == name).and_then(|at| arguments.get(at + 1)).and_then(|value| value.parse().ok()) };
    let plan = plan();

    if flag("--plan") {
        for (n, planned) in plan.iter().enumerate() {
            let (year, month, day) = planned.date;
            println!("{n:>3} {} {:?} {:?} {:?} {:?} {}h tags {:?} memo {:?}", iso_date(year, month, day), planned.parlor, planned.machine, planned.outlay, planned.takings, planned.hours, planned.tags, planned.memo);
        }
        println!("{} records", plan.len());
        return Ok(());
    }
    let Some(out) = arguments.iter().find(|argument| !argument.starts_with("--") && argument.parse::<usize>().is_err()) else {
        eprintln!("usage: seed --yes-add-records <expected.csv> [--from N] [--count N] [--adopt N] | seed --plan");
        return Ok(());
    };
    if !flag("--yes-add-records") {
        return stop("this adds records to ぱち簿 on the phone; say so with --yes-add-records");
    }
    let from = number("--from").unwrap_or(0);
    let adopt = number("--adopt");
    let count = number("--count").unwrap_or(plan.len());

    let program = std::env::var("ADB").unwrap_or_else(|_| "adb".to_owned());
    let phone = Adb::new(program, std::env::var("ANDROID_SERIAL").ok()).log_to(std::io::stderr());
    let mut ui = Ui { phone };
    ui.phone.launch_calendar()?;

    // Records written before, kept across runs begun with --from.
    let mut written: Vec<Record> = std::fs::read(out).ok().and_then(|bytes| read_expected(&String::from_utf8_lossy(&bytes))).unwrap_or_default();
    if let Some(n) = adopt {
        let Some(planned) = plan.get(n) else {
            return stop(format!("the plan has no record {n}"));
        };
        let (year, month, day) = planned.date;
        let found = ui.records_on(planned.date)?;
        let missing: Vec<Record> = found.into_iter().filter(|record| !written.contains(record)).collect();
        println!("{}: {} record(s) not in {out}, taken in", iso_date(year, month, day), missing.len());
        written.extend(missing);
        written.sort_by(|a, b| a.date.cmp(&b.date));
        std::fs::write(out, csv_of(&written))?;
        return Ok(());
    }
    for (n, planned) in plan.iter().enumerate().skip(from).take(count) {
        let (year, month, day) = planned.date;
        println!("{n:>3}/{} {}", plan.len(), iso_date(year, month, day));
        match ui.write(planned) {
            Ok(record) => {
                written.push(record);
                written.sort_by(|a, b| a.date.cmp(&b.date));
                std::fs::write(out, csv_of(&written))?;
            }
            Err(why) => {
                eprintln!("stopped at record {n}: {why}");
                eprintln!("the phone may hold a part-written record on {}: check, then begin again with --from {n}", iso_date(year, month, day));
                return Err(why);
            }
        }
    }
    println!("{} records written; expected file {out}", written.len());
    Ok(())
}

/// Records from an expected file written before (the rescue's CSV), so a run
/// begun again with `--from` adds to them; none where it does not read.
fn read_expected(text: &str) -> Option<Vec<Record>> {
    let mut records = Vec::new();
    let mut rows = csv_rows(text.trim_start_matches('\u{feff}'));
    rows.next()?;
    for row in rows {
        let field = |at: usize| row.get(at).cloned().unwrap_or_default();
        let number = |at: usize| field(at).parse::<i64>().ok();
        records.push(Record {
            date: field(0),
            parlor: field(1),
            machine: field(2),
            outlay: number(3),
            takings: number(4),
            revenue: number(5).unwrap_or_default(),
            cabinet: field(6),
            hours: field(7),
            memo: field(8),
            tag: field(9),
        });
    }
    Some(records)
}

/// The rows of a CSV as the rescue writes it (RFC 4180: quoted fields with
/// doubled quotes, CRLF lines).
fn csv_rows(text: &str) -> impl Iterator<Item = Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (_, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.into_iter()
}
