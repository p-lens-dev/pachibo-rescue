//! ぱち簿's records read off an Android phone's screen over adb.
//!
//! ぱち簿 (`jp.garakuta.pachibo`) keeps its records in the app's own private
//! storage, which a non-rooted phone will not let out: its `.pcb`/CSV export
//! is paid-only and, on some phones, broken. What the phone *will* show is the
//! screen. This crate reads the screen — the calendar and each record's editor
//! (`GameResult`) — as `uiautomator` describes it, and writes the records as a
//! CSV that P-Lens takes in at 設定 → インポート → 汎用形式.
//!
//! The parsing here is pure: it turns a `uiautomator` XML dump into records.
//! The driving of the phone is left to a [`Device`], so the walk ([`rescue`])
//! can be tested against scripted dumps and run for real over adb
//! ([`adb::Adb`]).
//!
//! The walk trusts no step to have landed. Each month turned is checked to be
//! the month before; each day tapped is checked to be the day selected; each
//! record opened is checked against the row it was opened from, and the list
//! is checked to be where it was on coming back; each month's results are
//! checked to add up to the month's total. A step that did not land is tried
//! again from the app brought up again, and a walk that cannot go on still
//! gives back every record it read.

pub mod adb;
mod dumper;
#[cfg(test)]
mod tests;

/// Why a rescue could not be carried out, or could not be carried on.
///
/// Some are final — there is no use going on ([`Error::is_final`]): adb
/// cannot be run, the phone is gone, or the caller stopped. The rest are a
/// step that did not land, which the walk meets by reading the month again
/// from the app brought up again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// adb could not be run at all (not installed, not executable): what the
    /// system said.
    AdbMissing(String),
    /// The phone cannot be reached — unplugged, offline, or no longer
    /// allowing this computer: what adb said.
    PhoneLost(String),
    /// A step on the phone failed: the command, how it ended, and what it said.
    Step(String),
    /// The app's calendar could not be brought on screen.
    NoCalendar,
    /// The calendar would not turn to this month.
    MonthStuck {
        /// The year sought.
        year: i32,
        /// The month sought.
        month: u32,
    },
    /// This day (`YYYY-MM-DD`) could not be selected on the calendar.
    DayNotSelected(String),
    /// A record of this day would not open.
    RecordNotOpened(String),
    /// A record of this day opened as some other record than the row tapped.
    RecordMismatch(String),
    /// Coming back from a record of this day did not find the day's list as
    /// it was left.
    NotBack(String),
    /// The caller stopped the walk partway through a step.
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AdbMissing(detail) => write!(f, "adb could not be run: {detail}"),
            Self::PhoneLost(detail) => write!(f, "the phone cannot be reached: {detail}"),
            Self::Step(detail) => write!(f, "a step on the phone failed: {detail}"),
            Self::NoCalendar => write!(f, "ぱち簿's calendar was not on screen"),
            Self::MonthStuck { year, month } => write!(f, "the calendar would not turn to {year:04}-{month:02}"),
            Self::DayNotSelected(date) => write!(f, "{date} could not be selected"),
            Self::RecordNotOpened(date) => write!(f, "a record of {date} would not open"),
            Self::RecordMismatch(date) => write!(f, "a record of {date} opened as another record"),
            Self::NotBack(date) => write!(f, "coming back from a record of {date} did not find the day as it was"),
            Self::Cancelled => write!(f, "the walk was stopped"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Whether there is no use going on: adb cannot be run, the phone is
    /// gone, or the caller stopped. Any other error is a step that did not
    /// land, worth trying again from the app brought up again.
    #[must_use]
    pub fn is_final(&self) -> bool {
        matches!(self, Self::AdbMissing(_) | Self::PhoneLost(_) | Self::Cancelled)
    }
}

/// ぱち簿's bundle id on the store.
pub const PACKAGE: &str = "jp.garakuta.pachibo";

/// The calendar activity, the one a launch brings up.
pub const CALENDAR_ACTIVITY: &str = "jp.garakuta.pachibo/.CalendarView";

// ---------------------------------------------------------------------------
// The dump, scanned into the nodes and attributes we read.
// ---------------------------------------------------------------------------

/// A rectangle on screen: left, top, right, bottom.
type Bounds = (i32, i32, i32, i32);

/// One `<node …>` element's attributes, as far as we read them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Node {
    /// The short resource id, what follows `…:id/`, where there is one.
    id: Option<String>,
    /// The `text` attribute, unescaped, where there is one.
    text: Option<String>,
    /// `[left,top][right,bottom]`, where it reads.
    bounds: Option<Bounds>,
    /// The `checked` attribute, where there is one.
    checked: Option<bool>,
}

impl Node {
    /// The middle of the node's bounds, for a tap.
    fn center(&self) -> Option<(i32, i32)> {
        self.bounds.map(|(l, t, r, b)| (i32::midpoint(l, r), i32::midpoint(t, b)))
    }

    /// Whether the node has this short id.
    fn is(&self, id: &str) -> bool {
        self.id.as_deref() == Some(id)
    }
}

/// A node's attributes, in order, each `name="value"` or `name='value'`,
/// their values unescaped. They are read one after another from the start,
/// so a value that holds what looks like another attribute (`text="…"`) is
/// never taken for it. uiautomator quotes a value that holds `"` with `'`
/// (seen on the phone: a memo `say "hi"` came as `text='say "hi"'`).
fn attributes(tag: &str) -> Vec<(&str, String)> {
    let mut found = Vec::new();
    let mut rest = tag;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        let Some(equals) = rest.find('=') else {
            break;
        };
        let name = rest.get(..equals).unwrap_or_default().trim();
        let after = rest.get(equals + 1..).unwrap_or_default().trim_start();
        let Some(quote) = after.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            break;
        };
        let body = after.get(quote.len_utf8()..).unwrap_or_default();
        let Some(end) = body.find(quote) else {
            break;
        };
        found.push((name, unescape(body.get(..end).unwrap_or_default())));
        rest = body.get(end + quote.len_utf8()..).unwrap_or_default();
    }
    found
}

/// Where a tag's text ends: the first `>` outside a quoted value (a memo may
/// hold `>`).
fn tag_end(text: &str) -> usize {
    let mut quote: Option<char> = None;
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), c) if c == open => quote = None,
            (None, '>') => return at,
            _ => {}
        }
    }
    text.len()
}

/// XML's escapes turned back: the five named ones and numeric ones (`&#10;`,
/// `&#x41;`), which `uiautomator` writes for a memo's line breaks. One pass,
/// so a written `&amp;lt;` stays `&lt;` rather than becoming `<`.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or(""));
        let after = rest.get(at..).unwrap_or("");
        let decoded = after.find(';').and_then(|end| {
            let name = after.get(1..end)?;
            let ch = match name {
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "amp" => '&',
                _ => {
                    let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                        u32::from_str_radix(hex, 16).ok()?
                    } else {
                        name.strip_prefix('#')?.parse().ok()?
                    };
                    char::from_u32(code)?
                }
            };
            Some((ch, end + 1))
        });
        if let Some((ch, used)) = decoded {
            out.push(ch);
            rest = after.get(used..).unwrap_or("");
        } else {
            out.push('&');
            rest = after.get(1..).unwrap_or("");
        }
    }
    out.push_str(rest);
    out
}

/// `jp.garakuta.pachibo:id/day` read as `day`; anything with no `/` kept whole.
fn short_id(resource_id: &str) -> String {
    resource_id.rsplit('/').next().unwrap_or(resource_id).to_owned()
}

