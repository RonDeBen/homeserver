#[path = "events_card.rs"]
mod events_card;
#[path = "month_card.rs"]
mod month_card;

use super::CalendarEventDto;
use chrono::NaiveDate;
use maud::{html, Markup};

pub(crate) fn render_month_card(events: &[CalendarEventDto], month: NaiveDate) -> Markup {
    month_card::render(events, month)
}

pub(crate) fn render_hub_month_card(events: &[CalendarEventDto], month: NaiveDate) -> Markup {
    month_card::render_with_presentation(events, month, true)
}

pub(crate) fn render_events_card(events: &[CalendarEventDto], month: NaiveDate) -> Markup {
    events_card::render(events, month)
}

pub(crate) fn render_view(events: &[CalendarEventDto], month: NaiveDate) -> Markup {
    render_view_with_class(events, month, "calendar-view--page")
}

pub(crate) fn render_hub_view(events: &[CalendarEventDto], month: NaiveDate) -> Markup {
    render_view_with_class(events, month, "calendar-view--hub")
}

fn render_view_with_class(
    events: &[CalendarEventDto],
    month: NaiveDate,
    presentation: &str,
) -> Markup {
    let hub = presentation == "calendar-view--hub";
    let view = if hub { "&view=hub" } else { "" };
    html! {
        div class=(format!("calendar-view {presentation}")) id="calendar-view" data-init=(format!("@get('/calendar/events?month={}{view}')", month.format("%Y-%m"))) {
            @if hub {
                (render_hub_month_card(events, month))
            } @else {
                (render_month_card(events, month))
            }
            (render_events_card(events, month))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};

    fn event(date: NaiveDate, title: &str) -> CalendarEventDto {
        CalendarEventDto {
            source: "test".into(),
            title: title.into(),
            starts_at: Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap()),
            ends_at: None,
            location: None,
            url: None,
            description: None,
        }
    }

    #[test]
    fn month_grid_starts_on_monday_and_has_complete_weeks() {
        let month =
            month_card::CalendarMonth::new(NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(), &[]);
        assert_eq!(month.days.len(), 42);
        assert_eq!(
            month.days.first().unwrap().date,
            NaiveDate::from_ymd_opt(2026, 7, 27).unwrap()
        );
        assert_eq!(
            month.days.last().unwrap().date,
            NaiveDate::from_ymd_opt(2026, 9, 6).unwrap()
        );
        assert_eq!(month.days.iter().filter(|day| day.in_month).count(), 31);
    }

    #[test]
    fn events_are_grouped_by_date_in_the_selected_month() {
        let events = vec![
            event(NaiveDate::from_ymd_opt(2026, 8, 12).unwrap(), "one"),
            event(NaiveDate::from_ymd_opt(2026, 8, 12).unwrap(), "two"),
            event(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(), "outside"),
        ];
        let month =
            month_card::CalendarMonth::new(NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(), &events);
        let selected = month
            .days
            .iter()
            .find(|day| day.date == NaiveDate::from_ymd_opt(2026, 8, 12).unwrap())
            .unwrap();
        assert_eq!(selected.events.len(), 2);
        let adjacent = month
            .days
            .iter()
            .find(|day| day.date == NaiveDate::from_ymd_opt(2026, 9, 1).unwrap())
            .unwrap();
        assert!(!adjacent.in_month);
        assert_eq!(adjacent.events.len(), 1);
    }

    #[test]
    fn hub_view_preserves_its_context_for_navigation_and_live_updates() {
        let month = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let html = render_hub_view(&[], month).into_string();

        assert!(html.contains("calendar-view--hub"));
        assert!(html.contains("/calendar/events?month=2026-08&amp;view=hub"));
        assert!(html.contains("/calendar/view?month=2026-07&amp;view=hub"));
        assert!(html.contains("/calendar/view?month=2026-09&amp;view=hub"));
    }
}
