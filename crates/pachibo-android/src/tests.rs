//! The parsing checked against dumps taken off a real phone (SH-RM15,
//! Android 12), and the walk checked against a pretend phone that behaves as
//! ぱち簿 does — and, on request, misbehaves as a real phone may.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::*;

/// A dump of the calendar with 11/11's record selected, so it carries the
/// header, the day cells (a holiday among them) and the day's list.
const CALENDAR: &str = include_str!("fixtures/calendar.xml");

/// A dump of a record's editor (`GameResult`), the fields empty but for the
/// date, parlor and machine carried in.
const EDITOR: &str = include_str!("fixtures/gameresult_blank.xml");

#[test]
fn yen_reads_the_way_the_screen_writes_it() {
    assert_eq!(yen_of("+127,000"), Some(127_000));
    assert_eq!(yen_of("-3,000"), Some(-3_000));
    assert_eq!(yen_of("−3,000"), Some(-3_000));
    assert_eq!(yen_of("0"), Some(0));
    assert_eq!(yen_of("¥1,200"), Some(1_200));
    assert_eq!(yen_of(""), None);
    assert_eq!(yen_of("—"), None);
}

#[test]
fn a_value_quoted_with_apostrophes_is_read() {
    // As uiautomator wrote a memo holding a double quote, on the phone.
    let nodes = scan(r#"<node index="0" text='say "hi"' resource-id="jp.garakuta.pachibo:id/memo" bounds="[150,1299][1080,1476]" />"#);
    let memo = nodes.first().expect("a node");
    assert_eq!(memo.text.as_deref(), Some("say \"hi\""));
    assert_eq!(memo.id.as_deref(), Some("memo"));
    assert_eq!(memo.bounds, Some((150, 1299, 1080, 1476)));
}

#[test]
fn a_value_holding_a_bracket_or_another_attribute_does_not_cut_the_tag() {
    let xml = r#"<node content-desc=' text="not this"' text="x>y" resource-id="a:id/memo" bounds="[0,0][10,10]" /><node text="next" bounds="[0,10][10,20]"/>"#;
    let nodes = scan(xml);
    assert_eq!(nodes.len(), 2);
    let first = nodes.first().expect("a node");
    assert_eq!(first.text.as_deref(), Some("x>y"));
    assert_eq!(first.id.as_deref(), Some("memo"));
    assert_eq!(nodes.get(1).and_then(|node| node.text.as_deref()), Some("next"));
}

#[test]
fn escapes_turn_back_named_and_numeric_once() {
    assert_eq!(unescape("a&amp;b"), "a&b");
    assert_eq!(unescape("&amp;lt;"), "&lt;");
    assert_eq!(unescape("朝&#10;夜"), "朝\n夜");
    assert_eq!(unescape("&#x41;&quot;"), "A\"");
    assert_eq!(unescape("a & b"), "a & b");
}

#[test]
fn the_calendar_gives_its_month_and_its_records() {
    let calendar = parse_calendar(CALENDAR).expect("the calendar reads");
    assert_eq!(calendar.shown(), (2026, 11));
    assert_eq!(calendar.total, Some(78_000));
    assert!(calendar.is_selected(11));
    assert!(!calendar.is_selected(1));
    assert!(calendar.pager.is_some());
    assert!(calendar.analysis.is_some());

    let recorded = calendar.recorded_cells();
    // The three records entered, and not the holidays (3日 文化の日, 23日 勤労感謝の日).
    assert_eq!(recorded.len(), 3);
    assert!(recorded.iter().all(|cell| cell.day != 3 && cell.day != 23));
    let eleventh = recorded.iter().find(|cell| cell.day == 11).expect("11日 holds a record");
    assert!(eleventh.machine.as_deref().unwrap_or_default().contains("魔術"));
    assert_eq!(eleventh.revenue, Some(-3_000));
    assert_ne!(eleventh.taps, Vec::new());
    // The month's results add up to its total.
    assert_eq!(recorded.iter().filter_map(|cell| cell.revenue).sum::<i64>(), 78_000);
}

#[test]
fn the_selected_day_lists_its_record() {
    let rows = parse_day_list(CALENDAR);
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the day holds a record");
    assert!(row.machine.contains("魔術"));
    assert!(row.parlor.contains("スーパー"));
    assert_eq!(row.revenue, -3_000);
    assert!(row.top < row.bottom);
}

#[test]
fn the_editor_gives_a_records_fields() {
    let record = parse_record(EDITOR).expect("the editor reads");
    assert_eq!(record.date, "2026-11-17");
    assert!(record.parlor.contains("スーパー"));
    assert!(record.machine.contains("魔術"));
    // invest/recovery are 円 at 0 in this fresh record, so read as 0, not left out.
    assert_eq!(record.outlay, Some(0));
    assert_eq!(record.takings, Some(0));
    assert_eq!(record.revenue, 0);
    assert_eq!(record.cabinet, ""); // 0000 is no cabinet
}

#[test]
fn the_calendars_own_create_button_is_not_taken_for_the_create_menu() {
    // The real calendar's bar carries 結果作成 (id=create): taking it for the
    // menu pressed back on the bare calendar, closing ぱち簿.
    assert!(!menu_open(&scan(CALENDAR)));
    let menu = r#"<node resource-id="jp.garakuta.pachibo:id/name" text="結果作成" bounds="[0,1500][1080,1600]"/>"#;
    assert!(menu_open(&scan(menu)));
}

#[test]
fn the_calendar_is_brought_up_without_pressing_back_on_it() {
    let mut phone = usual_phone();
    phone.launch_calendar().expect("it launches");
    phone.steps.clear();
    calendar_now(&mut phone).expect("the calendar reads");
    assert!(!phone.steps.iter().any(|step| step == "back"), "back was pressed: {:?}", phone.steps);
    assert_eq!(phone.screen, Screen::Calendar);
}

// ---------------------------------------------------------------------------
// Real compressed dumps (`uiautomator dump --compressed`, as the app takes
// them), off the same phone.
// ---------------------------------------------------------------------------

/// The calendar with 11/17's record selected.
const CALENDAR_DAY17: &str = include_str!("fixtures/calendar_day17.xml");
/// The calendar with 11/5 selected, a day with no record (`11月 5日(木)`).
const CALENDAR_DAY5: &str = include_str!("fixtures/calendar_day5.xml");
/// October, a month with no record, with 11/5 still selected.
const CALENDAR_EMPTY_MONTH: &str = include_str!("fixtures/calendar_empty_month.xml");
/// 11/17's record open in its editor.
const EDITOR_FILLED: &str = include_str!("fixtures/gameresult_filled.xml");
/// The analysis screen, both games, no period set.
const ANALYSIS: &str = include_str!("fixtures/analysis.xml");
/// The 作成 menu, opened by a tap on the day already selected.
const CREATE_MENU: &str = include_str!("fixtures/create_menu.xml");

#[test]
fn a_real_compressed_calendar_reads_with_its_selected_days_list() {
    let calendar = parse_calendar(CALENDAR_DAY17).expect("the calendar reads");
    assert_eq!(calendar.shown(), (2026, 11));
    assert_eq!(calendar.total, Some(78_000));
    assert!(calendar.is_selected(17));
    assert_eq!(calendar.recorded_cells().iter().map(|cell| cell.day).collect::<Vec<_>>(), [11, 17, 18]);
    assert!(calendar.pager.is_some() && calendar.analysis.is_some());
    let rows = parse_day_list(CALENDAR_DAY17);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.first().map(|row| row.revenue), Some(127_000));
    assert!(!menu_open(&scan(CALENDAR_DAY17)));
}