/// `[l,t][r,b]` read as four numbers.
fn parse_bounds(text: &str) -> Option<Bounds> {
    let numbers: Vec<i32> = text
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .filter(|piece| !piece.is_empty())
        .filter_map(|piece| piece.parse().ok())
        .collect();
    match numbers.as_slice() {
        [l, t, r, b] => Some((*l, *t, *r, *b)),
        _ => None,
    }
}

/// Every `<node …>` of a dump, in the order the dump writes them — which is the
/// order the screen nests them, so a cell's own nodes follow it.
fn scan(xml: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<node") {
        let after = rest.get(at + "<node".len()..).unwrap_or("");
        let end = tag_end(after);
        let tag = after.get(..end).unwrap_or("");
        let found = attributes(tag);
        let attr = |key: &str| found.iter().find(|(name, _)| *name == key).map(|(_, value)| value.clone());
        nodes.push(Node {
            id: attr("resource-id").as_deref().map(short_id),
            text: attr("text").filter(|value| !value.is_empty()),
            bounds: attr("bounds").as_deref().and_then(parse_bounds),
            checked: attr("checked").map(|value| value == "true"),
        });
        rest = after.get(end..).unwrap_or("");
    }
    nodes
}

/// The first `text` of a node with the given short id.
fn first_text(nodes: &[Node], id: &str) -> Option<String> {
    nodes.iter().find(|node| node.is(id)).and_then(|node| node.text.clone())
}

/// The bounds of the first node with the given short id.
fn first_bounds(nodes: &[Node], id: &str) -> Option<Bounds> {
    nodes.iter().find(|node| node.is(id)).and_then(|node| node.bounds)
}

/// Whether the create menu (作成: 結果作成/イベント作成/スケジュール作成), which a
/// tap on the day already selected opens, is over the screen. A tap on it would make a record,
/// so it is always closed before anything else is tapped.
///
/// The menu's items are rows with an `id=name`. The calendar's own bar of
/// buttons carries a 結果作成 too (`id=create`), always, so the text alone
/// does not mark the menu: taking it so pressed back on the bare calendar,
/// which closes the app.
fn menu_open(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| node.is("name") && matches!(node.text.as_deref().map(str::trim), Some("結果作成" | "イベント作成")))
}

/// A screen as `uiautomator` describes it, for a tool that drives ぱち簿
/// step by step (such as the seeder of test records, `examples/seed.rs`):
/// its views by id and by text, with where to tap them.
pub mod screen {
    use super::{Node, scan};

    /// One view on a screen.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct View {
        /// Its short resource id (what follows `…:id/`), where it has one.
        pub id: Option<String>,
        /// Its text, where it shows any.
        pub text: Option<String>,
        /// Its middle, to tap.
        pub center: Option<(i32, i32)>,
        /// Whether it is checked, where it can be.
        pub checked: Option<bool>,
    }

    /// A screen, its views in the order the dump writes them.
    #[derive(Debug, Clone, Default)]
    pub struct Screen {
        /// The views.
        nodes: Vec<Node>,
    }

    impl Screen {
        /// The screen a dump describes.
        #[must_use]
        pub fn parse(xml: &str) -> Self {
            Self { nodes: scan(xml) }
        }

        /// Every view, in order.
        #[must_use]
        pub fn views(&self) -> Vec<View> {
            self.nodes.iter().map(|node| View { id: node.id.clone(), text: node.text.clone(), center: node.center(), checked: node.checked }).collect()
        }

        /// The views with this short id, in order.
        #[must_use]
        pub fn with_id(&self, id: &str) -> Vec<View> {
            self.views().into_iter().filter(|view| view.id.as_deref() == Some(id)).collect()
        }

        /// The views showing exactly this text, in order.
        #[must_use]
        pub fn with_text(&self, text: &str) -> Vec<View> {
            self.views().into_iter().filter(|view| view.text.as_deref().map(str::trim) == Some(text)).collect()
        }

        /// The text of the first view with this short id, where it shows any.
        #[must_use]
        pub fn text_of(&self, id: &str) -> Option<String> {
            super::first_text(&self.nodes, id)
        }

        /// Whether a view with this short id is on screen.
        #[must_use]
        pub fn has(&self, id: &str) -> bool {
            self.nodes.iter().any(|node| node.is(id))
        }
    }
}

// ---------------------------------------------------------------------------
// Numbers and dates as the screen writes them.
// ---------------------------------------------------------------------------

/// A yen amount as the screen writes it (`+127,000`, `-3,000`, `0`, `¥1,200`),
/// read as a whole number; none where nothing of it reads as a number.
#[must_use]
pub fn yen_of(text: &str) -> Option<i64> {
    let kept: String = text
        .chars()
        .map(|c| if matches!(c, '−' | '－' | '▲') { '-' } else { c })
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    if kept.is_empty() || kept == "-" {
        return None;
    }
    kept.parse().ok()
}

/// `2026年11月17日(火)` or `2026年11月17日` read as `(year, month, day)`.
fn parse_jp_date(text: &str) -> Option<(i32, u32, u32)> {
    let year_rest = text.split_once('年')?;
    let month_rest = year_rest.1.split_once('月')?;
    let day_rest = month_rest.1.split_once('日')?;
    Some((year_rest.0.trim().parse().ok()?, month_rest.0.trim().parse().ok()?, day_rest.0.trim().parse().ok()?))
}

/// `2026-11-17` from its parts.
#[must_use]
pub fn iso_date(year: i32, month: u32, day: u32) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

/// The weekdays as the calendar writes them, Sunday first.
const WEEKDAYS: &str = "日月火水木金土";

/// The weekday of a date, Sunday as 0 (Sakamoto's method).
pub(crate) fn weekday_of(year: i32, month: u32, day: u32) -> usize {
    const OFFSETS: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let year = i64::from(year) - i64::from(month < 3);
    let offset = usize::try_from(month).ok().and_then(|month| OFFSETS.get(month.wrapping_sub(1))).copied().unwrap_or(0);
    let weekday = (year + year.div_euclid(4) - year.div_euclid(100) + year.div_euclid(400) + offset + i64::from(day)).rem_euclid(7);
    usize::try_from(weekday).unwrap_or(0)
}

/// A month as a count of months, for stepping and comparing.
fn month_index((year, month): (i32, u32)) -> i64 {
    i64::from(year) * 12 + i64::from(month) - 1
}

/// The month before.
fn month_before((year, month): (i32, u32)) -> (i32, u32) {
    if month <= 1 { (year - 1, 12) } else { (year, month - 1) }
}

// ---------------------------------------------------------------------------
// The calendar: the month shown and each day's cell.
// ---------------------------------------------------------------------------

/// A day's cell on the calendar: its number, where to tap to select it, and the
/// one record the cell shows (a day may hold more, read from the list once
/// selected).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The day of the month.
    pub day: u32,
    /// Where to tap to select the day, best first: the cell's text, then its
    /// result, then the cell's middle (a tap anywhere on a cell selects its
    /// day — but on the day already selected, opens the create menu).
    pub taps: Vec<(i32, i32)>,
    /// The cell's middle.
    pub center: (i32, i32),
    /// The text the cell shows (the machine, or a holiday's name).
    pub machine: Option<String>,
    /// The day's result the cell shows, in yen.
    pub revenue: Option<i64>,
}

