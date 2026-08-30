use super::super::CalendarEventDto;
use crate::card::{Card, CardSize, Recipe};
use chrono::{Datelike, Months, NaiveDate, Utc, Weekday};
use maud::{html, Markup};

pub(crate) fn render(events: &[CalendarEventDto], month: NaiveDate) -> Markup {
    render_with_presentation(events, month, false)
}

pub(crate) fn render_with_presentation(
    events: &[CalendarEventDto],
    month: NaiveDate,
    hub: bool,
) -> Markup {
    let model = CalendarMonth::new(month, events);
    html! { div id="calendar-month-card" { (Card::new("calendar-month-card", html! { div.calendar-month { (calendar_panel(&model, hub)) } }).recipe(Recipe::Information).size(CardSize::M).label("Calendar").without_repairs().render_today()) } }
}

#[derive(Debug)]
pub(super) struct CalendarDay<'a> {
    pub(super) date: NaiveDate,
    pub(super) in_month: bool,
    pub(super) events: Vec<&'a CalendarEventDto>,
}

#[derive(Debug)]
pub(super) struct CalendarMonth<'a> {
    pub(super) month: NaiveDate,
    pub(super) days: Vec<CalendarDay<'a>>,
}

impl<'a> CalendarMonth<'a> {
    pub(super) fn new(month: NaiveDate, events: &'a [CalendarEventDto]) -> Self {
        let month = month.with_day(1).unwrap();
        let grid_start = month - chrono::Days::new(month.weekday().num_days_from_monday() as u64);
        let next_month = month.checked_add_months(Months::new(1)).unwrap();
        let grid_end = next_month
            + chrono::Days::new(
                (Weekday::Sun.num_days_from_monday() - next_month.weekday().num_days_from_monday())
                    as u64,
            );
        let mut days = Vec::new();
        let mut date = grid_start;
        while date <= grid_end {
            days.push(CalendarDay {
                date,
                in_month: date.month() == month.month() && date.year() == month.year(),
                events: events
                    .iter()
                    .filter(|event| event.starts_at.date_naive() == date)
                    .collect(),
            });
            date = date + chrono::Days::new(1);
        }
        Self { month, days }
    }
}

fn calendar_panel(month: &CalendarMonth<'_>, hub: bool) -> Markup {
    let previous = (month.month - chrono::Days::new(1)).with_day(1).unwrap();
    let next = month.month.checked_add_months(Months::new(1)).unwrap();
    let today = Utc::now().date_naive();
    let month_param = |date: NaiveDate| date.format("%Y-%m").to_string();
    let view = if hub { "hub" } else { "page" };
    html! {
        header.calendar-month__header {
            a.calendar-month__nav href=(format!("/calendar?month={}", month_param(previous))) data-on:click__prevent=(format!("@get('/calendar/view?month={}&view={view}')", month_param(previous))) aria-label="Previous month" { "‹" }
            h3 { (month.month.format("%B %Y").to_string()) }
            a.calendar-month__nav href=(format!("/calendar?month={}", month_param(next))) data-on:click__prevent=(format!("@get('/calendar/view?month={}&view={view}')", month_param(next))) aria-label="Next month" { "›" }
        }
        div.calendar-month__weekdays { @for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] { span { (day) } } }
        div.calendar-month__days {
            @for day in &month.days {
                a href=(format!("#calendar-event-{}", day.date.format("%Y-%m-%d"))) class=(format!("calendar-day{}{}{}", if day.in_month { "" } else { " calendar-day--outside" }, if day.date == today { " calendar-day--today" } else { "" }, if day.events.is_empty() { "" } else { " calendar-day--has-events" })) style=[day.events.first().map(|event| format!("--calendar-label: url('/static/boro/labels/label-{}-01.webp')", event_label(event)))] title=[if day.events.is_empty() { None } else { Some(day.events.iter().map(|e| e.title.as_str()).collect::<Vec<_>>().join(", ")) }] { span.calendar-day__number { (day.date.day()) } }
            }
        }
    }
}

fn event_label(event: &CalendarEventDto) -> &'static str {
    ["charcoal", "faded-blue", "rust", "olive"][event_kind(event)]
}
fn event_kind(event: &CalendarEventDto) -> usize {
    event
        .source
        .bytes()
        .fold(0usize, |sum, byte| sum + byte as usize)
        % 4
}