#[test]
fn a_one_digit_day_is_selected_though_the_header_pads_it() {
    let calendar = parse_calendar(CALENDAR_DAY5).expect("the calendar reads");
    assert_eq!(calendar.selected.as_deref(), Some("11月 5日(木)"));
    assert!(calendar.is_selected(5));
    assert!(!calendar.is_selected(15));
    assert!(!calendar.is_selected(25));
}

#[test]
fn the_same_date_of_another_year_is_not_taken_for_the_one_selected() {
    // Seen on the phone: 2025-11 shown, with 2026-11-18 (a Wednesday) still
    // selected. 2025-11-18 was a Tuesday.
    let shown = |year| Calendar { year, month: 11, cells: Vec::new(), total: None, selected: Some("11月18日(水)".to_owned()), pager: None, analysis: None, this_month: None, header: None };
    assert!(shown(2026).is_selected(18));
    assert!(!shown(2025).is_selected(18));
    assert_eq!(weekday_of(2026, 11, 18), 3);
    assert_eq!(weekday_of(2024, 2, 29), 4); // a Thursday
    assert_eq!(weekday_of(2000, 1, 1), 6); // a Saturday
}

#[test]
fn a_walk_begun_years_back_takes_the_year_and_month_picker() {
    // ぱち簿 starts afresh on this month (2026-03); the walk begins years back.
    let mut phone = usual_phone().with((2019, 4), vec![kept(9, "海物語", 7_000)]);
    let outcome = rescue(&mut phone, Walk { start: Some((2019, 4)), ..walk(false) }, |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(dates(&outcome), ["2019-04-09"]);
    // Seven turns of the year wheel and one of the month's, rather than 83
    // turns of the month.
    assert!(phone.notes.iter().any(|note| note.contains("picker")), "{:?}", phone.notes);
    assert!(phone.steps.iter().filter(|step| step.starts_with("swipe")).count() < 20, "{}", phone.steps.len());
}

#[test]
fn a_walk_begun_away_without_the_picker_turns_by_swipes() {
    let mut phone = usual_phone();
    phone.has_picker = false;
    let outcome = rescue(&mut phone, Walk { start: Some((2025, 12)), ..walk(false) }, |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(dates(&outcome), ["2025-12-31"]);
    assert!(phone.notes.iter().any(|note| note.contains("turning by swipes")), "{:?}", phone.notes);
}

#[test]
fn a_walk_starts_pachibo_afresh_whatever_it_was_left_on() {
    // Left years back, with a date of another month selected: started afresh
    // it shows this month, and the walk reads from there.
    let mut phone = usual_phone();
    phone.shown = (2019, 4);
    phone.selected = Some((2026, 1, 2));
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 5);
    assert_eq!(phone.steps.first().map(String::as_str), Some("restart"));
}

#[test]
fn the_count_goes_up_with_every_record_not_only_every_month() {
    let mut phone = Phone::new((2026, 3)).with((2026, 3), vec![kept(5, "海物語", 10_000), kept(5, "北斗", 3_000), kept(9, "番長", -1_000)]);
    let mut heard: Vec<((i32, u32), usize)> = Vec::new();
    let outcome = rescue(&mut phone, walk(true), |progress| heard.push(((progress.year, progress.month), progress.records)), || false);
    assert_eq!(outcome.records.len(), 3);
    let this_month: Vec<usize> = heard.iter().filter(|(month, _)| *month == (2026, 3)).map(|&(_, records)| records).collect();
    assert_eq!(this_month, vec![0, 1, 2, 3]);
    assert!(heard.iter().filter(|(month, _)| *month == (2026, 2)).all(|&(_, records)| records == 3));
}

#[test]
fn a_walk_goes_on_after_pachibo_crashes_partway() {
    // Years back, ぱち簿 crashes (as it did, out of memory, after hours) and
    // starts again on this month: the walk brings it up and picks its way
    // back to the month it was reading.
    let phone = || {
        Phone::new((2026, 3))
            .with((2026, 3), vec![kept(5, "海物語", 10_000)])
            .with((2021, 6), vec![kept(1, "北斗", 3_000), kept(30, "番長", -1_000)])
            .with((2019, 2), vec![kept(14, "まどか", 5_000)])
    };
    let whole = rescue(&mut phone(), Walk { empty_months_before_stop: 60, ..walk(true) }, |_| {}, || false);
    assert_eq!(whole.records.len(), 4);
    let mut crashing = phone();
    crashing.crash_after = Some(140);
    let outcome = rescue(&mut crashing, Walk { empty_months_before_stop: 60, ..walk(true) }, |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records, whole.records);
    assert!(crashing.running);
    assert!(crashing.steps.iter().filter(|step| *step == "launch").count() >= 2);
}

#[test]
fn a_real_empty_month_has_no_records_no_total_and_not_the_selected_day() {
    let calendar = parse_calendar(CALENDAR_EMPTY_MONTH).expect("the calendar reads");
    assert_eq!(calendar.shown(), (2026, 10));
    assert_eq!(calendar.recorded_cells(), Vec::new());
    assert_eq!(calendar.total, None);
    // The header still names 11/5, selected in November: not October's 5th.
    assert!(!calendar.is_selected(5));
}

#[test]
fn a_real_filled_editor_reads_every_field() {
    let record = parse_record(EDITOR_FILLED).expect("the editor reads");
    assert_eq!(record.date, "2026-11-17");
    assert!(record.parlor.contains("錦糸町"));
    assert!(record.machine.contains("Re:ゼロ"));
    assert_eq!(record.outlay, Some(5_000));
    assert_eq!(record.takings, Some(132_000));
    assert_eq!(record.revenue, 127_000);
    assert_eq!(record.hours, "3時間00分");
    assert_eq!(record.cabinet, "");
    assert_eq!(record.tag, "");
    assert_eq!(record.memo, "");
}

#[test]
fn the_real_analysis_screen_gives_the_lifetime_count() {
    let mut phone = AnalysisOnly { screen: 0 };
    assert_eq!(survey(&mut phone, || false).expect("the survey runs").records, Some(3));
}

#[test]
fn the_real_create_menu_is_known_and_is_no_calendar() {
    let nodes = scan(CREATE_MENU);
    assert!(menu_open(&nodes));
    assert_eq!(calendar_of(&nodes), None);
}

/// A phone that shows the real calendar, then (once 分析 is tapped) the real
/// analysis screen, then the calendar again.
struct AnalysisOnly {
    screen: u8,
}

impl Device for AnalysisOnly {
    fn launch_calendar(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn dump(&mut self) -> Result<String, Error> {
        Ok(if self.screen == 1 { ANALYSIS } else { CALENDAR_DAY17 }.to_owned())
    }
    fn tap(&mut self, _x: i32, _y: i32) -> Result<(), Error> {
        self.screen = 1;
        Ok(())
    }
    fn back(&mut self) -> Result<(), Error> {
        self.screen = 2;
        Ok(())
    }
    fn swipe(&mut self, _from: (i32, i32), _to: (i32, i32), _millis: u32) -> Result<(), Error> {
        Ok(())
    }
}

#[test]
fn the_calendar_is_not_taken_for_an_editor_nor_the_editor_for_a_calendar() {
    assert_eq!(parse_record(CALENDAR), None);
    assert_eq!(parse_calendar(EDITOR), None);
}

#[test]
fn a_filled_editor_reads_invest_recovery_memo_and_tag() {
    let record = parse_record(FILLED_EDITOR).expect("the editor reads");
    assert_eq!(record.outlay, Some(20_000));
    assert_eq!(record.takings, Some(30_000));
    assert_eq!(record.revenue, 10_000);
    assert_eq!(record.cabinet, "0123");
    assert_eq!(record.tag, "朝一");
    assert_eq!(record.memo, "good\nday");
    assert_eq!(record.hours, "3時間00分");
}

#[test]
fn balls_and_medals_are_left_out_rather_than_read_as_yen() {
    let record = parse_record(BALLS_EDITOR).expect("the editor reads");
    assert_eq!(record.outlay, None); // 1500 玉 is not ¥1500
    assert_eq!(record.takings, None);
    assert_eq!(record.revenue, 8_000); // the result is still yen
}

#[test]
fn a_side_kept_in_balls_or_medals_is_worked_out_from_the_result_for_the_csv() {
    let record = |outlay: Option<i64>, takings: Option<i64>, revenue: i64| Record { outlay, takings, revenue, ..Record::default() };
    // As on the phone: 2017-01-05, put in as balls, ¥2,000 back, -¥3,000.
    assert_eq!(record(None, Some(2_000), -3_000).sides(), (5_000, 2_000));
    assert_eq!(record(Some(5_000), None, 25_000).sides(), (5_000, 30_000));
    // Both unknown (both in balls, or a shallow read): from the result alone.
    assert_eq!(record(None, None, -5_000).sides(), (5_000, 0));
    assert_eq!(record(None, None, 8_000).sides(), (0, 8_000));
    // A side that would come out below zero is not trusted.
    assert_eq!(record(None, Some(1_000), 3_000).sides(), (0, 3_000));
    assert_eq!(record(Some(1_000), Some(4_000), 3_000).sides(), (1_000, 4_000));
    let csv = String::from_utf8(csv_of(&[Record { date: "2017-01-05".to_owned(), ..record(None, Some(2_000), -3_000) }])).expect("the CSV is UTF-8");
    assert!(csv.contains("\r\n2017-01-05,,,5000,2000,-3000,"));
}

#[test]
fn the_csv_carries_the_header_bom_and_fields() {
    let record = Record {
        date: "2026-03-05".to_owned(),
        parlor: "マルハン, 新宿".to_owned(), // a comma, to check quoting
        machine: "スーパー海物語".to_owned(),
        outlay: Some(20_000),
        takings: Some(30_000),
        revenue: 10_000,
        cabinet: "0123".to_owned(),
        hours: String::new(),
        memo: "よき".to_owned(),
        tag: "朝一".to_owned(),
    };
    let csv = String::from_utf8(csv_of(&[record])).expect("the CSV is UTF-8");
    assert!(csv.starts_with('\u{feff}'));
    assert!(csv.contains("日付,ホール,機種,投資,回収,収支,台番,時間,メモ,タグ\r\n"));
    assert!(csv.contains("2026-03-05,\"マルハン, 新宿\",スーパー海物語,20000,30000,10000,0123,,よき,朝一\r\n"));
}

#[test]
fn rows_carried_over_a_scroll_are_counted_once() {
    let row = |machine: &str, top: i32| ListRow { machine: machine.to_owned(), parlor: "P".to_owned(), revenue: 1, center: (0, top), top, bottom: top + 10 };
    let before = [row("a", 0), row("b", 10), row("c", 20)];
    let now = [row("b", 0), row("c", 10), row("d", 20)];
    let fresh = rows_after(&before, &now);
    assert_eq!(fresh.iter().map(|row| row.machine.as_str()).collect::<Vec<_>>(), ["d"]);
    assert_eq!(rows_after(&before, &before), [] as [ListRow; 0]);
    assert_eq!(rows_after(&[], &now).len(), 3);
}

// ---------------------------------------------------------------------------
// The walk, over a pretend phone.
// ---------------------------------------------------------------------------

/// A record as the pretend phone keeps it.
#[derive(Debug, Clone)]
struct Kept {
    day: u32,
    machine: String,
    parlor: String,
    revenue: i64,
    memo: String,
}

/// What the pretend phone shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// The calendar (with a day's list where one is selected).
    Calendar,
    /// The create menu, over the calendar.
    Menu,
    /// The editor of the selected day's record at this index.
    Editor(usize),
    /// The analysis screen.
    Analysis,
    /// The year and month picker the header opens, at this year and month.
    Picker(i32, u32),
    /// The phone's home screen: the app has been left.
    Home,
}

/// A phone that behaves as ぱち簿 does — as seen on a real one (SH-RM15,
/// Android 12): a tap anywhere on a day's cell selects it, but a tap on the
/// day already selected opens the 作成 menu; the selection is a date, kept
/// across a turn of the month and a launch (the list header still names it);
/// a launch brings the app up where it was; a month with no record shows no
/// total. The screen is laid out as on that phone (a 7-wide grid in the
/// pager, the day's list below, seven rows of it in view), and faults can
/// be set.
struct Phone {
    /// This month by the phone's clock, where 今月 turns the calendar.
    today: (i32, u32),
    records: BTreeMap<(i32, u32), Vec<Kept>>,
    /// Days that name a holiday, whose text a tap on opens the menu.
    holidays: BTreeMap<(i32, u32, u32), String>,
    shown: (i32, u32),
    /// The date selected, `(year, month, day)`.
    selected: Option<(i32, u32, u32)>,
    /// How many times the 作成 menu was opened.
    menus_opened: u32,
    scroll: usize,
    screen: Screen,
    steps: Vec<String>,
    /// So many turns of the month ignored before they take.
    swipes_ignored: u32,
    /// So many record taps that open the record below instead.
    taps_off_by_one: u32,
    /// So many dumps answered before the phone goes away.
    dumps_left: Option<usize>,
    /// Notes the walk made.
    notes: Vec<String>,
    /// Whether the analysis screen shows one game only.
    analysis_narrowed: bool,
    /// Whether the header opens the year and month picker.
    has_picker: bool,
    /// So many dumps answered before ぱち簿 crashes (as it does, out of
    /// memory, after hours): the home screen until it is launched again,
    /// when it starts afresh on this month.
    crash_after: Option<usize>,
    /// Whether ぱち簿 is running.
    running: bool,
    /// How many dumps an editor takes to open or close, as on a ぱち簿 grown
    /// slow after long use.
    slow: usize,
    /// A change of screen under way: the screen it comes to, and the dumps
    /// before it shows.
    pending: Option<(Screen, usize)>,
}

const LIST_TOP: i32 = 1148;
const LIST_BOTTOM: i32 = 1962;
const ROW: i32 = 116;
const ROWS_IN_VIEW: usize = 7;
const CELL_W: i32 = 154;
const CELL_H: i32 = 147;
const GRID_TOP: i32 = 200;

fn days_in((year, month): (i32, u32)) -> u32 {
    match month {
        2 if year % 4 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn node(id: &str, text: &str, (l, t, r, b): Bounds) -> String {
    let text = text.replace('&', "&amp;").replace('"', "&quot;").replace('\n', "&#10;");
    format!("<node text=\"{text}\" resource-id=\"jp.garakuta.pachibo:id/{id}\" bounds=\"[{l},{t}][{r},{b}]\" />\n")
}

fn kept(day: u32, machine: &str, revenue: i64) -> Kept {
    Kept { day, machine: machine.to_owned(), parlor: "マルハン".to_owned(), revenue, memo: format!("{machine}\nmemo") }
}

impl Phone {
    fn new(today: (i32, u32)) -> Self {
        Self {
            today,
            records: BTreeMap::new(),
            holidays: BTreeMap::new(),
            shown: today,
            selected: None,
            menus_opened: 0,
            scroll: 0,
            screen: Screen::Home,
            steps: Vec::new(),
            swipes_ignored: 0,
            taps_off_by_one: 0,
            dumps_left: None,
            notes: Vec::new(),
            analysis_narrowed: false,
            has_picker: true,
            crash_after: None,
            running: true,
            slow: 0,
            pending: None,
        }
    }

    fn with(mut self, month: (i32, u32), records: Vec<Kept>) -> Self {
        self.records.insert(month, records);
        self
    }

    fn total(&self) -> usize {
        self.records.values().map(Vec::len).sum()
    }

    fn month_records(&self) -> Vec<Kept> {
        self.records.get(&self.shown).cloned().unwrap_or_default()
    }

    fn day_records(&self) -> Vec<Kept> {
        let Some((year, month, day)) = self.selected else {
            return Vec::new();
        };
        self.records.get(&(year, month)).cloned().unwrap_or_default().into_iter().filter(|record| record.day == day).collect()
    }

    fn cell_bounds(day: u32) -> Bounds {
        let at = i32::try_from(day).unwrap_or(1) - 1;
        let (col, row) = (at % 7, at / 7);
        let (l, t) = (col * CELL_W, GRID_TOP + row * CELL_H);
        (l, t, l + CELL_W, t + CELL_H)
    }

    fn text_bounds(day: u32) -> Bounds {
        let (l, t, r, _) = Self::cell_bounds(day);
        (l, t + 60, r, t + 100)
    }

    fn payment_bounds(day: u32) -> Bounds {
        let (l, t, r, _) = Self::cell_bounds(day);
        (l, t + 100, r, t + 140)
    }

    fn row_top(slot: usize) -> i32 {
        LIST_TOP + i32::try_from(slot).unwrap_or(0) * ROW
    }

    fn calendar_xml(&self) -> String {
        let (year, month) = self.shown;
        let records = self.month_records();
        let total: i64 = records.iter().map(|record| record.revenue).sum();
        let mut xml = String::from("<hierarchy>\n");
        xml += &node("date", "", (0, 85, 298, 162));
        xml += &node("year", &year.to_string(), (0, 106, 120, 157));
        xml += &node("month", &month.to_string(), (162, 85, 256, 162));
        if !records.is_empty() {
            xml += &node("total", &format!("{total:+}"), (298, 117, 466, 162));
        }
        xml += &node("this_month", "今月", (840, 72, 990, 162));
        xml += &node("viewpager", "", (0, 198, 1080, 1085));
        // A day of the month before, with no number.
        xml += &node("layout", "", (0, 1000, 154, 1080));
        xml += &node("day", "", (0, 1000, 40, 1030));
        for day in 1..=days_in(self.shown) {
            xml += &node("layout", "", Self::cell_bounds(day));
            xml += &node("day", &day.to_string(), Self::cell_bounds(day));
            let on_day: Vec<&Kept> = records.iter().filter(|record| record.day == day).collect();
            let holiday = self.holidays.get(&(year, month, day));
            let text = holiday.cloned().or_else(|| on_day.first().map(|record| record.machine.clone()));
            if let Some(text) = text {
                xml += &node("text", &text, Self::text_bounds(day));
            }
            if !on_day.is_empty() {
                let sum: i64 = on_day.iter().map(|record| record.revenue).sum();
                xml += &node("payment", &format!("{sum:+}"), Self::payment_bounds(day));
            }
        }
        if let Some((_, selected_month, day)) = self.selected {
            let selected_year = self.selected.map_or(year, |(selected_year, _, _)| selected_year);
            let weekday = WEEKDAYS.chars().nth(weekday_of(selected_year, selected_month, day)).unwrap_or('?');
            xml += &node("select", &format!("{selected_month}月{day:>2}日({weekday})"), (0, 1085, 300, 1148));
            xml += &node("list", "", (0, LIST_TOP, 1080, LIST_BOTTOM));
            for (slot, record) in self.day_records().iter().skip(self.scroll).take(ROWS_IN_VIEW).enumerate() {
                let top = Self::row_top(slot);
                xml += &node("name", &record.machine, (17, top + 10, 1063, top + 70));
                xml += &node("hall", &record.parlor, (17, top + 70, 972, top + 110));
                xml += &node("revenue", &format!("{:+}", record.revenue), (972, top + 70, 1063, top + 110));
            }
        }
        xml += &node("analysis", "分析", (48, 1965, 168, 2082));
        xml += &node("hall", "ホール", (264, 1964, 384, 2082));
        // The bar's create button, on every calendar, as on the real one.
        xml += &node("create", "結果作成", (912, 1962, 1032, 2082));
        xml += "</hierarchy>";
        xml
    }

    fn editor_xml(&self, index: usize) -> String {
        let records = self.day_records();
        let (Some(record), Some((year, month, _))) = (records.get(index), self.selected) else {
            return "<hierarchy></hierarchy>".to_owned();
        };
        let mut xml = String::from("<hierarchy>\n");
        xml += &node("date", &format!("{year}年{month}月{}日(水)", record.day), (150, 72, 1080, 177));
        xml += &node("hall", &record.parlor, (0, 200, 1080, 300));
        xml += &node("machine", &record.machine, (0, 300, 1080, 400));
        xml += &node("invest", "10,000", (0, 447, 500, 567));
        xml += &node("invest_mode", "円", (510, 447, 660, 567));
        xml += &node("recovery", &(10_000 + record.revenue).to_string(), (0, 567, 500, 687));
        xml += &node("recovery_mode", "円", (510, 567, 660, 687));
        xml += &node("revenue", &format!("{:+}", record.revenue), (0, 693, 480, 783));
        xml += &node("memo", &record.memo, (0, 800, 1080, 900));
        xml += &node("save", "保存", (223, 1950, 856, 2070));
        xml += "</hierarchy>";
        xml
    }

    fn analysis_xml(&self) -> String {
        let checked = if self.analysis_narrowed { "false" } else { "true" };
        format!(
            "<hierarchy>\n<node resource-id=\"jp.garakuta.pachibo:id/pachinko\" text=\"パチンコ\" checked=\"true\" bounds=\"[0,100][200,150]\"/>\n\
             <node resource-id=\"jp.garakuta.pachibo:id/slot\" text=\"スロット\" checked=\"{checked}\" bounds=\"[200,100][400,150]\"/>\n\
             <node text=\"回数\" bounds=\"[0,300][100,340]\"/>\n\
             <node resource-id=\"jp.garakuta.pachibo:id/count\" text=\"{}\" bounds=\"[120,300][260,340]\"/>\n</hierarchy>",
            self.total()
        )
    }

    fn within((left, top, right, bottom): Bounds, (x, y): (i32, i32)) -> bool {
        (left..right).contains(&x) && (top..bottom).contains(&y)
    }
}

impl Device for Phone {
    fn launch_calendar(&mut self) -> Result<(), Error> {
        // Brought up as it was: the month shown and the date selected kept —
        // unless it had crashed, when it starts afresh on this month.
        self.steps.push("launch".to_owned());
        self.pending = None;
        if !self.running {
            self.running = true;
            self.shown = self.today;
            self.selected = Some((self.today.0, self.today.1, 21));
            self.scroll = 0;
        }
        self.screen = Screen::Calendar;
        Ok(())
    }

    fn dump(&mut self) -> Result<String, Error> {
        if let Some(left) = &mut self.dumps_left {
            if *left == 0 {
                return Err(Error::PhoneLost("error: device offline".to_owned()));
            }
            *left -= 1;
        }
        if let Some((next, left)) = self.pending {
            if left == 0 {
                self.screen = next;
                self.pending = None;
            } else {
                self.pending = Some((next, left - 1));
            }
        }
        if let Some(left) = &mut self.crash_after {
            if *left == 0 {
                self.crash_after = None;
                self.running = false;
                self.screen = Screen::Home;
            } else {
                *left -= 1;
            }
        }
        Ok(match self.screen {
            Screen::Calendar => self.calendar_xml(),
            Screen::Menu => CREATE_MENU.to_owned(),
            Screen::Editor(index) => self.editor_xml(index),
            Screen::Analysis => self.analysis_xml(),
            Screen::Picker(year, month) => {
                let mut xml = String::from("<hierarchy>\n");
                for (row, offset) in [-1, 0, 1].into_iter().enumerate() {
                    let top = 890 + i32::try_from(row).unwrap_or(0) * 120;
                    let shown_month = (i64::from(month) - 1 + i64::from(offset)).rem_euclid(12) + 1;
                    let _ = writeln!(xml, "<node text=\"{}年\" bounds=\"[243,{top}][573,{}]\" />", year + offset, top + 120);
                    let _ = writeln!(xml, "<node text=\"{shown_month}月\" bounds=\"[573,{top}][843,{}]\" />", top + 120);
                }
                xml += &node("button1", "OK", (306, 1277, 780, 1421)).replace("jp.garakuta.pachibo:id/button1", "android:id/button1");
                xml + "</hierarchy>"
            }
            Screen::Home => "<hierarchy><node text=\"\" resource-id=\"com.android.launcher:id/workspace\" bounds=\"[0,0][1080,2160]\"/></hierarchy>".to_owned(),
        })
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<(), Error> {
        self.steps.push(format!("tap {x},{y}"));
        assert!(self.screen != Screen::Menu, "a tap with the create menu open would make a record");
        assert!(!matches!(self.screen, Screen::Editor(_)), "a tap inside a record's editor would change it");
        assert!(self.pending.is_none(), "a tap while the screen changes could land in an editor as it opens");
        if let Screen::Picker(year, month) = self.screen {
            if Self::within((306, 1277, 780, 1421), (x, y)) {
                self.shown = (year, month);
                self.screen = Screen::Calendar;
            }
            return Ok(());
        }
        if self.screen != Screen::Calendar {
            return Ok(());
        }
        if Self::within((0, 85, 298, 162), (x, y)) {
            if self.has_picker {
                self.screen = Screen::Picker(self.shown.0, self.shown.1);
            }
            return Ok(());
        }
        if Self::within((48, 1965, 168, 2082), (x, y)) {
            self.screen = Screen::Analysis;
            return Ok(());
        }
        if Self::within((840, 72, 990, 162), (x, y)) {
            self.shown = self.today;
            return Ok(());
        }
        if let Some(day) = (1..=days_in(self.shown)).find(|&day| Self::within(Self::cell_bounds(day), (x, y))) {
            let date = (self.shown.0, self.shown.1, day);
            if self.selected == Some(date) {
                self.screen = Screen::Menu;
                self.menus_opened += 1;
            } else {
                self.selected = Some(date);
                self.scroll = 0;
            }
            return Ok(());
        }
        if self.selected.is_some() && (LIST_TOP..LIST_BOTTOM).contains(&y) {
            let slot = usize::try_from((y - LIST_TOP) / ROW).unwrap_or(0);
            let mut index = self.scroll + slot;
            if self.taps_off_by_one > 0 {
                self.taps_off_by_one -= 1;
                index += 1;
            }
            if index < self.day_records().len() {
                if self.slow > 0 {
                    self.pending = Some((Screen::Editor(index), self.slow));
                } else {
                    self.screen = Screen::Editor(index);
                }
            }
        }
        Ok(())
    }

    fn back(&mut self) -> Result<(), Error> {
        self.steps.push("back".to_owned());
        assert!(self.pending.is_none(), "a back while the last one is under way would land on the calendar and close the app");
        if self.slow > 0 && matches!(self.screen, Screen::Editor(_)) {
            self.pending = Some((Screen::Calendar, self.slow));
            return Ok(());
        }
        self.screen = match self.screen {
            Screen::Calendar | Screen::Home => Screen::Home,
            Screen::Menu | Screen::Editor(_) | Screen::Analysis | Screen::Picker(..) => Screen::Calendar,
        };
        Ok(())
    }

    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), _millis: u32) -> Result<(), Error> {
        self.steps.push(format!("swipe {from:?}->{to:?}"));
        if let Screen::Picker(year, month) = self.screen {
            // A row-high stroke up turns a wheel on by one, down back by one.
            let step = if to.1 < from.1 { 1 } else { -1 };
            self.screen = if (243..573).contains(&from.0) {
                Screen::Picker(year + step, month)
            } else {
                Screen::Picker(year, u32::try_from((i64::from(month) - 1 + i64::from(step)).rem_euclid(12) + 1).unwrap_or(1))
            };
            return Ok(());
        }
        if self.screen != Screen::Calendar {
            return Ok(());
        }
        let horizontal = (to.0 - from.0).abs() > (to.1 - from.1).abs();
        if horizontal && (198..1085).contains(&from.1) {
            if self.swipes_ignored > 0 {
                self.swipes_ignored -= 1;
                return Ok(());
            }
            self.shown = if to.0 > from.0 { month_before(self.shown) } else { month_after(self.shown) };
        } else if !horizontal && (LIST_TOP..LIST_BOTTOM).contains(&from.1) {
            // Scroll by whole rows, a little short of the stroke.
            let rows = usize::try_from((from.1 - to.1) / ROW).unwrap_or(0).saturating_sub(1);
            let most = self.day_records().len().saturating_sub(ROWS_IN_VIEW);
            self.scroll = (self.scroll + rows).min(most);
        }
        Ok(())
    }

    fn note(&mut self, what: &str) {
        self.notes.push(what.to_owned());
    }

    fn restart(&mut self) -> Result<(), Error> {
        self.steps.push("restart".to_owned());
        self.running = false;
        self.launch_calendar()
    }
}

fn month_after((year, month): (i32, u32)) -> (i32, u32) {
    if month >= 12 { (year + 1, 1) } else { (year, month + 1) }
}

/// Three months of records with a month's gap, as one person might have.
fn usual_phone() -> Phone {
    Phone::new((2026, 3))
        .with((2026, 3), vec![kept(5, "海物語", 10_000), kept(5, "北斗", -2_000), kept(20, "ジャグラー", -5_000)])
        .with((2026, 1), vec![kept(2, "まどか", 30_000)])
        .with((2025, 12), vec![kept(31, "番長", -12_000)])
}

fn walk(deep: bool) -> Walk {
    Walk { deep, empty_months_before_stop: 3, ..Walk::default() }
}

fn dates(outcome: &Outcome) -> Vec<&str> {
    outcome.records.iter().map(|record| record.date.as_str()).collect()
}

#[test]
fn the_walk_reads_every_month_back_oldest_first() {
    let mut phone = usual_phone();
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(dates(&outcome), ["2025-12-31", "2026-01-02", "2026-03-05", "2026-03-05", "2026-03-20"]);
    assert_eq!(outcome.unsure_months, [] as [(i32, u32); 0]);
    let first = outcome.records.first().expect("a record");
    assert_eq!(first.machine, "番長");
    assert_eq!(first.outlay, Some(10_000));
    assert_eq!(first.takings, Some(-2_000));
    assert_eq!(first.memo, "番長\nmemo");
    // Both records of 3/5 were opened, each the right one.
    assert!(outcome.records.iter().any(|record| record.machine == "北斗" && record.revenue == -2_000));
}

#[test]
fn a_shallow_walk_reads_the_list_alone() {
    let mut phone = usual_phone();
    let outcome = rescue(&mut phone, walk(false), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 5);
    assert!(outcome.records.iter().all(|record| record.memo.is_empty() && record.outlay.is_none()));
}

#[test]
fn a_month_turn_that_does_not_take_is_tried_again_not_read_twice() {
    let mut phone = usual_phone();
    phone.swipes_ignored = 2;
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 5);
}

#[test]
fn a_record_opened_in_place_of_another_is_caught_and_the_month_read_again() {
    let mut phone = usual_phone();
    phone.taps_off_by_one = 1;
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 5);
    let hokuto = outcome.records.iter().find(|record| record.machine == "北斗").expect("北斗");
    assert_eq!(hokuto.memo, "北斗\nmemo");
}