impl Cell {
    /// Whether the cell shows a record (and so is worth selecting). A record
    /// always shows its result; a day that only names a holiday (an `id=text`
    /// too) shows no amount, so the amount, not the text, is the mark.
    #[must_use]
    pub fn has_record(&self) -> bool {
        self.revenue.is_some()
    }
}

/// The calendar as one dump shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    /// The year shown.
    pub year: i32,
    /// The month shown, 1–12.
    pub month: u32,
    /// The day cells, in the order the dump writes them.
    pub cells: Vec<Cell>,
    /// The month's total result (`id=total`), where it reads.
    pub total: Option<i64>,
    /// The selected day as the list's header writes it (`11月11日(水)`).
    pub selected: Option<String>,
    /// The month pager's bounds, to swipe across to turn the month.
    pub pager: Option<(i32, i32, i32, i32)>,
    /// The middle of the 分析 (analysis) button.
    pub analysis: Option<(i32, i32)>,
    /// The middle of the 今月 button, which turns the calendar to this month
    /// (by the phone's clock) at a tap.
    pub this_month: Option<(i32, i32)>,
    /// The middle of the header's year and month (`id=date`), which opens a
    /// picker of year and month.
    pub header: Option<(i32, i32)>,
}

impl Calendar {
    /// The cells that show a record.
    #[must_use]
    pub fn recorded_cells(&self) -> Vec<Cell> {
        self.cells.iter().filter(|cell| cell.has_record()).cloned().collect()
    }

    /// The month shown, as `(year, month)`.
    #[must_use]
    pub fn shown(&self) -> (i32, u32) {
        (self.year, self.month)
    }

    /// Whether the list below shows this day of the month shown.
    ///
    /// The header (`11月 5日(木)`) pads a one-digit day, so spaces are not
    /// compared. It keeps the date selected when the month is turned, even to
    /// the same month of another year, and names no year: so its weekday is
    /// compared too, which tells 2026's 11月18日(水) from 2025's (a Tuesday).
    #[must_use]
    pub fn is_selected(&self, day: u32) -> bool {
        let Some(header) = self.selected.as_deref() else {
            return false;
        };
        let header: String = header.chars().filter(|c| !c.is_whitespace()).collect();
        let Some(rest) = header.strip_prefix(&format!("{}月{day}日", self.month)) else {
            return false;
        };
        match rest.strip_prefix('(').and_then(|after| after.chars().next()) {
            Some(named) => WEEKDAYS.chars().position(|weekday| weekday == named) == Some(weekday_of(self.year, self.month, day)),
            // No weekday where one is always written: not this header.
            None => false,
        }
    }

    /// Where to swipe to turn the month: a horizontal stroke across the
    /// pager's middle (or, where the pager does not read, the cells').
    /// Left-to-right shows the month before.
    fn turn_stroke(&self, earlier: bool) -> Option<((i32, i32), (i32, i32))> {
        let (left, top, right, bottom) = self.pager.or_else(|| {
            let bounds: Vec<(i32, i32)> = self.cells.iter().flat_map(|cell| cell.taps.iter().copied()).collect();
            let left = bounds.iter().map(|point| point.0).min()?;
            let right = bounds.iter().map(|point| point.0).max()?;
            let top = bounds.iter().map(|point| point.1).min()?;
            let bottom = bounds.iter().map(|point| point.1).max()?;
            Some((left, top, right, bottom))
        })?;
        let width = right - left;
        if width <= 0 {
            return None;
        }
        let y = i32::midpoint(top, bottom);
        let (near, far) = (left + width / 5, right - width / 5);
        Some(if earlier { ((near, y), (far, y)) } else { ((far, y), (near, y)) })
    }
}

/// The calendar read from a dump, or none where the calendar is not on screen.
///
/// The header carries the year and month (`id=year`, `id=month`) and the
/// month's total (`id=total`). Each day is a clickable cell (`id=layout`)
/// whose own nodes follow it: the day number (`id=day`), and, where the day
/// holds a record, the machine (`id=text`) and the result (`id=payment`).
/// Days of the months either side are cells with no number, and are left out.
#[must_use]
pub fn parse_calendar(xml: &str) -> Option<Calendar> {
    calendar_of(&scan(xml))
}

/// The calendar from scanned nodes; see [`parse_calendar`].
fn calendar_of(nodes: &[Node]) -> Option<Calendar> {
    let year = first_text(nodes, "year").and_then(|text| text.trim().parse().ok())?;
    let month: u32 = first_text(nodes, "month").and_then(|text| text.trim().parse().ok())?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let mut cells: Vec<Cell> = Vec::new();
    for node in nodes {
        match node.id.as_deref() {
            Some("layout") => {
                if let Some(center) = node.center() {
                    cells.push(Cell { day: 0, taps: Vec::new(), center, machine: None, revenue: None });
                }
            }
            Some("day") => {
                if let (Some(cell), Some(day)) = (cells.last_mut(), node.text.as_deref().and_then(|t| t.trim().parse().ok())) {
                    cell.day = day;
                }
            }
            Some("text") => {
                if let Some(cell) = cells.last_mut()
                    && node.text.is_some()
                {
                    if cell.machine.is_none() {
                        cell.machine.clone_from(&node.text);
                    }
                    cell.taps.extend(node.center());
                }
            }
            Some("payment") => {
                if let Some(cell) = cells.last_mut()
                    && cell.revenue.is_none()
                    && let Some(revenue) = node.text.as_deref().and_then(yen_of)
                {
                    cell.revenue = Some(revenue);
                    cell.taps.extend(node.center());
                }
            }
            _ => {}
        }
    }
    let cells = cells
        .into_iter()
        .filter(|cell| cell.day > 0)
        .map(|cell| Cell { taps: cell.taps.iter().copied().chain(std::iter::once(cell.center)).collect(), ..cell })
        .collect();
    Some(Calendar {
        year,
        month,
        cells,
        total: first_text(nodes, "total").as_deref().and_then(yen_of),
        selected: first_text(nodes, "select"),
        pager: first_bounds(nodes, "viewpager"),
        analysis: nodes.iter().find(|node| node.is("analysis")).and_then(Node::center),
        this_month: nodes.iter().find(|node| node.is("this_month")).and_then(Node::center),
        header: nodes.iter().find(|node| node.is("date")).and_then(Node::center),
    })
}

// ---------------------------------------------------------------------------
// The day's list: each record on the selected day.
// ---------------------------------------------------------------------------

/// A record as the selected day's list shows it: the machine, the parlor and
/// the result, and where to tap to open it for the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    /// The machine's name.
    pub machine: String,
    /// The parlor's name.
    pub parlor: String,
    /// The result, in yen.
    pub revenue: i64,
    /// The middle of the machine's name, to tap to open the record.
    pub center: (i32, i32),
    /// The row's top edge on screen.
    pub top: i32,
    /// The row's bottom edge on screen.
    pub bottom: i32,
}

impl ListRow {
    /// What the row shows, without where: to match rows across a scroll.
    fn shows(&self) -> (&str, &str, i64) {
        (&self.machine, &self.parlor, self.revenue)
    }
}

/// The records on the selected day, read from the list at the foot of the
/// calendar. A row opens with the machine (`id=name`) and carries the parlor
/// (`id=hall`) and the result (`id=revenue`); the bar of buttons below the list
/// carries an `id=hall` too, but by then every row already has its parlor, so
/// it is left out.
#[must_use]
pub fn parse_day_list(xml: &str) -> Vec<ListRow> {
    day_list_of(&scan(xml))
}

