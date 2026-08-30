use super::super::CalendarEventDto;
use crate::card::{Card, CardSize, Material, Recipe};
use crate::views::safe_url;
use chrono::NaiveDate;
use maud::{html, Markup};

pub(crate) fn render(events: &[CalendarEventDto], month: NaiveDate) -> Markup {
    html! { div id="calendar-events-card" { (Card::new("calendar-events-card", html! {
        div.calendar-events {
            header.calendar-events__header { h3 { (month.format("%B %Y").to_string()) " · Upcoming events" } }
            div.calendar-events__list { (event_list(events)) }
        }
    }).recipe(Recipe::Information).size(CardSize::Tall).material(Material::FadedBlue).label("Upcoming events").without_repairs().render_today()) } }
}

fn event_kind(event: &CalendarEventDto) -> usize {
    event
        .source
        .bytes()
        .fold(0usize, |sum, byte| sum + byte as usize)
        % 4
}

/// Event scraps use a visual sequence independent from the event's source.
/// Keeping this separate from `event_kind` means source semantics don't leak
/// into the material vocabulary.
fn event_material(index: usize) -> Material {
    match index % 4 {
        0 | 2 => Material::Linen,
        1 => Material::Rust,
        _ => Material::Indigo,
    }
}

fn event_list(events: &[CalendarEventDto]) -> Markup {
    html! {
        @if events.is_empty() { p.empty { "No upcoming events." } } @else {
            div.calendar-event-list {
                @for (index, e) in events.iter().enumerate() {
                    article class=(format!("calendar-event material--{}", event_material(index).slug())) id=[if !events[..index].iter().any(|prior| prior.starts_at.date_naive() == e.starts_at.date_naive()) { Some(format!("calendar-event-{}", e.starts_at.format("%Y-%m-%d"))) } else { None }] {
                        div.calendar-event__date { span.calendar-event__weekday { (e.starts_at.format("%a").to_string()) } span.calendar-event__day { (e.starts_at.format("%-d").to_string()) } }
                        div.calendar-event__content {
                            div.calendar-event__main {
                                time.calendar-event__time datetime=(e.starts_at.to_rfc3339()) { (e.starts_at.format("%H:%M").to_string()) }
                                span.calendar-event__title { @match e.url.as_deref().and_then(safe_url) { Some(url) => a href=(url) target="_blank" rel="noopener noreferrer" { (e.title) }, None => (e.title), } }
                            }
                            @if let Some(loc) = &e.location { span.calendar-event__location { (loc) } }
                        }
                        span class=(format!("calendar-event__kind calendar-event__kind--{}", event_kind(e))) aria-hidden="true" {}
                    }
                }
            }
        }
    }
}