#[test]
fn a_day_of_many_records_is_scrolled_through() {
    let many: Vec<Kept> = (0..17).map(|n| kept(8, &format!("台{n}"), i64::from(n) * 1_000 - 5_000)).collect();
    let mut phone = Phone::new((2026, 3)).with((2026, 3), many);
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 17);
    let mut machines: Vec<&str> = outcome.records.iter().map(|record| record.machine.as_str()).collect();
    machines.sort_unstable();
    machines.dedup();
    assert_eq!(machines.len(), 17);
    assert_eq!(outcome.unsure_months, [] as [(i32, u32); 0]);
}

#[test]
fn a_holiday_on_a_recorded_day_is_read_all_the_same() {
    let mut phone = Phone::new((2026, 3)).with((2026, 3), vec![kept(20, "海物語", 4_000)]);
    phone.holidays.insert((2026, 3, 20), "春分の日".to_owned());
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 1);
}

#[test]
fn a_recorded_day_selected_before_the_walk_is_read_without_opening_the_create_menu() {
    // ぱち簿 keeps the date last selected: here 3/5, which holds records. A
    // tap on it would open the 作成 menu, so another day is selected first.
    let mut phone = usual_phone();
    phone.selected = Some((2026, 3, 5));
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 5);
    assert_eq!(phone.menus_opened, 0);
}