/// The day's list from scanned nodes; see [`parse_day_list`].
fn day_list_of(nodes: &[Node]) -> Vec<ListRow> {
    /// A row as it is read: a record has a result (`id=revenue`); the create
    /// menu's items open with an `id=name` too but have none, so are left out.
    struct Building {
        row: ListRow,
        is_record: bool,
    }
    let mut rows: Vec<Building> = Vec::new();
    for node in nodes {
        match node.id.as_deref() {
            Some("name") => {
                if let (Some(machine), Some(center), Some((_, top, _, bottom))) = (node.text.clone(), node.center(), node.bounds) {
                    rows.push(Building { row: ListRow { machine, parlor: String::new(), revenue: 0, center, top, bottom }, is_record: false });
                }
            }
            Some("hall") => {
                if let Some(building) = rows.last_mut()
                    && building.row.parlor.is_empty()
                    && let Some(parlor) = node.text.clone()
                {
                    building.row.parlor = parlor;
                    if let Some((_, _, _, bottom)) = node.bounds {
                        building.row.bottom = building.row.bottom.max(bottom);
                    }
                }
            }
            Some("revenue") => {
                if let Some(building) = rows.last_mut()
                    && !building.is_record
                    && let Some(value) = node.text.as_deref().and_then(yen_of)
                {
                    building.row.revenue = value;
                    building.is_record = true;
                    if let Some((_, _, _, bottom)) = node.bounds {
                        building.row.bottom = building.row.bottom.max(bottom);
                    }
                }
            }
            _ => {}
        }
    }
    rows.into_iter().filter(|building| building.is_record).map(|building| building.row).collect()
}

/// Whether the list may hold rows below the last one shown: there is no room
/// below the last row for another.
fn more_below(nodes: &[Node], rows: &[ListRow]) -> bool {
    let (Some((_, _, _, list_bottom)), Some(first), Some(last)) = (first_bounds(nodes, "list"), rows.first(), rows.last()) else {
        return false;
    };
    let row_height = (first.bottom - first.top).max(1);
    last.bottom + row_height > list_bottom
}

/// The rows of `now` that `before` did not show, where `now` is the list
/// scrolled on from `before`: the longest run that ends `before` and begins
/// `now` is the overlap.
fn rows_after(before: &[ListRow], now: &[ListRow]) -> Vec<ListRow> {
    let most = before.len().min(now.len());
    let overlap = (0..=most)
        .rev()
        .find(|&k| {
            let tail = before.get(before.len() - k..).unwrap_or_default();
            let head = now.get(..k).unwrap_or_default();
            tail.iter().map(ListRow::shows).eq(head.iter().map(ListRow::shows))
        })
        .unwrap_or(0);
    now.get(overlap..).unwrap_or_default().to_vec()
}

// ---------------------------------------------------------------------------
// The record editor: a record's full fields.
// ---------------------------------------------------------------------------

/// One rescued record, as much of it as the screen gives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    /// The day, `YYYY-MM-DD`.
    pub date: String,
    /// The parlor's name.
    pub parlor: String,
    /// The machine's name.
    pub machine: String,
    /// What went in, in yen, where the record keeps it in yen (not balls/medals).
    pub outlay: Option<i64>,
    /// What came back, in yen, where the record keeps it in yen.
    pub takings: Option<i64>,
    /// The result, in yen.
    pub revenue: i64,
    /// The cabinet number, where the record has one.
    pub cabinet: String,
    /// The hours, as the editor writes them.
    pub hours: String,
    /// The memo.
    pub memo: String,
    /// The labels (ぱち簿's tags).
    pub tag: String,
}

/// A record read from its editor (`GameResult`) dump: each field by its id;
/// none where the dump is not of the editor (no 保存 button, or no date).
///
/// Money is `円` in most records; where a record keeps what went in or came back
/// as balls or medals (`玉`/`枚`) instead, that side is left out (the result,
/// always in yen, is still read), so a count is never mistaken for yen; the
/// CSV works that side out from the result ([`Record::sides`]).
#[must_use]
pub fn parse_record(xml: &str) -> Option<Record> {
    editor_of(&scan(xml))
}

/// The record from scanned nodes; see [`parse_record`].
fn editor_of(nodes: &[Node]) -> Option<Record> {
    if !nodes.iter().any(|node| node.is("save")) {
        return None;
    }
    let text = |id: &str| first_text(nodes, id).unwrap_or_default().trim().to_owned();
    let (year, month, day) = parse_jp_date(&text("date"))?;
    let yen_side = |value_id: &str, mode_id: &str| {
        if first_text(nodes, mode_id).as_deref().map(str::trim) == Some("円") {
            yen_of(&text(value_id))
        } else {
            None
        }
    };
    let cabinet = text("number");
    Some(Record {
        date: iso_date(year, month, day),
        parlor: text("hall"),
        machine: text("machine"),
        outlay: yen_side("invest", "invest_mode"),
        takings: yen_side("recovery", "recovery_mode"),
        revenue: yen_of(&text("revenue")).unwrap_or_default(),
        cabinet: if cabinet.trim_matches('0').is_empty() { String::new() } else { cabinet },
        hours: text("time"),
        memo: text("memo"),
        tag: text("tag"),
    })
}

// ---------------------------------------------------------------------------
// The records as a CSV P-Lens takes in.
// ---------------------------------------------------------------------------

impl Record {
    /// What went in and what came back, in yen, for the CSV: P-Lens takes a
    /// row in only with both. A side ぱち簿 keeps in balls or medals (or that
    /// a shallow read did not open) is worked out from the result, which
    /// ぱち簿 keeps in yen, as `takings - outlay`: from the other side where
    /// that is known and the side comes out not below zero; otherwise, as
    /// P-Lens takes a result alone, a loss as what went in with nothing
    /// back, a gain as nothing in and the gain back.
    #[must_use]
    pub fn sides(&self) -> (i64, i64) {
        let alone = (self.revenue.saturating_neg().max(0), self.revenue.max(0));
        match (self.outlay, self.takings) {
            (Some(outlay), Some(takings)) => (outlay, takings),
            (None, Some(takings)) => takings.checked_sub(self.revenue).filter(|outlay| *outlay >= 0).map_or(alone, |outlay| (outlay, takings)),
            (Some(outlay), None) => outlay.checked_add(self.revenue).filter(|takings| *takings >= 0).map_or(alone, |takings| (outlay, takings)),
            (None, None) => alone,
        }
    }
}

