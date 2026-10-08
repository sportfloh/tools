//! Statistics card: events per day/week chart and average interval.

use crate::db::EventRow;
use crate::import::count_noun;
use crate::time::{
    average_interval_ms, bucket_counts, day_starts, format_interval, short_day_label,
    time_boundaries, week_starts,
};
use leptos::prelude::*;

// ─── Statistics card ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum StatsPeriod {
    Days,
    Weeks,
}

const STATS_DAYS: usize = 30;
const STATS_WEEKS: usize = 12;
// SVG user units; the chart scales to the card width.
const CHART_W: f64 = 300.0;
const CHART_H: f64 = 96.0;
const CHART_TOP: f64 = 6.0;

/// Path for a column with a rounded top (radius `r`) and a square base.
fn column_path(x: f64, w: f64, top: f64, base: f64) -> String {
    let r = 4.0_f64.min(w / 2.0).min(base - top);
    format!(
        "M{x},{base} V{} Q{x},{top} {},{top} H{} Q{},{top} {},{} V{base} Z",
        top + r,
        x + r,
        x + w - r,
        x + w,
        x + w,
        top + r
    )
}

/// Events per day (30 days) or per week (12 weeks) as a column chart, plus the
/// average interval between all events of the topic.
#[component]
pub(crate) fn StatsCard(events: RwSignal<Vec<EventRow>>) -> impl IntoView {
    let period = RwSignal::new(StatsPeriod::Days);
    let selected: RwSignal<Option<usize>> = RwSignal::new(None);

    // (bucket starts, counts) for the chosen period.
    let buckets = Memo::new(move |_| {
        let starts = match period.get() {
            StatsPeriod::Days => day_starts(STATS_DAYS),
            StatsPeriod::Weeks => week_starts(STATS_WEEKS),
        };
        let end = time_boundaries().today_end;
        let counts = events.with(|v| {
            bucket_counts(
                &v.iter().map(|e| e.timestamp_ms).collect::<Vec<_>>(),
                &starts,
                end,
            )
        });
        (starts, counts)
    });
    let interval = Memo::new(move |_| {
        events.with(|v| average_interval_ms(&v.iter().map(|e| e.timestamp_ms).collect::<Vec<_>>()))
    });

    let label = move |start: f64| match period.get_untracked() {
        StatsPeriod::Days => short_day_label(start),
        StatsPeriod::Weeks => format!("Week of {}", short_day_label(start)),
    };
    let count_text = |n: u32| count_noun(n as usize, "event");
    let span_text = move || match period.get() {
        StatsPeriod::Days => format!("last {STATS_DAYS} days"),
        StatsPeriod::Weeks => format!("last {STATS_WEEKS} weeks"),
    };

    // Line above the chart: the tapped/hovered column, or the period total.
    let readout = move || {
        let (starts, counts) = buckets.get();
        match selected.get().filter(|&i| i < counts.len()) {
            Some(i) => format!("{} · {}", label(starts[i]), count_text(counts[i])),
            None => format!("{} in the {}", count_text(counts.iter().sum()), span_text()),
        }
    };
    let aria_summary = move || {
        let (starts, counts) = buckets.get();
        let unit = if period.get() == StatsPeriod::Days {
            "day"
        } else {
            "week"
        };
        let peak = counts
            .iter()
            .enumerate()
            .max_by_key(|&(_, c)| *c)
            .filter(|&(_, c)| *c > 0)
            .map(|(i, c)| format!(", most on {}: {}", label(starts[i]), count_text(*c)))
            .unwrap_or_default();
        format!(
            "Events per {unit}, {}: {} in total{peak}",
            span_text(),
            count_text(counts.iter().sum())
        )
    };

    let columns = move || {
        let (starts, counts) = buckets.get();
        let n = counts.len().max(1) as f64;
        let max = counts.iter().copied().max().unwrap_or(0).max(1) as f64;
        let slot = CHART_W / n;
        // Column width: capped, with a 2px surface gap between neighbours.
        let w = (slot - 2.0).clamp(1.0, 24.0);
        counts
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let x = i as f64 * slot + (slot - w) / 2.0;
                let title = format!("{} · {}", label(starts[i]), count_text(c));
                let mark = if c == 0 {
                    // Hairline so empty days stay visible on the baseline.
                    view! { <rect class="stats-zero" x=x y=CHART_H - 1.0 width=w height=1.0 /> }
                        .into_any()
                } else {
                    let top = CHART_H - (c as f64 / max) * (CHART_H - CHART_TOP);
                    view! { <path class="stats-col" d=column_path(x, w, top, CHART_H) /> }
                        .into_any()
                };
                view! {
                    <g
                        class="stats-slot"
                        class:selected=move || selected.get() == Some(i)
                        on:pointerenter=move |_| selected.set(Some(i))
                        on:click=move |_| selected.set(Some(i))
                    >
                        <title>{title}</title>
                        // Hit target: the whole slot, taller and wider than the mark.
                        <rect class="stats-hit" x=i as f64 * slot y=0.0 width=slot height=CHART_H />
                        {mark}
                    </g>
                }
            })
            .collect_view()
    };

    let axis_left = move || {
        let (starts, _) = buckets.get();
        starts
            .first()
            .map(|&s| short_day_label(s))
            .unwrap_or_default()
    };
    // The only value scale: the tallest column's count.
    let axis_max = move || {
        let max = buckets.with(|(_, counts)| counts.iter().copied().max().unwrap_or(0));
        let unit = if period.get() == StatsPeriod::Days {
            "day"
        } else {
            "week"
        };
        (max > 0).then(|| format!("max {max}/{unit}"))
    };
    let axis_right = move || match period.get() {
        StatsPeriod::Days => "Today",
        StatsPeriod::Weeks => "This week",
    };
    let set_period = move |p: StatsPeriod| {
        selected.set(None);
        period.set(p);
    };

    view! {
        <section class="stats-card" aria-label="Statistics">
            <div class="stats-head">
                <span class="stats-readout">{readout}</span>
                <div class="stats-toggle" role="group" aria-label="Chart period">
                    <button
                        type="button"
                        aria-pressed=move || (period.get() == StatsPeriod::Days).to_string()
                        on:click=move |_| set_period(StatsPeriod::Days)
                    >
                        "30 days"
                    </button>
                    <button
                        type="button"
                        aria-pressed=move || (period.get() == StatsPeriod::Weeks).to_string()
                        on:click=move |_| set_period(StatsPeriod::Weeks)
                    >
                        "12 weeks"
                    </button>
                </div>
            </div>
            <svg
                class="stats-chart"
                viewBox=format!("0 0 {CHART_W} {CHART_H}")
                role="img"
                aria-label=aria_summary
                on:pointerleave=move |_| selected.set(None)
            >
                <line class="stats-baseline" x1=0.0 x2=CHART_W y1=CHART_H - 0.5 y2=CHART_H - 0.5 />
                {columns}
            </svg>
            <div class="stats-axis" aria-hidden="true">
                <span>{axis_left}</span>
                <span>{axis_max}</span>
                <span>{axis_right}</span>
            </div>
            <p class="stats-interval">
                {move || match interval.get() {
                    Some(ms) => format!("Ø {}", format_interval(ms)),
                    None => "Log a second event to see the average interval".to_string(),
                }}
            </p>
        </section>
    }
}