#[test]
fn a_walk_from_where_the_app_was_left_turns_to_the_month_asked() {
    // Left on an earlier month, with a date of another month selected.
    let mut phone = usual_phone();
    phone.shown = (2025, 6);
    phone.selected = Some((2026, 1, 2));
    let outcome = rescue(&mut phone, Walk { start: Some((2026, 3)), ..walk(true) }, |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 5);
    assert_eq!(phone.menus_opened, 0);
}

#[test]
fn two_empty_months_in_a_row_are_walked_through() {
    // Records in March and again in December: January and February are empty.
    let mut phone = Phone::new((2026, 3))
        .with((2026, 3), vec![kept(5, "海物語", 10_000)])
        .with((2025, 12), vec![kept(31, "番長", -12_000), kept(1, "北斗", 3_000)]);
    let outcome = rescue(&mut phone, Walk { deep: true, ..Walk::default() }, |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(dates(&outcome), ["2025-12-01", "2025-12-31", "2026-03-05"]);
    assert_eq!(outcome.unsure_months, [] as [(i32, u32); 0]);
}

#[test]
fn a_phone_lost_partway_keeps_what_was_read() {
    let mut phone = usual_phone();
    phone.dumps_left = Some(12); // gone partway through, after March is read
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert!(matches!(outcome.end, End::Failed(Error::PhoneLost(_))), "{:?} {}", outcome.end, outcome.records.len());
    assert_ne!(outcome.records, [] as [Record; 0]);
    assert!(outcome.records.len() < 5);
}

#[test]
fn a_long_gap_ends_a_walk_with_no_count_but_not_one_still_short() {
    let phone = || Phone::new((2026, 3)).with((2026, 3), vec![kept(1, "海物語", 1_000)]).with((2024, 6), vec![kept(1, "番長", 2_000)]);
    let gap_walk = Walk { deep: false, ..Walk::default() };

    let mut unknowing = phone();
    let outcome = rescue(&mut unknowing, gap_walk, |_| {}, || false);
    assert_eq!(outcome.records.len(), 1); // 20 empty months: past the 12 it waits

    let mut knowing = phone();
    let expected = Walk { expected: Some(2), ..gap_walk };
    let outcome = rescue(&mut knowing, expected, |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records.len(), 2);
}

#[test]
fn the_walk_begins_from_the_month_asked() {
    let mut phone = usual_phone();
    let outcome = rescue(&mut phone, Walk { start: Some((2026, 1)), ..walk(false) }, |_| {}, || false);
    assert_eq!(dates(&outcome), ["2025-12-31", "2026-01-02"]);
}

#[test]
fn a_cancelled_walk_stops_and_returns_what_it_has() {
    let mut phone = usual_phone();
    let outcome = rescue(&mut phone, walk(true), |_| {}, || true);
    assert_eq!(outcome.end, End::Cancelled);
    assert_eq!(outcome.records, [] as [Record; 0]);
}

#[test]
fn a_survey_reads_the_lifetime_count_off_the_analysis_screen() {
    let mut phone = usual_phone();
    let found = survey(&mut phone, || false).expect("the survey runs");
    assert_eq!(found.records, Some(5));
    assert_eq!(phone.screen, Screen::Calendar); // came back, and no further
    assert!(!phone.steps.iter().any(|step| step.starts_with("swipe")));
}

#[test]
fn a_survey_stops_between_its_steps_when_asked() {
    let mut phone = usual_phone();
    let asked = std::cell::Cell::new(0);
    // Asked to stop once it has started ぱち簿: it taps nothing after.
    let result = survey(&mut phone, || {
        asked.set(asked.get() + 1);
        asked.get() > 1
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(!phone.steps.iter().any(|step| step.starts_with("tap")));
}

#[test]
fn a_survey_of_one_game_only_is_left_unknown() {
    let mut phone = usual_phone();
    phone.analysis_narrowed = true;
    assert_eq!(survey(&mut phone, || false).expect("the survey runs").records, None);
}

// ---------------------------------------------------------------------------
// Scripted dumps, built to the shape of the real ones above.
// ---------------------------------------------------------------------------

const FILLED_EDITOR: &str = r#"<hierarchy>
<node resource-id="jp.garakuta.pachibo:id/date" text="2026年3月5日(木)" bounds="[0,0][100,20]"/>
<node resource-id="jp.garakuta.pachibo:id/hall" text="マルハン新宿" bounds="[0,20][100,40]"/>
<node resource-id="jp.garakuta.pachibo:id/machine" text="スーパー海物語" bounds="[0,40][100,60]"/>
<node resource-id="jp.garakuta.pachibo:id/invest" text="20,000" bounds="[0,60][100,80]"/>
<node resource-id="jp.garakuta.pachibo:id/invest_mode" text="円" bounds="[100,60][140,80]"/>
<node resource-id="jp.garakuta.pachibo:id/recovery" text="30,000" bounds="[0,80][100,100]"/>
<node resource-id="jp.garakuta.pachibo:id/recovery_mode" text="円" bounds="[100,80][140,100]"/>
<node resource-id="jp.garakuta.pachibo:id/revenue" text="+10,000" bounds="[0,100][100,120]"/>
<node resource-id="jp.garakuta.pachibo:id/time" text="3時間00分" bounds="[0,120][100,140]"/>
<node resource-id="jp.garakuta.pachibo:id/number" text="0123" bounds="[0,140][100,160]"/>
<node resource-id="jp.garakuta.pachibo:id/tag" text="朝一" bounds="[0,160][100,180]"/>
<node resource-id="jp.garakuta.pachibo:id/memo" text="good&#10;day" bounds="[0,180][100,200]"/>
<node resource-id="jp.garakuta.pachibo:id/save" text="保存" bounds="[0,200][100,220]"/>
</hierarchy>"#;

const BALLS_EDITOR: &str = r#"<hierarchy>
<node resource-id="jp.garakuta.pachibo:id/date" text="2026年3月6日(金)" bounds="[0,0][100,20]"/>
<node resource-id="jp.garakuta.pachibo:id/invest" text="1,500" bounds="[0,60][100,80]"/>
<node resource-id="jp.garakuta.pachibo:id/invest_mode" text="玉" bounds="[100,60][140,80]"/>
<node resource-id="jp.garakuta.pachibo:id/recovery" text="2,000" bounds="[0,80][100,100]"/>
<node resource-id="jp.garakuta.pachibo:id/recovery_mode" text="玉" bounds="[100,80][140,100]"/>
<node resource-id="jp.garakuta.pachibo:id/revenue" text="+8,000" bounds="[0,100][100,120]"/>
<node resource-id="jp.garakuta.pachibo:id/save" text="保存" bounds="[0,200][100,220]"/>
</hierarchy>"#;

#[test]
fn a_walk_stopped_partway_through_a_day_stops_there() {
    let many: Vec<Kept> = (0..17).map(|n| kept(8, &format!("台{n}"), 1_000)).collect();
    let mut phone = Phone::new((2026, 3)).with((2026, 3), many);
    let asked = std::cell::Cell::new(0);
    let outcome = rescue(
        &mut phone,
        walk(true),
        |_| {},
        || {
            asked.set(asked.get() + 1);
            asked.get() > 5
        },
    );
    assert_eq!(outcome.end, End::Cancelled);
    assert!(outcome.records.len() < 17, "it read on to {} records", outcome.records.len());
}

#[test]
fn a_phone_gone_ends_the_walk_at_once_rather_than_trying_again() {
    let mut phone = usual_phone();
    phone.dumps_left = Some(12);
    let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
    assert!(matches!(outcome.end, End::Failed(Error::PhoneLost(_))));
    // No month was tried again once the phone was gone: it is final.
    assert!(!phone.notes.iter().any(|note| note.contains("again")), "{:?}", phone.notes);
    assert!(Error::PhoneLost(String::new()).is_final());
    assert!(Error::AdbMissing(String::new()).is_final());
    assert!(!Error::Step(String::new()).is_final());
}

#[test]
fn a_step_that_fails_partway_is_met_by_reading_the_month_again() {
    let whole = rescue(&mut usual_phone(), walk(true), |_| {}, || false);
    // Into March's first day, two dumps in a row fail as uiautomator killed
    // (the adb device has already tried each five times): the month is read
    // again from the app brought up again, and the walk reads it all.
    let mut flaky = FlakyAfter { phone: usual_phone(), after: 4, failures: 2 };
    let outcome = rescue(&mut flaky, walk(true), |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records, whole.records);
    assert!(flaky.phone.notes.iter().any(|note| note.contains("did not land")), "{:?}", flaky.phone.notes);
}

#[test]
fn a_month_that_fails_every_try_ends_the_walk_keeping_what_was_read() {
    // March read whole; then, in January, every one of the walk's tries fails.
    let tries = Walk::default().attempts;
    let mut flaky = FlakyAfter { phone: usual_phone(), after: 12, failures: tries };
    let outcome = rescue(&mut flaky, walk(true), |_| {}, || false);
    assert!(matches!(outcome.end, End::Failed(Error::Step(_))), "{:?}", outcome.end);
    assert_eq!(dates(&outcome), ["2026-03-05", "2026-03-05", "2026-03-20"]);
}

#[test]
fn a_survey_whose_analysis_screen_will_not_read_goes_on_without_a_count() {
    let mut flaky = FlakyAfter { phone: usual_phone(), after: 1, failures: 5 };
    let found = survey(&mut flaky, || false).expect("the survey does not fail the reading");
    assert_eq!(found.records, None);
    assert!(flaky.phone.notes.iter().any(|note| note.contains("could not be read")), "{:?}", flaky.phone.notes);
}

#[test]
fn a_survey_with_the_phone_gone_fails() {
    let mut phone = usual_phone();
    phone.dumps_left = Some(1);
    assert!(matches!(survey(&mut phone, || false), Err(Error::PhoneLost(_))));
}

/// The pretend phone, whose dumps fail as a step `failures` times in a row
/// once `after` have been answered (as uiautomator being killed would).
struct FlakyAfter {
    phone: Phone,
    after: usize,
    failures: u32,
}

impl Device for FlakyAfter {
    fn launch_calendar(&mut self) -> Result<(), Error> {
        self.phone.launch_calendar()
    }
    fn dump(&mut self) -> Result<String, Error> {
        if self.after == 0 && self.failures > 0 {
            self.failures -= 1;
            return Err(Error::Step("adb shell uiautomator dump: exit 137: Killed".to_owned()));
        }
        self.after = self.after.saturating_sub(1);
        self.phone.dump()
    }
    fn tap(&mut self, x: i32, y: i32) -> Result<(), Error> {
        self.phone.tap(x, y)
    }
    fn back(&mut self) -> Result<(), Error> {
        self.phone.back()
    }
    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), millis: u32) -> Result<(), Error> {
        self.phone.swipe(from, to, millis)
    }
    fn note(&mut self, what: &str) {
        self.phone.note(what);
    }
}

#[test]
fn a_slow_pachibo_is_waited_for_not_tapped_or_sent_back_twice() {
    // Grown slow, ぱち簿 takes a dump or two to open an editor and to leave
    // it: the walk looks again rather than tapping again (which could land in
    // the editor) or sending back again (which could close the app).
    let whole = rescue(&mut usual_phone(), walk(true), |_| {}, || false);
    for slow in [1, 2] {
        let mut phone = usual_phone();
        phone.slow = slow;
        let outcome = rescue(&mut phone, walk(true), |_| {}, || false);
        assert_eq!(outcome.end, End::Finished, "slow {slow}");
        assert_eq!(outcome.records, whole.records, "slow {slow}");
        assert!(!phone.notes.iter().any(|note| note.contains("did not land")), "slow {slow}: {:?}", phone.notes);
    }
}

#[test]
fn pachibo_started_afresh_between_months_reads_the_same() {
    let whole = rescue(&mut usual_phone(), walk(true), |_| {}, || false);
    let mut phone = usual_phone();
    let outcome = rescue(&mut phone, Walk { fresh_start_every: Some(std::time::Duration::ZERO), ..walk(true) }, |_| {}, || false);
    assert_eq!(outcome.end, End::Finished);
    assert_eq!(outcome.records, whole.records);
    assert!(phone.steps.iter().filter(|step| *step == "restart").count() >= 3);
}