/// The records as a CSV for P-Lens's 汎用形式: a byte order mark so a desk
/// reads it as UTF-8, CRLF lines (RFC 4180), and a header of the names P-Lens
/// offers each column by, so the columns map themselves. Every row has both
/// what went in and what came back ([`Record::sides`]), as P-Lens needs.
#[must_use]
pub fn csv_of(records: &[Record]) -> Vec<u8> {
    let mut out = String::from("\u{feff}");
    out.push_str("日付,ホール,機種,投資,回収,収支,台番,時間,メモ,タグ\r\n");
    for record in records {
        let (outlay, takings) = record.sides();
        let fields = [
            record.date.as_str(),
            record.parlor.as_str(),
            record.machine.as_str(),
            &outlay.to_string(),
            &takings.to_string(),
            &record.revenue.to_string(),
            record.cabinet.as_str(),
            record.hours.as_str(),
            record.memo.as_str(),
            record.tag.as_str(),
        ];
        for (at, field) in fields.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            out.push_str(&csv_field(field));
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

/// A cell quoted where it holds a comma, a quote or a line break, its quotes
/// doubled (RFC 4180).
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

// ---------------------------------------------------------------------------
// Driving the phone: the walk, over a device left to the caller.
// ---------------------------------------------------------------------------

/// A phone to drive, each step as adb would: the walk asks for these and the
/// caller carries them out (over adb for real, scripted in a test). Each step
/// waits for the screen to settle before it returns.
pub trait Device {
    /// Bring ぱち簿 up on its calendar, leaving whatever was over it; the month
    /// shown may be any.
    ///
    /// # Errors
    /// [`Error::AdbMissing`], [`Error::PhoneLost`] or [`Error::Step`] where
    /// the phone could not be driven.
    fn launch_calendar(&mut self) -> Result<(), Error>;
    /// The screen as `uiautomator` describes it now.
    ///
    /// # Errors
    /// [`Error::AdbMissing`], [`Error::PhoneLost`] or [`Error::Step`] where
    /// the phone could not be driven.
    fn dump(&mut self) -> Result<String, Error>;
    /// Tap a point.
    ///
    /// # Errors
    /// [`Error::AdbMissing`], [`Error::PhoneLost`] or [`Error::Step`] where
    /// the phone could not be driven.
    fn tap(&mut self, x: i32, y: i32) -> Result<(), Error>;
    /// Go back (leave a record's editor, close a menu).
    ///
    /// # Errors
    /// [`Error::AdbMissing`], [`Error::PhoneLost`] or [`Error::Step`] where
    /// the phone could not be driven.
    fn back(&mut self) -> Result<(), Error>;
    /// Drag from one point to another over `millis` milliseconds.
    ///
    /// # Errors
    /// [`Error::AdbMissing`], [`Error::PhoneLost`] or [`Error::Step`] where
    /// the phone could not be driven.
    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), millis: u32) -> Result<(), Error>;
    /// Note what the walk is doing or why it is trying again, for a caller
    /// that keeps a record of the run. Nothing by default.
    fn note(&mut self, _what: &str) {}
    /// Stop ぱち簿 and start it afresh on its calendar: it grows slow after
    /// long use, and in the end crashes out of memory. Bringing it up as it
    /// is, by default.
    ///
    /// # Errors
    /// As [`Device::launch_calendar`].
    fn restart(&mut self) -> Result<(), Error> {
        self.launch_calendar()
    }
}

/// How far a walk goes, how much of each record it reads, and how hard it
/// tries.
#[derive(Debug, Clone, Copy)]
pub struct Walk {
    /// Open each record for what the calendar does not show (投資/回収/メモ/タグ);
    /// where false, only the day, parlor, machine and result are read.
    pub deep: bool,
    /// The month to begin from (this month, by the caller's clock). Where
    /// none, the month the calendar opens on.
    pub start: Option<(i32, u32)>,
    /// How many records there are in all, where known (from [`survey`]):
    /// while fewer have been read, the walk goes on through longer gaps.
    pub expected: Option<usize>,
    /// Stop after this many months in a row hold no record.
    pub empty_months_before_stop: u32,
    /// …or this many, while fewer records than expected have been read.
    pub empty_months_while_short: u32,
    /// Never read more than this many months, whatever else (a ceiling on a
    /// runaway walk).
    pub most_months: u32,
    /// How many times to try a month (or a turn of the month) before giving up.
    pub attempts: u32,
    /// Start ぱち簿 afresh, between months, once it has been read this long
    /// (it is started afresh at the start of a walk too):
    /// it grows slow after long use (a dump twice as slow after ten minutes,
    /// on the SH-RM15) and in the end crashes out of memory. None, never.
    pub fresh_start_every: Option<std::time::Duration>,
}

impl Default for Walk {
    fn default() -> Self {
        Self { deep: true, start: None, expected: None, empty_months_before_stop: 12, empty_months_while_short: 60, most_months: 600, attempts: 5, fresh_start_every: Some(std::time::Duration::from_mins(10)) }
    }
}

/// What a walk has reached, for the caller to show.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    /// The year being read.
    pub year: i32,
    /// The month being read.
    pub month: u32,
    /// Records gathered so far.
    pub records: usize,
}

/// How a walk ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// It read back to where the records end.
    Finished,
    /// The caller stopped it.
    Cancelled,
    /// It could not go on; what it read before is kept.
    Failed(Error),
}

/// What a walk gives back: every record it read, oldest first, however it
/// ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The records read, oldest first.
    pub records: Vec<Record>,
    /// How the walk ended.
    pub end: End,
    /// The months whose results, as read, did not add up to the month's
    /// total even when read again: a record may be missing from them.
    pub unsure_months: Vec<(i32, u32)>,
}

/// What a quick look found, before the slow reading.
#[derive(Debug, Clone, Copy, Default)]
pub struct Survey {
    /// How many records ぱち簿 holds across every month and both games, where
    /// the analysis screen gave it plainly.
    pub records: Option<usize>,
}

/// Count every record in one quick look, without walking the months: ぱち簿's
/// analysis screen shows the lifetime total (`生涯 → 回数`, both games counted),
/// so this opens it, reads the total and comes back. Where the screen is
/// narrowed to one game, or the count does not read — or the analysis screen
/// cannot be read at all — the count is left unknown rather than guessed,
/// and the walk goes on without it.
///
/// # Errors
/// Only a final error ([`Error::is_final`]), or [`Error::NoCalendar`] and the
/// like where the calendar cannot be brought up to begin with;
/// [`Error::Cancelled`] where `cancelled` says so between its steps.
pub fn survey(device: &mut impl Device, cancelled: impl Fn() -> bool) -> Result<Survey, Error> {
    let go_on = || if cancelled() { Err(Error::Cancelled) } else { Ok(()) };
    go_on()?;
    device.restart()?;
    go_on()?;
    let calendar = calendar_now(device)?;
    go_on()?;
    let Some((x, y)) = calendar.analysis else {
        device.note("survey: no 分析 button on the calendar; the count is unknown");
        return Ok(Survey { records: None });
    };
    device.tap(x, y)?;
    go_on()?;
    let records = match device.dump() {
        Ok(xml) => {
            let nodes = scan(&xml);
            let opened = calendar_of(&nodes).is_none();
            // One game unchecked, or a period set under フィルタ (its words beside
            // the button, `id=period_`), narrows the count below the whole.
            let narrowed = nodes.iter().any(|node| (node.is("pachinko") || node.is("slot")) && node.checked == Some(false))
                || first_text(&nodes, "period_").is_some_and(|text| !text.trim().is_empty());
            let count = first_text(&nodes, "count").as_deref().and_then(yen_of).and_then(|count| usize::try_from(count).ok());
            if !opened {
                device.note("survey: the analysis screen did not open; the count is unknown");
                return Ok(Survey { records: None });
            }
            if narrowed {
                device.note("survey: the analysis screen is narrowed (one game, or a period); the count is unknown");
            } else if count.is_none() {
                device.note("survey: no count on the analysis screen; the count is unknown");
            }
            if narrowed { None } else { count }
        }
        Err(error) if error.is_final() => return Err(error),
        Err(error) => {
            device.note(&format!("survey: the analysis screen could not be read ({error}); the count is unknown"));
            None
        }
    };
    // The analysis screen was opened (or may have been): leave it. The walk
    // brings the calendar up again itself, so a failure here only notes.
    go_on()?;
    device.back()?;
    match calendar_now(device) {
        Ok(_) => {}
        Err(error) if error.is_final() => return Err(error),
        Err(error) => device.note(&format!("survey: the calendar did not come back ({error}); the walk will bring it up")),
    }
    Ok(Survey { records })
}

/// Read every record off the phone, month by month from this month back to
/// the earliest, and give them back oldest first — however the walk ends.
///
/// `report` is called as each month is reached, for a caller that shows
/// progress; a caller with nothing to show passes `|_| {}`. `cancelled` is
/// asked often; once it answers true the walk stops. A caller that never
/// cancels passes `|| false`.
pub fn rescue(device: &mut impl Device, walk: Walk, mut report: impl FnMut(Progress), cancelled: impl Fn() -> bool) -> Outcome {
    let mut outcome = Outcome { records: Vec::new(), end: End::Finished, unsure_months: Vec::new() };
    outcome.end = walk_months(device, &walk, &mut outcome, &mut report, &cancelled);
    outcome.records.sort_by(|a, b| a.date.cmp(&b.date));
    outcome
}

/// The walk itself, gathering into `outcome`; how it ended. A step that fails
/// once the caller has asked to stop counts as stopped, not failed.
fn walk_months(device: &mut impl Device, walk: &Walk, outcome: &mut Outcome, report: &mut impl FnMut(Progress), cancelled: &impl Fn() -> bool) -> End {
    match walk_months_or_fail(device, walk, outcome, report, cancelled) {
        End::Failed(_) if cancelled() => End::Cancelled,
        End::Failed(Error::Cancelled) => End::Cancelled,
        end => end,
    }
}

/// [`walk_months`], before a failure on stopping is taken as a stop.
fn walk_months_or_fail(device: &mut impl Device, walk: &Walk, outcome: &mut Outcome, report: &mut impl FnMut(Progress), cancelled: &impl Fn() -> bool) -> End {
    let attempts = walk.attempts.max(1);
    // Afresh from the start: a ぱち簿 left running from before (a survey, an
    // earlier read) has grown slow, and may be near its crash. A fresh start
    // shows this month, where the walk begins.
    let opened = device.restart().and_then(|()| calendar_now(device));
    let mut calendar = match opened {
        Ok(calendar) => calendar,
        Err(error) => return End::Failed(error),
    };
    let mut target = walk.start.unwrap_or_else(|| calendar.shown());
    calendar = match reach(device, calendar, target, attempts, cancelled) {
        Ok(calendar) => calendar,
        Err(error) => return End::Failed(error),
    };
    let mut empty_run = 0;
    let mut started = std::time::Instant::now();
    for _ in 0..walk.most_months {
        if cancelled() {
            return End::Cancelled;
        }
        let before = outcome.records.len();
        report(Progress { year: target.0, month: target.1, records: before });
        // Each record as it is read, so a month of many deep reads does not
        // leave the count standing still for minutes.
        let mut tally = |in_month: usize| report(Progress { year: target.0, month: target.1, records: before + in_month });
        let month = match read_month_surely(device, walk.deep, &mut calendar, target, attempts, &mut tally, cancelled) {
            Ok(month) => month,
            Err(error) => return End::Failed(error),
        };
        let found = month.records.len();
        outcome.records.extend(month.records);
        if month.cancelled {
            return End::Cancelled;
        }
        if !month.adds_up {
            outcome.unsure_months.push(target);
        }
        empty_run = if found == 0 { empty_run + 1 } else { 0 };
        let short = walk.expected.is_some_and(|expected| outcome.records.len() < expected);
        let limit = if short { walk.empty_months_while_short } else { walk.empty_months_before_stop };
        if empty_run >= limit {
            return End::Finished;
        }
        if cancelled() {
            return End::Cancelled;
        }
        target = month_before(target);
        if walk.fresh_start_every.is_some_and(|every| started.elapsed() >= every) {
            device.note(&format!("starting ぱち簿 afresh after {} minutes of reading, before {:04}-{:02}", started.elapsed().as_secs() / 60, target.0, target.1));
            calendar = match device.restart().and_then(|()| calendar_now(device)) {
                Ok(calendar) => calendar,
                Err(error) => return End::Failed(error),
            };
            started = std::time::Instant::now();
        }
        calendar = match reach(device, calendar, target, attempts, cancelled) {
            Ok(calendar) => calendar,
            Err(error) => return End::Failed(error),
        };
    }
    End::Finished
}

/// The calendar now on screen, with nothing over it. Whatever lies over it
/// (an editor, a dialog, the create menu) is left by going back, a step at a
/// time; back is never pressed on the bare calendar, which would close the app.
fn calendar_now(device: &mut impl Device) -> Result<Calendar, Error> {
    for _ in 0..3 {
        let nodes = scan(&device.dump()?);
        if !menu_open(&nodes)
            && let Some(calendar) = calendar_of(&nodes)
        {
            return Ok(calendar);
        }
        device.back()?;
    }
    let nodes = scan(&device.dump()?);
    if menu_open(&nodes) {
        return Err(Error::NoCalendar);
    }
    calendar_of(&nodes).ok_or(Error::NoCalendar)
}

/// A wheel of the year and month picker: its values on screen, top to
/// bottom, with where each is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Wheel {
    /// `(y, value)`, top to bottom.
    rows: Vec<(i32, i32)>,
    /// Where the wheel is across.
    x: i32,
}

impl Wheel {
    /// The value chosen — the middle of three shown — with the height of a
    /// row; none where the wheel does not show three.
    fn chosen(&self) -> Option<(i32, i32, i32)> {
        let [(top, _), (middle, value), _] = self.rows.as_slice() else {
            return None;
        };
        Some((*middle, *value, middle - top))
    }
}

/// The year and month picker the calendar's header opens: its year wheel,
/// its month wheel, and its OK — or none where the screen is not it.
fn picker_of(nodes: &[Node]) -> Option<(Wheel, Wheel, (i32, i32))> {
    let wheel = |unit: char| {
        let mut rows: Vec<(i32, i32, i32)> = nodes
            .iter()
            .filter_map(|node| {
                let value = node.text.as_deref()?.trim().strip_suffix(unit)?.parse().ok()?;
                let (x, y) = node.center()?;
                Some((y, x, value))
            })
            .collect();
        rows.sort_unstable();
        let x = rows.first()?.1;
        Some(Wheel { rows: rows.into_iter().map(|(y, _, value)| (y, value)).collect(), x })
    };
    let ok = nodes.iter().find(|node| node.is("button1")).and_then(Node::center)?;
    Some((wheel('年')?, wheel('月')?, ok))
}

/// Turn the calendar to `target` with the picker its header opens: each
/// wheel turned a row at a time (a row-high swipe turns it by one) until its
/// middle shows the value sought, checked after every turn, then OK, and the
/// month the calendar then shows checked. An error where the picker is not
/// there or does not do as it should; the caller turns the month by swipes.
fn pick_month(device: &mut impl Device, calendar: &Calendar, target: (i32, u32), cancelled: &impl Fn() -> bool) -> Result<Calendar, Error> {
    let failed = || Error::MonthStuck { year: target.0, month: target.1 };
    let (x, y) = calendar.header.ok_or_else(failed)?;
    let wanted_month = i32::try_from(target.1).map_err(|_| failed())?;
    device.tap(x, y)?;
    for _ in 0..80 {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let nodes = scan(&device.dump()?);
        let Some((years, months, ok)) = picker_of(&nodes) else {
            // Not the picker (it did not open, or something else did): leave
            // whatever it is for the calendar.
            calendar_now(device)?;
            return Err(failed());
        };
        let (Some((year_middle, year, row)), Some((month_middle, month, _))) = (years.chosen(), months.chosen()) else {
            device.back()?;
            return Err(failed());
        };
        let (wheel_x, middle, now, wanted) = if year == target.0 { (months.x, month_middle, month, wanted_month) } else { (years.x, year_middle, year, target.0) };
        if now == wanted {
            device.tap(ok.0, ok.1)?;
            let shown = calendar_now(device)?;
            return if shown.shown() == target { Ok(shown) } else { Err(failed()) };
        }
        let below = middle + row;
        if now < wanted {
            device.swipe((wheel_x, below), (wheel_x, middle), 400)?;
        } else {
            device.swipe((wheel_x, middle), (wheel_x, below), 400)?;
        }
    }
    device.back()?;
    Err(failed())
}

/// Turn the calendar to `target` by whatever way gets there: the year and
/// month picker where it is far, else (or where the picker fails) 今月 and
/// swipes, checking every turn.
///
/// # Errors
/// [`Error::MonthStuck`] where it cannot be got to, or the phone's own errors.
pub fn go_to_month(device: &mut impl Device, target: (i32, u32)) -> Result<Calendar, Error> {
    let calendar = calendar_now(device)?;
    turn_to(device, calendar, target, &|| false)
}

/// Turn the calendar from `calendar` to `target`, a month at a time, checking
/// each turn landed: a turn that leaves the month where it was is tried again,
/// and a walk that wanders is given up.
fn turn_to(device: &mut impl Device, mut calendar: Calendar, target: (i32, u32), cancelled: &impl Fn() -> bool) -> Result<Calendar, Error> {
    let stuck_error = Error::MonthStuck { year: target.0, month: target.1 };
    // Far from the month sought — a walk years back after ぱち簿 was started
    // again (it crashes, out of memory, after hours of use), or a launch that
    // brings it up years from where the walk is — the header's picker gets
    // there in a few turns of its wheels.
    if (month_index(calendar.shown()) - month_index(target)).unsigned_abs() > 2 {
        match pick_month(device, &calendar, target, cancelled) {
            Ok(there) => {
                device.note(&format!("{:04}-{:02}: picked from {:04}-{:02} with the year and month picker", target.0, target.1, calendar.year, calendar.month));
                return Ok(there);
            }
            Err(error) if error.is_final() => return Err(error),
            Err(error) => {
                device.note(&format!("{:04}-{:02}: the year and month picker did not get there ({error}); turning by swipes", target.0, target.1));
                calendar = calendar_now(device)?;
            }
        }
    }
    // Still far, 今月 may bring it nearer at a tap (the walk begins from this
    // month). Where it is shown is checked as ever.
    if (month_index(calendar.shown()) - month_index(target)).unsigned_abs() > 2
        && let Some((x, y)) = calendar.this_month
    {
        device.tap(x, y)?;
        let now = calendar_now(device)?;
        device.note(&format!("{:04}-{:02}: 今月 turned the calendar from {:04}-{:02} to {:04}-{:02}", target.0, target.1, calendar.year, calendar.month, now.year, now.month));
        calendar = now;
    }
    let distance = (month_index(calendar.shown()) - month_index(target)).unsigned_abs();
    let most_turns = distance + 6;
    let mut turns = 0;
    let mut stuck = 0;
    loop {
        let gap = month_index(calendar.shown()) - month_index(target);
        if gap == 0 {
            return Ok(calendar);
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if turns >= most_turns || stuck >= 3 {
            return Err(stuck_error);
        }
        let (from, to) = calendar.turn_stroke(gap > 0).ok_or_else(|| stuck_error.clone())?;
        device.swipe(from, to, 300)?;
        turns += 1;
        let next = calendar_now(device)?;
        stuck = if next.shown() == calendar.shown() { stuck + 1 } else { 0 };
        calendar = next;
    }
}

/// [`turn_to`] the target, and where that fails, bring the app up again and
/// try again, up to `attempts` times in all.
fn reach(device: &mut impl Device, calendar: Calendar, target: (i32, u32), attempts: u32, cancelled: &impl Fn() -> bool) -> Result<Calendar, Error> {
    let mut last = match turn_to(device, calendar, target, cancelled) {
        Ok(calendar) => return Ok(calendar),
        Err(error) if error.is_final() => return Err(error),
        Err(error) => error,
    };
    for attempt in 1..attempts {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        device.note(&format!("{:04}-{:02}: could not turn to it ({last}); bringing the app up again (try {} of {attempts})", target.0, target.1, attempt + 1));
        match afresh(device, target, cancelled) {
            Ok(calendar) => return Ok(calendar),
            Err(error) if error.is_final() => return Err(error),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// Bring the app up again (leaving whatever lies over the calendar) and turn
/// to `target`.
fn afresh(device: &mut impl Device, target: (i32, u32), cancelled: &impl Fn() -> bool) -> Result<Calendar, Error> {
    device.launch_calendar()?;
    let calendar = calendar_now(device)?;
    turn_to(device, calendar, target, cancelled)
}

/// One month as read.
struct MonthRead {
    /// The month's records.
    records: Vec<Record>,
    /// Whether the results read add up to the month's total (true where the
    /// total does not read).
    adds_up: bool,
    /// Whether the walk was stopped partway through the month.
    cancelled: bool,
}

/// Read the month `target`, shown on `calendar`, trying it again from a fresh
/// start where a step does not land or the results do not add up. A month
/// whose results still do not add up is given back as read (and so marked);
/// a month that cannot be read at all fails the walk.
#[allow(clippy::too_many_arguments)] // Each is one thing the month's read needs.
fn read_month_surely(device: &mut impl Device, deep: bool, calendar: &mut Calendar, target: (i32, u32), attempts: u32, tally: &mut impl FnMut(usize), cancelled: &impl Fn() -> bool) -> Result<MonthRead, Error> {
    let mut not_adding_up: Option<MonthRead> = None;
    let mut last = Error::NoCalendar;
    for attempt in 0..attempts {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if attempt > 0 {
            device.note(&format!("{:04}-{:02}: reading it again from the app brought up again (try {} of {attempts})", target.0, target.1, attempt + 1));
            match afresh(device, target, cancelled) {
                Ok(fresh) => *calendar = fresh,
                Err(error) if error.is_final() => return Err(error),
                Err(error) => {
                    device.note(&format!("{:04}-{:02}: could not get back to it: {error}", target.0, target.1));
                    last = error;
                    continue;
                }
            }
        }
        match read_month(device, deep, calendar, tally, cancelled) {
            Ok(month) if month.cancelled || month.adds_up => return Ok(month),
            Ok(month) => {
                let sum: i64 = month.records.iter().map(|record| record.revenue).sum();
                device.note(&format!("{:04}-{:02}: {} records adding up to {sum}, but the month's total reads {:?}", target.0, target.1, month.records.len(), calendar.total));
                // Read once more; where it is the same again, the total and
                // the records truly differ, and there is no more to be had.
                if let Some(earlier) = &not_adding_up
                    && earlier.records == month.records
                {
                    return Ok(month);
                }
                not_adding_up = Some(month);
            }
            Err(error) if error.is_final() => return Err(error),
            Err(error) => {
                device.note(&format!("{:04}-{:02}: a step did not land: {error}", target.0, target.1));
                last = error;
            }
        }
    }
    not_adding_up.ok_or(last)
}

/// Read one month's records: select each recorded day, read its list (the
/// whole of it, scrolling where it runs on), and (where `deep`) open each
/// record for the rest. `tally` is told how many of the month's records have
/// been read, after each one (from the start again on a second try).
fn read_month(device: &mut impl Device, deep: bool, calendar: &mut Calendar, tally: &mut impl FnMut(usize), cancelled: &impl Fn() -> bool) -> Result<MonthRead, Error> {
    let mut records: Vec<Record> = Vec::new();
    // The day selected now, as the screen last read showed it.
    let mut selected = calendar.cells.iter().map(|cell| cell.day).find(|&day| calendar.is_selected(day));
    for cell in calendar.recorded_cells() {
        if cancelled() {
            return Ok(MonthRead { records, adds_up: false, cancelled: true });
        }
        let date = iso_date(calendar.year, calendar.month, cell.day);
        let mut page = select_day(device, calendar, selected, &cell, &date)?;
        selected = Some(cell.day);
        let mut shown: Vec<ListRow> = Vec::new();
        for _ in 0..100 {
            let rows = day_list_of(&page);
            let fresh = rows_after(&shown, &rows);
            if fresh.is_empty() {
                break;
            }
            for row in &fresh {
                if cancelled() {
                    return Ok(MonthRead { records, adds_up: false, cancelled: true });
                }
                let mut record = Record { date: date.clone(), parlor: row.parlor.clone(), machine: row.machine.clone(), revenue: row.revenue, ..Record::default() };
                if deep {
                    let full = open_record(device, calendar, cell.day, row, &rows, &date)?;
                    merge(&mut record, full);
                }
                records.push(record);
                tally(records.len());
            }
            if !more_below(&page, &rows) {
                break;
            }
            page = scroll_list(device, &page, calendar, cell.day, &date)?;
            shown = rows;
        }
    }
    let sum: i64 = records.iter().map(|record| record.revenue).sum();
    let adds_up = calendar.total.is_none_or(|total| total == sum);
    Ok(MonthRead { records, adds_up, cancelled: false })
}

/// Select a day on the calendar and give the screen with its list showing,
/// checked to be that day's.
///
/// A tap on the day already selected opens the create menu rather than
/// selecting it, so where `selected` is this day, another day is selected
/// first (which also brings the day's list back to its top). A menu that
/// opens all the same — a tap that landed though its screen did not read —
/// is closed, and the screen looked at again.
fn select_day(device: &mut impl Device, calendar: &Calendar, selected: Option<u32>, cell: &Cell, date: &str) -> Result<Vec<Node>, Error> {
    let not_selected = || Error::DayNotSelected(date.to_owned());
    if selected == Some(cell.day) {
        let other = calendar.cells.iter().find(|other| other.day != cell.day).ok_or_else(not_selected)?;
        device.tap(other.center.0, other.center.1)?;
        let nodes = scan(&device.dump()?);
        if menu_open(&nodes) {
            device.back()?;
            return Err(not_selected());
        }
        if !calendar_of(&nodes).is_some_and(|now| now.shown() == calendar.shown() && now.is_selected(other.day)) {
            return Err(not_selected());
        }
    }
    for &(x, y) in &cell.taps {
        device.tap(x, y)?;
        let mut nodes = scan(&device.dump()?);
        if menu_open(&nodes) {
            device.back()?;
            nodes = scan(&device.dump()?);
            if menu_open(&nodes) {
                return Err(not_selected());
            }
        }
        match calendar_of(&nodes) {
            Some(now) if now.shown() != calendar.shown() => return Err(not_selected()),
            Some(now) if now.is_selected(cell.day) => return Ok(nodes),
            // Not selected yet (the tap missed): the next place to tap.
            Some(_) => {}
            // Something else came up: leave it, then the next place to tap.
            None => {
                calendar_now(device)?;
            }
        }
    }
    Err(not_selected())
}

/// Scroll the day's list on by most of its height, slowly so it does not fling,
/// and give the screen, checked to be still the same day.
fn scroll_list(device: &mut impl Device, page: &[Node], calendar: &Calendar, day: u32, date: &str) -> Result<Vec<Node>, Error> {
    let (left, top, right, bottom) = first_bounds(page, "list").ok_or_else(|| Error::DayNotSelected(date.to_owned()))?;
    let x = i32::midpoint(left, right);
    let margin = (bottom - top) / 10;
    device.swipe((x, bottom - margin), (x, top + margin), 1000)?;
    let nodes = scan(&device.dump()?);
    let same_day = !menu_open(&nodes) && calendar_of(&nodes).is_some_and(|now| now.shown() == calendar.shown() && now.is_selected(day));
    if same_day { Ok(nodes) } else { Err(Error::DayNotSelected(date.to_owned())) }
}

/// Open the record of `row` and read it, then come back: the record must be
/// the row's (its day and result), and the list must be as it was left, with
/// the rows where they were, so the next row's place still holds.
fn open_record(device: &mut impl Device, calendar: &Calendar, day: u32, row: &ListRow, rows: &[ListRow], date: &str) -> Result<Record, Error> {
    for _ in 0..2 {
        device.tap(row.center.0, row.center.1)?;
        // ぱち簿 grows slow after long use: an editor slow to open is looked
        // for again before the row is tapped again (a second tap could land
        // in the editor).
        let mut nodes = scan(&device.dump()?);
        for _ in 0..2 {
            let still_calendar = editor_of(&nodes).is_none() && !menu_open(&nodes) && calendar_of(&nodes).is_some();
            if !still_calendar {
                break;
            }
            nodes = scan(&device.dump()?);
        }
        if let Some(full) = editor_of(&nodes) {
            device.back()?;
            if full.date != date || full.revenue != row.revenue {
                return Err(Error::RecordMismatch(date.to_owned()));
            }
            return back_on_day(device, calendar, day, rows, date).map(|()| full);
        }
        if menu_open(&nodes) || calendar_of(&nodes).is_none() {
            // Something other than the editor came up: leave it, and try once more.
            device.back()?;
        }
    }
    Err(Error::RecordNotOpened(date.to_owned()))
}

/// Check that back from a record's editor found the day's list as it was
/// left, with the rows where they were (so the next row's place still holds).
/// An editor still showing is looked at again: back is slow when ぱち簿 has
/// grown slow. It is not sent back a second time — were the first back only
/// late, the second would land on the calendar and close the app — but left
/// to the month's reading again, which leaves an editor a checked step at a
/// time.
fn back_on_day(device: &mut impl Device, calendar: &Calendar, day: u32, rows: &[ListRow], date: &str) -> Result<(), Error> {
    for _ in 0..4 {
        let back = scan(&device.dump()?);
        if editor_of(&back).is_some() {
            continue;
        }
        let as_left = !menu_open(&back)
            && calendar_of(&back).is_some_and(|now| now.shown() == calendar.shown() && now.is_selected(day))
            && day_list_of(&back) == rows;
        return if as_left { Ok(()) } else { Err(Error::NotBack(date.to_owned())) };
    }
    Err(Error::NotBack(date.to_owned()))
}

/// Fill a list-read record with what only the editor shows, keeping the list's
/// date and the editor's richer fields where both have them.
fn merge(record: &mut Record, full: Record) {
    record.outlay = full.outlay;
    record.takings = full.takings;
    record.cabinet = full.cabinet;
    record.hours = full.hours;
    record.memo = full.memo;
    record.tag = full.tag;
    if !full.parlor.is_empty() {
        record.parlor = full.parlor;
    }
    if !full.machine.is_empty() {
        record.machine = full.machine;
    }
}
